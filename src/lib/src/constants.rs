//! Constants used throughout the codebase
//!

/// Rust library version
pub const OXEN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// # Filenames and dirs
/// .oxen is the name of the hidden directory where all our data lives
pub const OXEN_HIDDEN_DIR: &str = ".oxen";
/// Folder name for oxen home within `.cache`, `.config`., etc.
pub const OXEN: &str = "oxen";
/// ~/.cache/oxen holds tmp downloads
pub const TMP_DIR: &str = ".cache";
/// ~/.config/oxen holds config files
pub const CONFIG_DIR: &str = ".config";
/// .oxenignore is the name of the file that contains the ignore patterns
pub const OXEN_IGNORE_FILE: &str = ".oxenignore";
/// Config file for the repository
pub const REPO_CONFIG_FILENAME: &str = "config.toml";
/// HEAD file holds onto where the head commit is (commit_id or branch name)
pub const HEAD_FILE: &str = "HEAD";
/// refs/ is a key,val store of branch names to commit ids
pub const REFS_DIR: &str = "refs";
/// history/ dir is a list of directories named after commit ids
pub const HISTORY_DIR: &str = "history";
/// commits/ is a key-value database of commit ids to commit objects
pub const COMMITS_DIR: &str = "commits";
/// name of the schema db
pub const SCHEMAS_DIR: &str = "schemas";
/// schemas node in merkle tree
pub const SCHEMAS_TREE_PREFIX: &str = ".oxen";
// name of dir for locking branches during push
pub const BRANCH_LOCKS_DIR: &str = "locks";
// name of file for locking repository during push
pub const REPOSITORY_LOCK_FILE: &str = "LOCK";
/// prefix for the commit rows
pub const ROWS_DIR: &str = "rows";
/// prefix for the commit entry files
pub const FILES_DIR: &str = "files";
/// prefix for the cached dataframes
pub const DATA_FRAMES_DIR: &str = "data_frames";
/// prefix for the commit entry dirs
pub const DIRS_DIR: &str = "dirs";
/// prefix for a commit dir => hash maping
pub const DIR_HASHES_DIR: &str = "dir_hashes";
/// prefix for the commit merkle tree db
pub const TREE_DIR: &str = "tree";
/// prefix for the cached stats dirs
pub const CACHE_DIR: &str = "cache";
/// prefix for cached compare dfs
pub const COMPARES_DIR: &str = "compares";
/// prefix for the left commit pointer in cached compares
pub const LEFT_COMPARE_COMMIT: &str = "LEFT";
/// prefix for the right commit pointer in cached compares
pub const RIGHT_COMPARE_COMMIT: &str = "RIGHT";
/// prefix for the stats dir
pub const STATS_DIR: &str = "stats";
/// prefix for the staged dirs
pub const STAGED_DIR: &str = "staged";
/// Name of the table in the duckdb db used for remote staging
pub const TABLE_NAME: &str = "df";
/// Oxen's internal row id column in duckdb remote staging tables
pub const OXEN_COLS: [&str; 4] = [OXEN_ID_COL, DIFF_STATUS_COL, OXEN_ROW_ID_COL, DIFF_HASH_COL];
pub const OXEN_ROW_ID_COL: &str = "_oxen_row_id";
/// Oxen's internal id column in duckdb remote staging tables
pub const OXEN_ID_COL: &str = "_oxen_id";
/// Name of the folder of the cache dir in which dfs are indexed as duckdbs
pub const DUCKDB_CACHE_DIR: &str = "duckdb";
/// Default name for duckdb table used for indexing dataframes
pub const DUCKDB_DF_TABLE_NAME: &str = "df";
/// Max number of rows to query from a dataframe
pub const MAX_QUERYABLE_ROWS: usize = 1_000_000;
/// prefix for the sync status dirs to tell if commits are synced locally
pub const SYNC_STATUS_DIR: &str = "sync_status";
/// Flag for if the repository was cloned in a shallow fashion
pub const SHALLOW_FLAG: &str = "SHALLOW";
/// prefix for the commit indices
pub const INDICES_DIR: &str = "indices";
/// prefix for the schema fields that are indexed
pub const FIELDS_DIR: &str = "fields";
/// versions/ is where all the versions are stored so that we can use to quickly swap between versions of the file
pub const VERSIONS_DIR: &str = "versions";
/// objects/ stores pointers to data files and sub-tree structures for efficient commit representations
pub const OBJECTS_DIR: &str = "objects";
/// Storage of file node representations in objects dir
pub const OBJECT_FILES_DIR: &str = "files";
/// Storage of dir node representations in objects dir
pub const OBJECT_DIRS_DIR: &str = "dirs";
/// Storage of hash-bucketed vnode representations in objects dir
pub const OBJECT_VNODES_DIR: &str = "vnodes";
/// Storage of schema node representations in objects dir
pub const OBJECT_SCHEMAS_DIR: &str = "schemas";
/// File name for files stored in versions directory (>0.8.4). (Was commit id <= 0.8.4)
pub const VERSION_FILE_NAME: &str = "data";
/// merge/ is where any merge conflicts are stored so that we can get rid of them
pub const MERGE_DIR: &str = "merge";
/// mods/ is where we can stage appends, modifications, deletions to files to be merged later
pub const MODS_DIR: &str = "mods";
/// data.arrow
pub const DATA_ARROW_FILE: &str = "data.arrow";

