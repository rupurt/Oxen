//! versioner is responsible for interacting with entries in the versioned directory
//!

use filetime::FileTime;
use std::io::Write;
use std::path::Path;

use crate::current_function;
use crate::error::OxenError;
use crate::model::{CommitEntry, LocalRepository, Schema};
use crate::util;

use super::CommitDirEntryWriter;

pub fn backup_file(
    repository: &LocalRepository,
    committer: &CommitDirEntryWriter,
    entry: &CommitEntry,
    filepath: impl AsRef<Path>,
) -> Result<(), OxenError> {
    let version_path = util::fs::version_path(repository, entry);
    let filepath = filepath.as_ref();
    if should_copy_entry(entry, &version_path) {
        log::debug!("{} unpack {:?}", current_function!(), entry.path);
        match util::fs::copy_mkdir(filepath, &version_path) {
            Ok(_) => {}
            Err(err) => {
                log::error!(
                    "Could not copy {:?} to {:?}: {}",
                    version_path,
                    filepath,
                    err
                );
            }
        }

        log::debug!(
            "{} updating timestamp for {:?}",
            current_function!(),
            filepath
        );

        match util::fs::metadata(filepath) {
            Ok(metadata) => {
                let mtime = FileTime::from_last_modification_time(&metadata);
                committer.set_file_timestamps(entry, &mtime)?;
            }
            Err(err) => {
                log::error!("Could not update timestamp for {:?}: {}", filepath, err);
            }
        }
    }
    Ok(())
}

pub fn backup_schema(repository: &LocalRepository, schema: &Schema) -> Result<(), OxenError> {
    log::debug!("backing up schema {:?}", schema);
    let version_path = util::fs::version_path_from_schema(repository.path.clone(), schema);
    // Create all parent dirs that don't exist
    if let Some(parent) = version_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    log::debug!("got version path for schema {:?}", version_path);
    if should_copy_schema(schema, &version_path) {
        // Write the schema out to the version path name
        let mut file = std::fs::File::create(&version_path)?;
        // Write to path with serde
        let schema_json = serde_json::to_string(schema)?;

        file.write_all(schema_json.as_bytes())?;
    }
    Ok(())
}

pub fn should_copy_entry(entry: &CommitEntry, path: &Path) -> bool {
    !path.exists() || path_hash_is_different(entry, path)
}

pub fn should_copy_schema(_schema: &Schema, path: &Path) -> bool {
    !path.exists()
}

fn path_hash_is_different(entry: &CommitEntry, path: &Path) -> bool {
    if let Ok(hash) = util::hasher::hash_file_contents(path) {
        return hash != entry.hash;
    }
    false
}
