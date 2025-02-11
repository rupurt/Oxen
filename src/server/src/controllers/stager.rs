use crate::errors::OxenHttpError;
use crate::helpers::get_repo;
use crate::params::{
    app_data, df_opts_query, parse_resource, path_param, DFOptsQuery, PageNumQuery,
};

use actix_files::NamedFile;

use liboxen::constants::TABLE_NAME;
use liboxen::core::cache::commit_cacher;
use liboxen::core::db::{df_db, staged_df_db};
use liboxen::core::index::mod_stager;
use liboxen::core::index::remote_df_stager::{get_row_id, get_row_idx};
use liboxen::error::OxenError;
use liboxen::model::diff::DiffResult;
use liboxen::model::entry::mod_entry::NewMod;
use liboxen::model::metadata::metadata_image::ImgResize;
use liboxen::model::CommitEntry;
use liboxen::model::{
    entry::mod_entry::ModType, Branch, ContentType, LocalRepository, NewCommitBody, Schema,
};
use liboxen::opts::DFOpts;
use liboxen::util::{self, paginate};
use liboxen::view::compare::{CompareTabular, CompareTabularResponseWithDF};
use liboxen::view::entry::{
    PaginatedMetadataEntries, PaginatedMetadataEntriesResponse, ResourceVersion,
};
use liboxen::view::json_data_frame_view::{JsonDataFrameRowResponse, JsonDataFrameSource};
use liboxen::view::remote_staged_status::{DFIsEditableResponse, RemoteStagedStatus};
use liboxen::view::{
    CommitResponse, FilePathsResponse, JsonDataFrameView, JsonDataFrameViewResponse,
    JsonDataFrameViews, RemoteStagedStatusResponse, StatusMessage,
};
use liboxen::{api, constants, core::index};

use actix_web::{web, web::Bytes, HttpRequest, HttpResponse};

use actix_multipart::Multipart;
use actix_web::Error;
use futures_util::TryStreamExt as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub async fn status_dir(
    req: HttpRequest,
    query: web::Query<PageNumQuery>,
) -> actix_web::Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;
    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, &namespace, &repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let page_num = query.page.unwrap_or(constants::DEFAULT_PAGE_NUM);
    let page_size = query.page_size.unwrap_or(constants::DEFAULT_PAGE_SIZE);

    log::debug!(
        "{} resource {namespace}/{repo_name}/{resource}",
        liboxen::current_function!()
    );

    get_dir_status_for_branch(
        &repo,
        &resource.branch.ok_or(OxenHttpError::NotFound)?.name,
        &identifier,
        &resource.file_path,
        page_num,
        page_size,
    )
}

pub async fn diff_file(req: HttpRequest) -> actix_web::Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;
    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;

    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;

    // Need resource to have a branch
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    // Get the branch repo for remote staging
    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    let diff_result =
        api::local::diff::diff_staged_df(&repo, &branch, resource.file_path.clone(), &identifier)?;
    let diff = match diff_result {
        DiffResult::Tabular(diff) => diff,
        _ => {
            return Err(OxenHttpError::BadRequest(
                "Expected tabular diff result".into(),
            ))
        }
    };
    // TODO expensive clone
    let diff_df = diff.contents.clone();
    let diff_view = CompareTabular::from(diff);

    // TODO: Oxen schema vs polars inferred schema

    let diff_schema = Schema::from_polars(&diff_df.schema().clone());

    let opts = DFOpts::empty();
    let diff_json_df = JsonDataFrameViews::from_df_and_opts(diff_df, diff_schema, &opts);

    let response = CompareTabularResponseWithDF {
        data: diff_json_df,
        dfs: diff_view,
        status: StatusMessage::resource_found(),
        messages: vec![],
    };

    // The path to the actual file is just the working directory here...

    Ok(HttpResponse::Ok().json(response))
}

pub async fn diff_df(
    req: HttpRequest,
    query: web::Query<DFOptsQuery>,
) -> actix_web::Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;
    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let identifier = path_param(&req, "identifier")?;

    let mut opts = DFOpts::empty();
    opts = df_opts_query::parse_opts(&query, &mut opts);

    opts.page = Some(query.page.unwrap_or(constants::DEFAULT_PAGE_NUM));
    opts.page_size = Some(query.page_size.unwrap_or(constants::DEFAULT_PAGE_SIZE));

    // Remote staged calls must be on a branch
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    let staged_db_path =
        mod_stager::mods_df_db_path(&repo, &branch, &identifier, &resource.file_path);

    let conn = df_db::get_connection(staged_db_path)?;

    let diff_df = staged_df_db::df_diff(&conn)?;

    let df_schema = df_db::get_schema(&conn, TABLE_NAME)?;

    let df_views = JsonDataFrameViews::from_df_and_opts(diff_df, df_schema, &opts);

    let resource = ResourceVersion {
        path: resource.file_path.to_string_lossy().to_string(),
        version: resource.version(),
    };

    let resource = JsonDataFrameViewResponse {
        data_frame: df_views,
        status: StatusMessage::resource_found(),
        resource: Some(resource),
        commit: None,
        derived_resource: None,
    };

    Ok(HttpResponse::Ok().json(resource))
}

