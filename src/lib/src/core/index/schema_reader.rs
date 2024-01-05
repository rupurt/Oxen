use crate::constants::{FILES_DIR, HISTORY_DIR, SCHEMAS_DIR, SCHEMAS_TREE_PREFIX};
use crate::core::db::tree_db::{TreeObject, TreeObjectChild};
use crate::core::db::{self, path_db};
use crate::core::db::{str_json_db, str_val_db};
use crate::core::index::CommitEntryWriter;
use crate::error::OxenError;
use crate::model::Schema;
use crate::util;

use rocksdb::{DBWithThreadMode, MultiThreaded};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::Arc;

use crate::model::LocalRepository;

use super::ObjectDBReader;

pub struct SchemaReader {
    schema_db: DBWithThreadMode<MultiThreaded>,
    schema_files_db: DBWithThreadMode<MultiThreaded>,
    object_reader: Arc<ObjectDBReader>,
    dir_hashes_db: DBWithThreadMode<MultiThreaded>,
    repository: LocalRepository,
    commit_id: String,
}

impl SchemaReader {
    pub fn schemas_db_dir(repo: &LocalRepository, commit_id: &str) -> PathBuf {
        // .oxen/history/COMMIT_ID/schemas/schemas
        util::fs::oxen_hidden_dir(&repo.path)
            .join(HISTORY_DIR)
            .join(commit_id)
            .join(SCHEMAS_DIR) // double schemas/schemas is intentional because we have multiple dirs at this level
            .join(SCHEMAS_DIR)
    }

    pub fn schema_files_db_dir(repo: &LocalRepository, commit_id: &str) -> PathBuf {
        // .oxen/history/COMMIT_ID/schemas/files
        util::fs::oxen_hidden_dir(&repo.path)
            .join(HISTORY_DIR)
            .join(commit_id)
            .join(SCHEMAS_DIR)
            .join(FILES_DIR)
    }

    pub fn new(repository: &LocalRepository, commit_id: &str) -> Result<SchemaReader, OxenError> {
        let schema_db_path = SchemaReader::schemas_db_dir(repository, commit_id);
        log::debug!("SchemaReader db {:?}", schema_db_path);
        let schema_files_db_path = SchemaReader::schema_files_db_dir(repository, commit_id);

        let dir_hashes_db_path = CommitEntryWriter::commit_dir_hash_db(&repository.path, commit_id);

        log::debug!("SchemaReader files db {:?}", schema_files_db_path);
        let opts = db::opts::default();
        if !schema_db_path.exists() {
            std::fs::create_dir_all(&schema_db_path)?;
            // open it then lose scope to close it
            let _db: DBWithThreadMode<MultiThreaded> =
                DBWithThreadMode::open(&opts, dunce::simplified(&schema_db_path))?;
        }

        if !schema_files_db_path.exists() {
            std::fs::create_dir_all(&schema_files_db_path)?;
            // open it then lose scope to close it
            let _db: DBWithThreadMode<MultiThreaded> =
                DBWithThreadMode::open(&opts, dunce::simplified(&schema_files_db_path))?;
        }

        if !dir_hashes_db_path.exists() {
            std::fs::create_dir_all(&dir_hashes_db_path)?;
            // open it then lose scope to close it
            let _db: DBWithThreadMode<MultiThreaded> =
                DBWithThreadMode::open(&opts, dunce::simplified(&dir_hashes_db_path))?;
        }

        let object_reader = ObjectDBReader::new(&repository)?;

        Ok(SchemaReader {
            schema_db: DBWithThreadMode::open_for_read_only(&opts, &schema_db_path, false)?,
            schema_files_db: DBWithThreadMode::open_for_read_only(
                &opts,
                &schema_files_db_path,
                false,
            )?,
            dir_hashes_db: DBWithThreadMode::open_for_read_only(&opts, &dir_hashes_db_path, false)?,
            object_reader,
            repository: repository.clone(),
            commit_id: commit_id.to_owned(),
        })
    }

    /// See if a commit id exists - this is unused
    // pub fn schema_hash_exists(&self, hash: &str) -> bool {
    //     str_json_db::has_key(&self.schema_db, hash)
    // }

    // Schema hash exists in this commit...