/// if we have merge conflicts we write to MERGE_HEAD and ORIG_HEAD to keep track of the parents
pub const MERGE_HEAD_FILE: &str = "MERGE_HEAD";
/// if we have merge conflicts we write to MERGE_HEAD and ORIG_HEAD to keep track of the parents
pub const ORIG_HEAD_FILE: &str = "ORIG_HEAD";

/// Key for hash of the file
pub const HASH_FILE: &str = "HASH";
/// Key for content being valid
pub const CONTENT_IS_VALID: &str = "CONTENT_IS_VALID";
/// Key for if something is synced
pub const IS_SYNCED: &str = "IS_SYNCED";

/// Default branch name: main
pub const DEFAULT_BRANCH_NAME: &str = "main";
/// Default remote name: origin
pub const DEFAULT_REMOTE_NAME: &str = "origin";
/// Default remote host: hub.oxen.ai
pub const DEFAULT_HOST: &str = "hub.oxen.ai";

/// Default Namespace: ox
pub const DEFAULT_NAMESPACE: &str = "ox";

/// Initial Commit Message
pub const INITIAL_COMMIT_MSG: &str = "Initialized Repo 🐂";

/// Internal Name When Performing Computation
pub const ROW_NUM_COL_NAME: &str = "_row_num";
/// Internal Name When Performing Computation
pub const ROW_HASH_COL_NAME: &str = "_row_hash";
/// Internal Name When Performing Computation
pub const FILE_ROW_NUM_COL_NAME: &str = "_file_row_num";
// Internal Name When Performing Computation
pub const TARGETS_HASH_COL: &str = "_targets_hash";
// Internal Name When Performing Computation
pub const KEYS_HASH_COL: &str = "_keys_hash";
// Internal Name When Performing Computation
pub const DIFF_STATUS_COL: &str = ".oxen.diff.status";
// Internal Name When Performing Computation
pub const DIFF_HASH_COL: &str = ".oxen.diff.hash";

// Data transfer
// Average chunk size of ~4mb
/// Average chunk size of ~4mb when chunking and sending data
// pub const AVG_CHUNK_SIZE: u64 = 1024 * 1024 * 4;
pub const AVG_CHUNK_SIZE: u64 = 1024 * 1024 * 4;
// Retry and back off of requests N times
/// Retry and back off of requests N times
pub const NUM_HTTP_RETRIES: u64 = 10;
/// Number of workers
pub const DEFAULT_NUM_WORKERS: usize = 8;

/// Pagination page size of 10
pub const DEFAULT_PAGE_SIZE: usize = 100;
/// Pagination page number of 1
pub const DEFAULT_PAGE_NUM: usize = 1;

/// Redis queue name for post commit actions
pub const COMMIT_QUEUE_NAME: &str = "commit_queue";
pub const DEFAULT_REDIS_URL: &str = "redis://localhost:6379";

/// Data Types
pub const TEXT: &str = "text";
pub const IMAGE: &str = "image";
pub const VIDEO: &str = "video";
pub const AUDIO: &str = "audio";
pub const TABULAR: &str = "tabular";
pub const BINARY: &str = "binary";
pub const DIR: &str = "dir";

/// Minimum allowable oxen version to push or pull data
pub const MIN_CLI_VERSION: &str = "0.10.0";

/// Filepath used to track repo and server-level migration status
pub const LAST_MIGRATION_FILE: &str = "last_migration.txt";

/// Constraints for diff and compare size
pub const MAX_DISPLAY_DIRS: usize = 10;