pub async fn get_file(
    req: HttpRequest,
    query: web::Query<ImgResize>,
) -> Result<NamedFile, OxenHttpError> {
    let app_data = app_data(&req)?;
    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let identifier = path_param(&req, "identifier")?;

    // Remote staged calls must be on a branch
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    // The path in a remote staged context is just the working path of the branch repo
    let file_path = branch_repo.path.join(resource.file_path);

    log::debug!("got staged file path {:?}", file_path);

    let img_resize = query.into_inner();

    if img_resize.width.is_some() || img_resize.height.is_some() {
        let resized_path = util::fs::resized_path_for_staged_entry(
            repo,
            &file_path,
            img_resize.width,
            img_resize.height,
        )?;

        util::fs::resize_cache_image(&file_path, &resized_path, img_resize)?;
        return Ok(NamedFile::open(resized_path)?);
    }
    Ok(NamedFile::open(file_path)?)
}

async fn save_parts(
    repo: &LocalRepository,
    branch: &Branch,
    user_id: &str,
    directory: &Path,
    mut payload: Multipart,
) -> Result<Vec<PathBuf>, Error> {
    let mut files: Vec<PathBuf> = vec![];
    // iterate over multipart stream
    while let Some(mut field) = payload.try_next().await? {
        // A multipart/form-data stream has to contain `content_disposition`
        let content_disposition = field.content_disposition();

        let upload_filename = content_disposition
            .get_filename()
            .map_or_else(|| Uuid::new_v4().to_string(), sanitize_filename::sanitize);

        log::debug!("Got uploaded file name: {upload_filename:?}");

        let staging_dir = index::remote_dir_stager::branch_staging_dir(repo, branch, user_id);
        let full_dir = staging_dir.join(directory);

        if !full_dir.exists() {
            std::fs::create_dir_all(&full_dir)?;
        }

        let filepath = full_dir.join(&upload_filename);
        let filepath_cpy = full_dir.join(&upload_filename);
        log::debug!("stager::save_file writing file to {:?}", filepath);

        // File::create is blocking operation, use threadpool
        let mut f = web::block(|| std::fs::File::create(filepath)).await??;

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.try_next().await? {
            // filesystem operations are blocking, we have to use threadpool
            f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
        }
        files.push(filepath_cpy);
    }

    Ok(files)
}

fn get_content_type(req: &HttpRequest) -> Option<&str> {
    req.headers().get("content-type")?.to_str().ok()
}

pub async fn df_get_row(req: HttpRequest) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let row_id = path_param(&req, "row_id")?;

    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    // Have to initialize this branch repo before we can do any operations on it
    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    // TODONOW: error handling if it has not been indexed

    let row_df = index::remote_df_stager::get_row_by_id(
        &repo,
        &branch,
        resource.file_path,
        &identifier,
        &row_id,
    )?;

    let row_id = get_row_id(&row_df)?;
    let row_index = get_row_idx(&row_df)?;

    let opts = DFOpts::empty();
    let row_schema = Schema::from_polars(&row_df.schema().clone());
    let row_df_source = JsonDataFrameSource::from_df(&row_df, &row_schema);
    let row_df_view = JsonDataFrameView::from_df_opts(row_df, row_schema, &opts);

    let response = JsonDataFrameRowResponse {
        data_frame: JsonDataFrameViews {
            source: row_df_source,
            view: row_df_view,
        },
        commit: None,
        derived_resource: None,
        status: StatusMessage::resource_found(),
        resource: None,
        row_id,
        row_index,
    };

    Ok(HttpResponse::Ok().json(response))
}