    /// Get a commit object from an ID
    /// only used in get_schema_for_file
    // pub fn get_schema_by_hash<S: AsRef<str>>(&self, hash: S) -> Result<Option<Schema>, OxenError> {
    //     str_json_db::get(&self.schema_db, hash)
    // }

    pub fn get_schema_hash_for_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<String>, OxenError> {
        str_val_db::get(&self.schema_files_db, path.as_ref().to_str().unwrap())
    }

    pub fn get_schema_for_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Option<Schema>, OxenError> {
        log::debug!("in get_schema_for_file path {:?}", path.as_ref());
        let schema_path = Path::new(SCHEMAS_TREE_PREFIX).join(&path);
        let path_parent = path.as_ref().parent().unwrap_or(Path::new(""));

        // Get the parent dir hash in which the schema is stored
        log::debug!("getting parent dir hash for path {:?}", path_parent);
        let parent_dir_hash: String =
            path_db::get_entry(&self.dir_hashes_db, path_parent.to_str().unwrap())?.unwrap();

        let parent_dir_obj: TreeObject = self.object_reader.get_dir(&parent_dir_hash)?.unwrap();

        // Get the hash of the schema's path
        let schema_path_hash_prefix = util::hasher::hash_pathbuf(&schema_path)[0..2].to_string();

        // Binary search for the appropriate vnode
        let vnode_child: Option<TreeObjectChild> = parent_dir_obj
            .binary_search_on_path(&PathBuf::from(schema_path_hash_prefix.clone()))?;

        if vnode_child.is_none() {
            return Ok(None);
        }

        let vnode_child = vnode_child.unwrap();
        let vnode = self.object_reader.get_vnode(&vnode_child.hash())?.unwrap();

        log::debug!("got vnode");
        // Binary search for the appropriate schema
        let schema_child: Option<TreeObjectChild> =
            vnode.binary_search_on_path(&PathBuf::from(schema_path_hash_prefix.clone()))?;

        if schema_child.is_none() {
            return Ok(None);
        }

        let schema_child = schema_child.unwrap();

        // Get the schema from the versions directory by hash
        let version_path = util::fs::version_path_from_schema_hash(
            &self.repository.path,
            schema_child.hash().to_string(),
        );

        let schema: Result<Schema, serde_json::Error> =
            serde_json::from_reader(std::fs::File::open(version_path)?);

        match schema {
            Ok(schema) => Ok(Some(schema)),
            Err(_) => Ok(None),
        }
    }

    pub fn list_schemas(&self) -> Result<HashMap<PathBuf, Schema>, OxenError> {
        log::debug!("calling list schemas");
        let root_hash: String = path_db::get_entry(&self.dir_hashes_db, "")?.unwrap();
        let root_node: TreeObject = self.object_reader.get_dir(&root_hash)?.unwrap();

        let mut path_vals: HashMap<PathBuf, Schema> = HashMap::new();

        self.r_list_schemas(root_node, &mut path_vals)?;

        Ok(path_vals)
    }

