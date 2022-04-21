
use crate::app_data::SyncDir;

use liboxen::api;
use liboxen::api::local::RepositoryAPI;
use liboxen::http;
use liboxen::http::response::EntryResponse;
use liboxen::http::{MSG_RESOURCE_CREATED, STATUS_SUCCESS};
use liboxen::model::{Entry, Repository};
use serde::Deserialize;

use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::stream::StreamExt as _;

use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

#[derive(Deserialize, Debug)]
pub struct EntryQuery {
    filename: String,
    hash: String,
}

pub async fn create(
    req: HttpRequest,
    body: web::Payload,
    data: web::Query<EntryQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let sync_dir = req.app_data::<SyncDir>().unwrap();
    let api = RepositoryAPI::new(&sync_dir.path);

    println!("GOT REQ: {:?}\n\n\nquery: {}", req, req.query_string());

    // path to the repo
    let path: &str = req.match_info().get("name").unwrap();
    match api.get_by_path(Path::new(&path)) {
        Ok(result) => {
            create_entry(&sync_dir.path, result.repository, body, data).await
        }
        Err(err) => {
            let msg = format!("Err: {}", err);
            Ok(HttpResponse::BadRequest().json(http::StatusMessage::error(&msg)))
        }
    }
}

async fn create_entry(
    sync_dir: &Path,
    repository: Repository,
    mut body: web::Payload,
    data: web::Query<EntryQuery>,
) -> Result<HttpResponse, actix_web::Error> {
    let repo_dir = &sync_dir.join(&repository.name);

    let filepath = repo_dir.join(&data.filename);

    if let Some(parent) = filepath.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut file = File::create(&filepath)?;
    let mut total_bytes = 0;
    while let Some(item) = body.next().await {
        total_bytes += file.write(&item?)?;
    }
    if let Some(extension) = filepath.extension() {
        println!(
            "Wrote {} bytes to {:?} with extension",
            total_bytes, filepath,
        );
        let url = format!("{}/{}", api::endpoint::url_from(&repository.name), &data.filename);

        Ok(HttpResponse::Ok().json(EntryResponse {
            status: String::from(STATUS_SUCCESS),
            status_message: String::from(MSG_RESOURCE_CREATED),
            entry: Entry {
                id: format!("{}", uuid::Uuid::new_v4()), // generate a new one on the server for now
                data_type: data_type_from_ext(extension.to_str().unwrap()),
                url,
                filename: String::from(&data.filename),
                hash: String::from(&data.hash),
            },
        }))
    } else {
        let msg = format!("Invalid file extension: {:?}", &data.filename);
        Ok(HttpResponse::BadRequest().json(http::StatusMessage::error(&msg)))
    }
}

fn data_type_from_ext(ext: &str) -> String {
    match ext {
        "jpg" | "png" => String::from("image"),
        "txt" => String::from("text"),
        _ => String::from("binary"),
    }
}

#[cfg(test)]
mod tests {

    use actix_web::http::{self};
    use actix_web::{App, web};
    use actix_web::http::{header, StatusCode};

    use actix_web::body::to_bytes;

    use liboxen::error::OxenError;

    use liboxen::http::response::{ListRepositoriesResponse, RepositoryResponse};
    use liboxen::http::STATUS_SUCCESS;

    use crate::controllers;
    use crate::test;
    use crate::app_data::SyncDir;

    #[actix_web::test]
    async fn test_entries_create() -> Result<(), OxenError> {
        let sync_dir = test::get_sync_dir();

        let mut app = actix_web::test::init_service(
            App::new()
                .app_data(SyncDir { path: sync_dir.clone() })
                .route("/repositories/{name}/entries", web::post().to(controllers::entries::create))
        ).await;
        let req = actix_web::test::TestRequest::post()
            .uri("/repositories/test/entries?filename=test.txt&hash=1234")
            .to_request();
        let mut resp = actix_web::test::call_service(&mut app, req).await;
        let body = resp.into_body();
        println!("GOT BODY {:?}", body);
        // assert!(resp.status().is_success());
        assert!(false);

        // cleanup
        std::fs::remove_dir_all(sync_dir)?;

        Ok(())
    }
}