pub async fn df_add_row(req: HttpRequest, bytes: Bytes) -> Result<HttpResponse, OxenHttpError> {
    log::debug!("in the df add row controller");
    let app_data = app_data(&req)?;
    log::debug!("trying to get repo at path {:?}", app_data.path);

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    log::debug!("with namespace {:?}", namespace);
    log::debug!("with repo_name {:?}", repo_name);
    log::debug!("with identifier {:?}", identifier);
    let repo = get_repo(&app_data.path, namespace.clone(), repo_name.clone())?;
    let resource = parse_resource(&req, &repo)?;

    // TODO: better error handling for content-types
    let content_type_str = get_content_type(&req).unwrap_or("text/plain");
    let content_type = ContentType::from_http_content_type(content_type_str)?;

    let data = String::from_utf8(bytes.to_vec()).expect("Could not parse bytes as utf8");

    // TODO clean up
    if content_type != ContentType::Json {
        return Err(OxenHttpError::BadRequest(
            "Unsupported content type, must be json".to_string().into(),
        ));
    }

    // If the json has an outer property of "data", serialize the inner object
    let json_value: serde_json::Value = serde_json::from_str(&data)?;
    // TODO yeesh
    let data = if let Some(data_obj) = json_value.get("data") {
        serde_json::to_string(data_obj)?
    } else {
        data
    };

    log::debug!("we got data {:?}", data);

    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    // Have to initialize this branch repo before we can do any operations on it
    let branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;
    log::debug!(
        "stager::df_add_row repo {resource} -> staged repo path {:?}",
        repo.path
    );
    log::debug!(
        "stager::df_add_row branch repo {resource} -> staged repo path {:?}",
        branch_repo.path
    );

    let commit = api::local::commits::get_by_id(&repo, &branch.commit_id)?.ok_or(
        OxenError::revision_not_found(branch.commit_id.to_owned().into()),
    )?;

    // If entry does not exist, create it, and stage it with the first row being the data.

    let entry = api::local::entries::get_commit_entry(&repo, &commit, &resource.file_path)?
        .ok_or(OxenError::entry_does_not_exist(resource.file_path.clone()))?;

    let new_mod = NewMod {
        content_type,
        mod_type: ModType::Append,
        entry,
        data,
    };

    let row_df = liboxen::core::index::mod_stager::add_row(&repo, &branch, &identifier, &new_mod)?;
    let row_id: Option<String> = get_row_id(&row_df)?;
    let row_index: Option<usize> = get_row_idx(&row_df)?;

    let opts = DFOpts::empty();
    let row_schema = Schema::from_polars(&row_df.schema().clone());
    let row_df_source = JsonDataFrameSource::from_df(&row_df, &row_schema);
    let row_df_view = JsonDataFrameView::from_df_opts(row_df, row_schema, &opts);

    let response = JsonDataFrameRowResponse {
        data_frame: JsonDataFrameViews {
            source: row_df_source,
            view: row_df_view,
        },
        commit: None,
        derived_resource: None,
        status: StatusMessage::resource_found(),
        resource: None,
        row_id,
        row_index,
    };

    Ok(HttpResponse::Ok().json(response))
}

pub async fn df_restore_row(req: HttpRequest) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req).unwrap();

    let namespace: &str = req.match_info().get("namespace").unwrap();
    let repo_name: &str = req.match_info().get("repo_name").unwrap();
    let identifier = req.match_info().get("identifier").unwrap();
    let row_id = req.match_info().get("row_id").unwrap();

    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let commit = api::local::commits::get_by_id(&repo, &branch.commit_id)?.ok_or(
        OxenError::revision_not_found(branch.commit_id.to_owned().into()),
    )?;

    let entry = api::local::entries::get_commit_entry(&repo, &commit, &resource.file_path)?
        .ok_or(OxenError::entry_does_not_exist(resource.file_path.clone()))?;

    let restored_row = mod_stager::restore_row(&repo, &branch, &entry, identifier, row_id)?;

    let row_index = get_row_idx(&restored_row)?;
    let row_id = get_row_id(&restored_row)?;

    log::debug!("Restored row in controller is {:?}", restored_row);
    let schema = Schema::from_polars(&restored_row.schema());
    Ok(HttpResponse::Ok().json(JsonDataFrameRowResponse {
        data_frame: JsonDataFrameViews {
            source: JsonDataFrameSource::from_df(&restored_row, &schema),
            view: JsonDataFrameView::from_df_opts(restored_row, schema, &DFOpts::empty()),
        },
        commit: None,
        derived_resource: None,
        status: StatusMessage::resource_updated(),
        resource: None,
        row_id,
        row_index,
    }))
}