    fn r_list_schemas(
        &self,
        dir_node: TreeObject,
        path_vals: &mut HashMap<PathBuf, Schema>,
    ) -> Result<(), OxenError> {
        for vnode in dir_node.children() {
            let vnode = self.object_reader.get_vnode(&vnode.hash())?.unwrap();
            for child in vnode.children() {
                match child {
                    TreeObjectChild::Dir { hash, .. } => {
                        let dir_node = self.object_reader.get_dir(&hash)?.unwrap();
                        self.r_list_schemas(dir_node, path_vals)?;
                    }
                    TreeObjectChild::Schema { path, hash, .. } => {
                        let stripped_path = path.strip_prefix(SCHEMAS_TREE_PREFIX).unwrap();
                        let found_schema = self.get_schema_by_hash(&hash)?;
                        path_vals.insert(stripped_path.to_path_buf(), found_schema);
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn list_schemas_for_ref(
        &self,
        schema_ref: impl AsRef<str>,
    ) -> Result<HashMap<PathBuf, Schema>, OxenError> {
        let schema_ref = schema_ref.as_ref();
        // This is a map of paths to schema hashes
        let paths_to_hashes: HashMap<String, String> = str_val_db::hash_map(&self.schema_files_db)?;

        // This is a map of hashes to schemas
        let hash_to_schemas: HashMap<String, Schema> = str_json_db::hash_map(&self.schema_db)?;

        // For each path, get the schema
        let path_vals: HashMap<PathBuf, Schema> = paths_to_hashes
            .iter()
            .map(|(k, v)| (PathBuf::from(k), hash_to_schemas.get(v).unwrap().clone()))
            .filter(|(k, v)| {
                k.to_string_lossy() == schema_ref
                    || v.hash == schema_ref
                    || v.name == Some(schema_ref.to_string())
            })
            .collect();

        Ok(path_vals)
    }

    fn get_schema_by_hash(&self, hash: &str) -> Result<Schema, OxenError> {
        let version_path =
            util::fs::version_path_from_schema_hash(&self.repository.path, hash.to_string());
        let schema = serde_json::from_reader(std::fs::File::open(version_path)?)?;
        Ok(schema)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::api;
    use crate::core::index::SchemaReader;
    use crate::error::OxenError;
    use crate::test;

    #[test]
    fn test_schema_reader_list_empty_schemas() -> Result<(), OxenError> {
        test::run_training_data_repo_test_no_commits(|repo| {
            let history = api::local::commits::list(&repo)?;
            let last_commit = history.first().unwrap();
            let schema_reader = SchemaReader::new(&repo, &last_commit.id)?;
            let schemas = schema_reader.list_schemas()?;

            assert_eq!(schemas.len(), 0);

            Ok(())
        })
    }

    #[test]
    fn test_schema_reader_list_committed_schemas() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed(|repo| {
            let history = api::local::commits::list(&repo)?;
            let last_commit = history.first().unwrap();
            let schema_reader = SchemaReader::new(&repo, &last_commit.id)?;
            let schemas = schema_reader.list_schemas()?;

            for (k, v) in schemas.iter() {
                println!("{}: {}", k.to_string_lossy(), v.hash);
            }

            assert_eq!(schemas.len(), 7);
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/bounding_box.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/one_shot.csv")));
            assert!(
                schemas.contains_key(&PathBuf::from("nlp/classification/annotations/train.tsv"))
            );
            assert!(schemas.contains_key(&PathBuf::from("large_files/test.csv")));
            assert!(schemas.contains_key(&PathBuf::from("nlp/classification/annotations/test.tsv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/two_shot.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/test/annotations.csv")));

            Ok(())
        })
    }

    #[test]
    fn test_schema_reader_get_schema_ref_file() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed(|repo| {
            let history = api::local::commits::list(&repo)?;
            let last_commit = history.first().unwrap();
            let schema_reader = SchemaReader::new(&repo, &last_commit.id)?;

            let schema_ref = "annotations/train/bounding_box.csv";
            let schemas = schema_reader.list_schemas_for_ref(schema_ref)?;

            assert_eq!(schemas.len(), 1);
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/bounding_box.csv")));

            Ok(())
        })
    }

    #[test]
    fn test_schema_reader_get_schema_ref_by_name() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed(|repo| {
            let history = api::local::commits::list(&repo)?;
            let last_commit = history.first().unwrap();
            let schema_reader = SchemaReader::new(&repo, &last_commit.id)?;

            let schema_ref = "bounding_box";
            let schemas = schema_reader.list_schemas_for_ref(schema_ref)?;

            assert_eq!(schemas.len(), 4);
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/bounding_box.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/one_shot.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/two_shot.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/test/annotations.csv")));

            Ok(())
        })
    }

    #[test]
    fn test_schema_reader_get_schema_ref_by_hash() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed(|repo| {
            let history = api::local::commits::list(&repo)?;
            let last_commit = history.first().unwrap();
            let schema_reader = SchemaReader::new(&repo, &last_commit.id)?;

            let schema_ref = "b821946753334c083124fd563377d795";
            let schemas = schema_reader.list_schemas_for_ref(schema_ref)?;

            for (k, v) in schemas.iter() {
                println!("{}: {}", k.to_string_lossy(), v);
            }

            assert_eq!(schemas.len(), 4);
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/bounding_box.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/one_shot.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/train/two_shot.csv")));
            assert!(schemas.contains_key(&PathBuf::from("annotations/test/annotations.csv")));

            Ok(())
        })
    }
}