pub async fn df_modify_row(req: HttpRequest, bytes: Bytes) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req).unwrap();

    let namespace: &str = req.match_info().get("namespace").unwrap();
    let repo_name: &str = req.match_info().get("repo_name").unwrap();
    let identifier = req.match_info().get("identifier").unwrap();
    let row_id = req.match_info().get("row_id").unwrap();

    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let commit = api::local::commits::get_by_id(&repo, &branch.commit_id)?.ok_or(
        OxenError::revision_not_found(branch.commit_id.to_owned().into()),
    )?;

    let entry = api::local::entries::get_commit_entry(&repo, &commit, &resource.file_path)?
        .ok_or(OxenError::entry_does_not_exist(resource.file_path.clone()))?;

    // TODO: better error handling for content-types
    let content_type_str = get_content_type(&req).unwrap_or("text/plain");
    let content_type = ContentType::from_http_content_type(content_type_str)?;

    let data = String::from_utf8(bytes.to_vec()).expect("Could not parse bytes as utf8");

    // TODO clean up
    if content_type != ContentType::Json {
        return Err(OxenHttpError::BadRequest(
            "Unsupported content type, must be json".to_string().into(),
        ));
    }

    // If the json has an outer property of "data", serialize the inner object
    let json_value: serde_json::Value = serde_json::from_str(&data)?;
    // TODO yeesh
    let data = if let Some(data_obj) = json_value.get("data") {
        serde_json::to_string(data_obj)?
    } else {
        data
    };

    log::debug!("we got data {:?}", data);

    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    // Have to initialize this branch repo before we can do any operations on it
    let branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, identifier)?;
    log::debug!(
        "stager::df_modify_row repo {resource} -> staged repo path {:?}",
        repo.path
    );
    log::debug!(
        "stager::df_modify_row branch repo {resource} -> staged repo path {:?}",
        branch_repo.path
    );

    let new_mod = NewMod {
        content_type,
        mod_type: ModType::Modify,
        entry,
        data,
    };

    // TODO: Add, delete, and modify should use the resource schema here.
    let modified_row = mod_stager::modify_row(&repo, &branch, identifier, row_id, &new_mod)?;

    let row_index = get_row_idx(&modified_row)?;
    let row_id = get_row_id(&modified_row)?;

    log::debug!("Modified row in controller is {:?}", modified_row);
    let schema = Schema::from_polars(&modified_row.schema());
    Ok(HttpResponse::Ok().json(JsonDataFrameRowResponse {
        data_frame: JsonDataFrameViews {
            source: JsonDataFrameSource::from_df(&modified_row, &schema),
            view: JsonDataFrameView::from_df_opts(modified_row, schema, &DFOpts::empty()),
        },
        commit: None,
        derived_resource: None,
        status: StatusMessage::resource_updated(),
        resource: None,
        row_id,
        row_index,
    }))
}

pub async fn df_delete_row(req: HttpRequest, _bytes: Bytes) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req).unwrap();

    let namespace: &str = req.match_info().get("namespace").unwrap();
    let repo_name: &str = req.match_info().get("repo_name").unwrap();
    let user_id: &str = req.match_info().get("identifier").unwrap();
    let row_id: &str = req.match_info().get("row_id").unwrap();
    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let commit = api::local::commits::get_by_id(&repo, &branch.commit_id)?.ok_or(
        OxenError::revision_not_found(branch.commit_id.to_owned().into()),
    )?;
    let entry = api::local::entries::get_commit_entry(&repo, &commit, &resource.file_path)?
        .ok_or(OxenError::entry_does_not_exist(resource.file_path.clone()))?;

    delete_row(&repo, &branch, user_id, &entry, row_id.to_string())
}

fn delete_row(
    repo: &LocalRepository,
    branch: &Branch,
    user_id: &str,
    entry: &CommitEntry,
    uuid: String,
) -> Result<HttpResponse, OxenHttpError> {
    let new_mod = NewMod {
        entry: entry.clone(),
        data: "".to_string(),
        mod_type: ModType::Delete,
        content_type: ContentType::Json,
    };
    match liboxen::core::index::mod_stager::delete_row(repo, branch, user_id, &uuid, &new_mod) {
        Ok(df) => {
            let schema = Schema::from_polars(&df.schema());
            Ok(HttpResponse::Ok().json(JsonDataFrameRowResponse {
                data_frame: JsonDataFrameViews {
                    source: JsonDataFrameSource::from_df(&df, &schema),
                    view: JsonDataFrameView::from_df_opts(df, schema, &DFOpts::empty()),
                },
                commit: None,
                derived_resource: None,
                status: StatusMessage::resource_deleted(),
                resource: None,
                row_id: None,
                row_index: None,
            }))
        }
        Err(OxenError::Basic(err)) => {
            log::error!(
                "unable to delete data to file {:?}/{:?} uuid {}. Err: {}",
                branch.name,
                entry.path,
                uuid,
                err
            );
            Ok(HttpResponse::BadRequest().json(StatusMessage::error(err.to_string())))
        }
        Err(err) => {
            log::error!(
                "unable to delete data to file {:?}/{:?} uuid {}. Err: {}",
                branch.name,
                entry.path,
                uuid,
                err
            );
            Ok(HttpResponse::BadRequest().json(StatusMessage::error(format!("{err:?}"))))
        }
    }
}

pub async fn add_file(req: HttpRequest, payload: Multipart) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let user_id = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, namespace, &repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    log::debug!("stager::stage repo name {repo_name} -> {:?}", resource);

    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &user_id)?;
    log::debug!(
        "stager::stage file repo {resource} -> staged repo path {:?}",
        repo.path
    );

    let files = save_parts(&repo, &branch, &user_id, &resource.file_path, payload).await?;
    let mut ret_files = vec![];

    for file in files.iter() {
        log::debug!("stager::stage file {:?}", file);
        let file_path =
            index::remote_dir_stager::stage_file(&repo, &branch_repo, &branch, &user_id, file)?;
        log::debug!("stager::stage ✅ success! staged file {:?}", file_path);
        ret_files.push(file_path);
    }
    Ok(HttpResponse::Ok().json(FilePathsResponse {
        status: StatusMessage::resource_created(),
        paths: ret_files,
    }))
}

pub async fn commit(req: HttpRequest, body: String) -> Result<HttpResponse, Error> {
    let app_data = app_data(&req)?;

    let namespace: &str = req.match_info().get("namespace").unwrap();
    let repo_name: &str = req.match_info().get("repo_name").unwrap();
    let user_id: &str = req.match_info().get("identifier").unwrap();
    let branch_name: &str = req.match_info().query("branch");

    log::debug!("stager::commit got body: {body}");

    let data: Result<NewCommitBody, serde_json::Error> = serde_json::from_str(&body);

    let data = match data {
        Ok(data) => data,
        Err(err) => {
            log::error!("unable to parse commit data. Err: {}\n{}", err, body);
            return Ok(HttpResponse::BadRequest().json(StatusMessage::error(err.to_string())));
        }
    };

    log::debug!("stager::commit repo name {repo_name} -> {branch_name}");
    match api::local::repositories::get_by_namespace_and_name(&app_data.path, namespace, repo_name)
    {
        Ok(Some(repo)) => match api::local::branches::get_by_name(&repo, branch_name) {
            Ok(Some(branch)) => {
                let branch_repo =
                    index::remote_dir_stager::init_or_get(&repo, &branch, user_id).unwrap();
                match index::remote_dir_stager::commit(&repo, &branch_repo, &branch, &data, user_id)
                {
                    Ok(commit) => {
                        log::debug!("stager::commit ✅ success! commit {:?}", commit);

                        // Clone the commit so we can move it into the thread
                        let ret_commit = commit.clone();

                        // Start computing data about the commit in the background thread
                        std::thread::spawn(move || {
                            log::debug!("Processing commit {:?} on repo {:?}", commit, repo.path);
                            let force = false;
                            match commit_cacher::run_all(&repo, &commit, force) {
                                Ok(_) => {
                                    log::debug!(
                                        "Success processing commit {:?} on repo {:?}",
                                        commit,
                                        repo.path
                                    );
                                }
                                Err(err) => {
                                    log::error!(
                                        "Could not process commit {:?} on repo {:?}: {}",
                                        commit,
                                        repo.path,
                                        err
                                    );
                                }
                            }
                        });

                        Ok(HttpResponse::Ok().json(CommitResponse {
                            status: StatusMessage::resource_created(),
                            commit: ret_commit,
                        }))
                    }
                    Err(err) => {
                        log::error!("unable to commit branch {:?}. Err: {}", branch_name, err);
                        Ok(HttpResponse::UnprocessableEntity()
                            .json(StatusMessage::error(format!("{err:?}"))))
                    }
                }
            }
            Ok(None) => {
                log::debug!("unable to find branch {}", branch_name);
                Ok(HttpResponse::NotFound().json(StatusMessage::resource_not_found()))
            }
            Err(err) => {
                log::error!("Could not commit staged: {:?}", err);
                Ok(
                    HttpResponse::InternalServerError()
                        .json(StatusMessage::internal_server_error()),
                )
            }
        },
        Ok(None) => {
            log::debug!("unable to find repo {}", repo_name);
            Ok(HttpResponse::NotFound().json(StatusMessage::resource_not_found()))
        }
        Err(err) => {
            log::error!("Could not commit staged: {:?}", err);
            Ok(HttpResponse::InternalServerError().json(StatusMessage::internal_server_error()))
        }
    }
}

pub async fn clear_modifications(req: HttpRequest) -> HttpResponse {
    let app_data = app_data(&req).unwrap();
    let namespace: &str = req.match_info().get("namespace").unwrap();
    let repo_name: &str = req.match_info().get("repo_name").unwrap();
    let user_id: &str = req.match_info().get("identifier").unwrap();
    let resource: PathBuf = req.match_info().query("resource").parse().unwrap();

    log::debug!(
        "stager::clear_modifications repo name {repo_name}/{}",
        resource.to_string_lossy()
    );
    match api::local::repositories::get_by_namespace_and_name(&app_data.path, namespace, repo_name)
    {
        Ok(Some(repo)) => match api::local::resource::parse_resource(&repo, &resource) {
            Ok(Some((_, branch_name, file_name))) => {
                clear_staged_modifications_on_branch(&repo, &branch_name, user_id, &file_name)
            }
            Ok(None) => {
                log::error!("unable to find resource {:?}", resource);
                HttpResponse::NotFound().json(StatusMessage::resource_not_found())
            }
            Err(err) => {
                log::error!("Could not parse resource  {repo_name} -> {err}");
                HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
            }
        },
        Ok(None) => {
            log::error!("unable to find repo {}", repo_name);
            HttpResponse::NotFound().json(StatusMessage::resource_not_found())
        }
        Err(err) => {
            log::error!("Error getting repo by name {repo_name} -> {err}");
            HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
        }
    }
}

pub async fn delete_file(req: HttpRequest) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;
    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let user_id = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, namespace, &repo_name)?;
    let resource = parse_resource(&req, &repo)?;

    // Staging calls must be on a branch
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    // Get commit for branch head
    // let commit = api::local::commits::get_by_id(&repo, &branch.commit_id)?
    //     .ok_or(OxenError::resource_not_found(branch.commit_id.clone()))?;

    log::debug!(
        "stager::delete_file repo name {repo_name}/{}",
        resource.file_path.to_string_lossy()
    );

    // This may not be in the commit if it's added, so have to parse tabular-ness from the path.
    // TODO: can we find the file / check if it's in the staging area?

    if util::fs::is_tabular(&resource.file_path) {
        mod_stager::restore_df(&repo, &branch, &user_id, &resource.file_path)?;
        Ok(HttpResponse::Ok().json(StatusMessage::resource_deleted()))
    } else {
        log::debug!("not tabular");
        Ok(delete_staged_file_on_branch(
            &repo,
            &branch.name,
            &user_id,
            &resource.file_path,
        ))
    }

    // TODO convert this to result

    // match api::local::repositories::get_by_namespace_and_name(&app_data.path, namespace, repo_name)
    // {
    //     Ok(Some(repo)) => match api::local::resource::parse_resource(&repo, &resource) {
    //         Ok(Some((_, branch_name, file_name))) => {
    //             delete_staged_file_on_branch(&repo, &branch_name, user_id, &file_name)
    //         }
    //         Ok(None) => {
    //             log::error!("unable to find resource {:?}", resource);
    //             HttpResponse::NotFound().json(StatusMessage::resource_not_found())
    //         }
    //         Err(err) => {
    //             log::error!("Could not parse resource  {repo_name} -> {err}");
    //             HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
    //         }
    //     },
    //     Ok(None) => {
    //         log::error!("unable to find repo {}", repo_name);
    //         HttpResponse::NotFound().json(StatusMessage::resource_not_found())
    //     }
    //     Err(err) => {
    //         log::error!("Error getting repo by name {repo_name} -> {err}");
    //         HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
    //     }
    // }
}

pub async fn unindex_dataset(req: HttpRequest) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let resource = parse_resource(&req, &repo)?;

    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    index::remote_df_stager::unindex_df(&repo, &branch, &identifier, &resource.file_path)?;

    Ok(HttpResponse::Ok().json(StatusMessage::resource_deleted()))
}

pub async fn index_dataset(req: HttpRequest) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req)?;

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, &namespace, &repo_name)?;
    let resource = parse_resource(&req, &repo)?;

    log::debug!(
        "{} indexing dataset for resource {namespace}/{repo_name}/{resource}",
        liboxen::current_function!()
    );

    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    // Initialize the branch repository before any operations
    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    match liboxen::core::index::remote_df_stager::index_dataset(
        &repo,
        &branch,
        &resource.file_path,
        &identifier,
    ) {
        Ok(_) => {
            log::info!(
                "Dataset indexing completed successfully for {namespace}/{repo_name}/{resource}"
            );
            let response = StatusMessage::resource_created();
            Ok(HttpResponse::Ok().json(response))
        }
        Err(err) => {
            log::error!("Failed to index dataset for {namespace}/{repo_name}/{resource}: {err}");
            Err(OxenHttpError::InternalServerError)
        }
    }
}

pub async fn get_staged_df(
    req: HttpRequest,
    query: web::Query<DFOptsQuery>,
) -> Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req).unwrap();

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, &namespace, &repo_name)?;
    let resource = parse_resource(&req, &repo)?;
    let commit = resource.commit.clone();

    let entry = api::local::entries::get_commit_entry(&repo, &commit, &resource.file_path)?
        .ok_or(OxenError::entry_does_not_exist(resource.file_path.clone()))?;

    let schema = api::local::schemas::get_by_path(&repo, &resource.file_path)?
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    log::debug!("got this schema for the endpoint {:?}", schema);

    log::debug!(
        "{} indexing dataset for resource {namespace}/{repo_name}/{resource}",
        liboxen::current_function!()
    );

    // Staged dataframes must be on a branch.
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    let mut opts = DFOpts::empty();
    opts = df_opts_query::parse_opts(&query, &mut opts);

    opts.page = Some(query.page.unwrap_or(constants::DEFAULT_PAGE_NUM));
    opts.page_size = Some(query.page_size.unwrap_or(constants::DEFAULT_PAGE_SIZE));

    if index::remote_df_stager::dataset_is_indexed(
        &repo,
        &branch,
        &identifier,
        &resource.file_path,
    )? {
        let count = index::remote_df_stager::count(
            &repo,
            &branch,
            resource.file_path.clone(),
            &identifier,
        )?;

        let df =
            index::remote_df_stager::query_staged_df(&repo, &entry, &branch, &identifier, &opts)?;

        let df_schema = Schema::from_polars(&df.schema());

        let df_views =
            JsonDataFrameViews::from_df_and_opts_unpaginated(df, df_schema, count, &opts);
        let resource = ResourceVersion {
            path: resource.file_path.to_string_lossy().to_string(),
            version: resource.version(),
        };

        let response = JsonDataFrameViewResponse {
            status: StatusMessage::resource_found(),
            data_frame: df_views,
            resource: Some(resource),
            commit: None, // Not at a committed state
            derived_resource: None,
        };

        Ok(HttpResponse::Ok().json(response))
    } else {
        Err(OxenHttpError::NotFound)
    }
}

pub async fn get_df_is_editable(
    req: HttpRequest,
) -> actix_web::Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req).unwrap();

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, &namespace, &repo_name)?;
    let resource = parse_resource(&req, &repo)?;

    log::debug!(
        "{} indexing dataset for resource {namespace}/{repo_name}/{resource}",
        liboxen::current_function!()
    );

    // Staged dataframes must be on a branch.
    let branch = resource
        .branch
        .clone()
        .ok_or(OxenError::parsed_resource_not_found(resource.to_owned()))?;

    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    let is_editable = index::remote_df_stager::dataset_is_indexed(
        &repo,
        &branch,
        &identifier,
        &resource.file_path,
    )?;

    Ok(HttpResponse::Ok().json(DFIsEditableResponse {
        status: StatusMessage::resource_found(),
        is_editable,
    }))
}

pub async fn list_editable_dfs(
    req: HttpRequest,
    query: web::Query<PageNumQuery>,
) -> actix_web::Result<HttpResponse, OxenHttpError> {
    let app_data = app_data(&req).unwrap();

    let namespace = path_param(&req, "namespace")?;
    let repo_name = path_param(&req, "repo_name")?;
    let identifier = path_param(&req, "identifier")?;
    let repo = get_repo(&app_data.path, namespace, repo_name)?;
    let branch_name: &str = req.match_info().query("branch");

    let page = query.page.unwrap_or(constants::DEFAULT_PAGE_NUM);
    let page_size = query.page_size.unwrap_or(constants::DEFAULT_PAGE_SIZE);

    // Staged dataframes must be on a branch.
    let branch = api::local::branches::get_by_name(&repo, branch_name)?
        .ok_or(OxenError::remote_branch_not_found(branch_name))?;

    let commit = api::local::commits::get_by_id(&repo, &branch.commit_id)?
        .ok_or(OxenError::resource_not_found(&branch.commit_id))?;

    let _branch_repo = index::remote_dir_stager::init_or_get(&repo, &branch, &identifier)?;

    let entries = api::local::entries::list_tabular_files_in_repo(&repo, &commit)?;

    let mut editable_entries = vec![];

    for entry in entries {
        if let Some(resource) = entry.resource.clone() {
            if index::remote_df_stager::dataset_is_indexed(
                &repo,
                &branch,
                &identifier,
                &PathBuf::from(resource.path),
            )? {
                editable_entries.push(entry);
            }
        }
    }

    let (paginated_entries, pagination) = paginate(editable_entries, page, page_size);
    Ok(HttpResponse::Ok().json(PaginatedMetadataEntriesResponse {
        status: StatusMessage::resource_found(),
        entries: PaginatedMetadataEntries {
            entries: paginated_entries,
            pagination,
        },
    }))
}

fn clear_staged_modifications_on_branch(
    repo: &LocalRepository,
    branch_name: &str,
    user_id: &str,
    path: &Path,
) -> HttpResponse {
    match api::local::branches::get_by_name(repo, branch_name) {
        Ok(Some(branch)) => {
            index::remote_dir_stager::init_or_get(repo, &branch, user_id).unwrap();
            match mod_stager::restore_df(repo, &branch, user_id, path) {
                Ok(_) => {
                    log::debug!("clear_staged_modifications_on_branch success!");
                    HttpResponse::Ok().json(StatusMessage::resource_deleted())
                }
                Err(err) => {
                    log::error!("unable to delete file {:?}. Err: {}", path, err);
                    HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
                }
            }
        }
        Ok(None) => {
            log::error!("unable to find branch {}", branch_name);
            HttpResponse::NotFound().json(StatusMessage::resource_not_found())
        }
        Err(err) => {
            log::error!("Error getting branch by name {branch_name} -> {err}");
            HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
        }
    }
}

fn delete_staged_file_on_branch(
    repo: &LocalRepository,
    branch_name: &str,
    user_id: &str,
    path: &Path,
) -> HttpResponse {
    log::debug!("delete_staged_file_on_branch()");
    match api::local::branches::get_by_name(repo, branch_name) {
        Ok(Some(branch)) => {
            let branch_repo =
                index::remote_dir_stager::init_or_get(repo, &branch, user_id).unwrap();
            log::debug!("got branch_repo");
            match index::remote_dir_stager::has_file(&branch_repo, path) {
                Ok(true) => match index::remote_dir_stager::delete_file(&branch_repo, path) {
                    Ok(_) => {
                        log::debug!("stager::delete_file success!");
                        HttpResponse::Ok().json(StatusMessage::resource_deleted())
                    }
                    Err(err) => {
                        log::error!("unable to delete file {:?}. Err: {}", path, err);
                        HttpResponse::InternalServerError()
                            .json(StatusMessage::internal_server_error())
                    }
                },
                Ok(false) => {
                    log::error!("unable to find file {:?}", path);
                    HttpResponse::NotFound().json(StatusMessage::resource_not_found())
                }
                Err(err) => {
                    log::error!("Error getting file by path {path:?} -> {err}");
                    HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
                }
            }
        }
        Ok(None) => {
            log::error!("unable to find branch {}", branch_name);
            HttpResponse::NotFound().json(StatusMessage::resource_not_found())
        }
        Err(err) => {
            log::error!("Error getting branch by name {branch_name} -> {err}");
            HttpResponse::InternalServerError().json(StatusMessage::internal_server_error())
        }
    }
}

fn get_dir_status_for_branch(
    repo: &LocalRepository,
    branch_name: &str,
    user_id: &str,
    directory: &Path,
    page_num: usize,
    page_size: usize,
) -> actix_web::Result<HttpResponse, OxenHttpError> {
    let branch = api::local::branches::get_by_name(repo, branch_name)?
        .ok_or(OxenError::remote_branch_not_found(branch_name))?;

    let branch_repo = index::remote_dir_stager::init_or_get(repo, &branch, user_id)?;

    log::debug!(
        "GOT BRANCH REPO {:?} and DIR {:?}",
        branch_repo.path,
        directory
    );
    let staged = index::remote_dir_stager::list_staged_data(
        repo,
        &branch_repo,
        &branch,
        user_id,
        directory,
    )?;

    staged.print_stdout();
    let full_path = index::remote_dir_stager::branch_staging_dir(repo, &branch, user_id);
    let branch_repo = LocalRepository::new(&full_path).unwrap();

    let response = RemoteStagedStatusResponse {
        status: StatusMessage::resource_found(),
        staged: RemoteStagedStatus::from_staged(&branch_repo, &staged, page_num, page_size),
    };
    Ok(HttpResponse::Ok().json(response))
}
