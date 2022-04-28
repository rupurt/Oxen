//! # Oxen Commands
//!
//! Top level commands you are likely to run on an Oxen repository
//!

use crate::error::OxenError;
use crate::index::{Committer, Referencer, Stager};
use crate::model::{Branch, Commit, LocalRepository, StagedData};
use crate::util;

use std::path::Path;

pub const NO_REPO_MSG: &str = "fatal: no oxen repository exists, looking for directory: .oxen ";

/// # Initialize an Empty Oxen Repository
///
/// ```
/// # use liboxen::command;
/// # use liboxen::error::OxenError;
/// # use std::path::Path;
/// # fn main() -> Result<(), OxenError> {
///
/// let base_dir = Path::new("/tmp/repo_dir");
/// command::init(base_dir)?;
/// assert!(base_dir.join(".oxen").exists());
///
/// # std::fs::remove_dir_all(base_dir)?;
/// # Ok(())
/// # }
/// ```
pub fn init(path: &Path) -> Result<LocalRepository, OxenError> {
    let hidden_dir = util::fs::oxen_hidden_dir(path);
    std::fs::create_dir_all(hidden_dir)?;
    let config_path = util::fs::config_filepath(path);
    let repo = LocalRepository::new(path)?;
    repo.save(&config_path)?;

    // write a little hidden easter egg .drove.txt file and commit it to get plowing
    let path = repo.path.join(".drove.txt");
    util::fs::write_to_path(&path, "👋 🐂");

    add(&repo, &path)?;
    if let Some(commit) = commit(&repo, "Initialized Repo 🐂")? {
        println!("Initial commit {}", commit.id);
    }

    Ok(repo)
}

/// # Get status of files in repository
///
/// What files are tracked, added, untracked, etc
///
/// Empty Repository:
///
/// ```
/// use liboxen::command;
/// # use liboxen::error::OxenError;
/// # use std::path::Path;
/// # fn main() -> Result<(), OxenError> {
///
/// let base_dir = Path::new("/tmp/repo_dir");
/// // Initialize empty repo
/// let repo = command::init(&base_dir)?;
/// // Get status on repo
/// let status = command::status(&repo)?;
/// assert!(status.is_clean());
///
/// # std::fs::remove_dir_all(base_dir)?;
/// # Ok(())
/// # }
/// ```
///
/// Repository with files
/// ```
/// use liboxen::command;
/// use liboxen::util;
/// # use liboxen::error::OxenError;
/// # use std::path::Path;
/// # fn main() -> Result<(), OxenError> {
///
/// let base_dir = Path::new("/tmp/repo_dir");
/// // Initialize empty repo
/// let repo = command::init(&base_dir)?;
///
/// // Write file to disk
/// let hello_file = base_dir.join("hello.txt");
/// util::fs::write_to_path(&hello_file, "Hello World");
///
/// // Get status on repo
/// let status = command::status(&repo)?;
/// assert_eq!(status.untracked_files.len(), 1);
///
/// # std::fs::remove_dir_all(base_dir)?;
/// # Ok(())
/// # }
/// ```
pub fn status(repository: &LocalRepository) -> Result<StagedData, OxenError> {
    let hidden_dir = util::fs::oxen_hidden_dir(&repository.path);
    if !hidden_dir.exists() {
        let err = NO_REPO_MSG.to_string();
        return Err(OxenError::basic_str(&err));
    }

    let committer = Committer::new(repository)?;
    let stager = Stager::new(repository)?;
    let status = stager.status(&committer)?;
    Ok(status)
}

/// # Get status of files in repository
pub fn add(repo: &LocalRepository, path: &Path) -> Result<(), OxenError> {
    let stager = Stager::new(repo)?;
    let committer = Committer::new(repo)?;
    stager.add(path, &committer)?;
    Ok(())
}

/// # Commit the staged files in the repo
pub fn commit(repo: &LocalRepository, message: &str) -> Result<Option<Commit>, OxenError> {
    let stager = Stager::new(repo)?;
    let mut committer = Committer::new(repo)?;
    let status = stager.status(&committer)?;
    if let Some(commit) = committer.commit(&status, message)? {
        stager.unstage()?;
        Ok(Some(commit))
    } else {
        Ok(None)
    }
}

/// # Get a log of all the commits
pub fn log(repo: &LocalRepository) -> Result<Vec<Commit>, OxenError> {
    let committer = Committer::new(repo)?;
    let commits = committer.list_commits()?;
    Ok(commits)
}

/// # Create a new branch
/// This creates a new pointer to the current commit with a name
/// It does not switch you to this branch, you still must call `checkout_branch`
pub fn create_branch(repo: &LocalRepository, name: &str) -> Result<(), OxenError> {
    let committer = Committer::new(repo)?;
    match committer.get_head_commit() {
        Ok(Some(head_commit)) => {
            committer.referencer.create_branch(name, &head_commit.id)?;
            Ok(())
        }
        _ => Err(OxenError::basic_str(
            "Err: No Commits. Cannot create a branch until you make your initial commit.",
        )),
    }
}

/// # Checkout a branch
/// This switches HEAD to point to the branch name
/// It also updates all the local files to be from the commit that this branch references
pub fn checkout_branch(repo: &LocalRepository, name: &str) -> Result<(), OxenError> {
    println!("checkout_branch {}", name);
    let committer = Committer::new(repo)?;
    let current_branch = committer.referencer.get_current_branch()?;

    // If we are already on the branch, do nothing
    if current_branch.name == name {
        eprintln!("Already on branch {}", name);
        return Ok(());
    }

    if committer.referencer.has_branch(name) && current_branch.name != name {
        committer.set_working_repo_to_branch(name)?;
        committer.referencer.set_head(name)?;

        Ok(())
    } else {
        let err = format!("Err: Branch not found '{}'", name);
        Err(OxenError::basic_str(&err))
    }
}

/// # Create a branch and check it out in one go
/// This creates a branch with name
/// Then switches HEAD to point to the branch
pub fn create_checkout_branch(repo: &LocalRepository, name: &str) -> Result<(), OxenError> {
    println!("create_checkout_branch {}", name);
    let committer = Committer::new(repo)?;
    match committer.get_head_commit() {
        Ok(Some(head_commit)) => {
            committer.referencer.create_branch(name, &head_commit.id)?;
            committer.referencer.set_head(name)?;
            Ok(())
        }
        _ => Err(OxenError::basic_str(
            "Err: No Commits. Cannot create a branch until you make your initial commit.",
        )),
    }
}

/// # List branches
pub fn list_branches(repo: &LocalRepository) -> Result<Vec<Branch>, OxenError> {
    let referencer = Referencer::new(repo)?;
    let branches = referencer.list_branches()?;
    Ok(branches)
}

/// # Get the current branch
pub fn current_branch(repo: &LocalRepository) -> Result<Branch, OxenError> {
    let referencer = Referencer::new(repo)?;
    let branch = referencer.get_current_branch()?;
    Ok(branch)
}

#[cfg(test)]
mod tests {

    use crate::command;
    use crate::error::OxenError;
    use crate::test;
    use crate::util;
    use crate::constants;

    #[test]
    fn test_command_init() -> Result<(), OxenError> {
        test::run_empty_repo_dir_test(|repo_dir| {
            // Init repo
            let repo = command::init(repo_dir)?;

            // Init should create the .oxen directory
            let hidden_dir = util::fs::oxen_hidden_dir(repo_dir);
            let config_file = util::fs::config_filepath(repo_dir);
            assert!(hidden_dir.exists());
            assert!(config_file.exists());

            // Name and id will be random but should be populated
            assert!(!repo.id.is_empty());
            assert!(!repo.name.is_empty());

            // We make an initial parent commit and branch called "main" 
            // just to make our lives easier down the line
            let orig_branch = command::current_branch(&repo)?;
            assert_eq!(orig_branch.name, constants::DEFAULT_BRANCH_NAME);
            assert!(!orig_branch.commit_id.is_empty());

            Ok(())
        })
    }

    #[test]
    fn test_command_status_empty() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            let repo_status = command::status(&repo)?;

            assert_eq!(repo_status.added_dirs.len(), 0);
            assert_eq!(repo_status.added_files.len(), 0);
            assert_eq!(repo_status.untracked_files.len(), 0);
            assert_eq!(repo_status.untracked_dirs.len(), 0);

            Ok(())
        })
    }

    #[test]
    fn test_command_commit_nothing_staged() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            let commits = command::log(&repo)?;
            let initial_len = commits.len();
            command::commit(&repo, "Should not work")?;
            // We should not have added any commits
            assert_eq!(commits.len(), initial_len);
            Ok(())
        })
    }

    #[test]
    fn test_command_status_has_txt_file() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write to file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello World");

            // Get status
            let repo_status = command::status(&repo)?;
            assert_eq!(repo_status.added_dirs.len(), 0);
            assert_eq!(repo_status.added_files.len(), 0);
            assert_eq!(repo_status.untracked_files.len(), 1);
            assert_eq!(repo_status.untracked_dirs.len(), 0);

            Ok(())
        })
    }

    #[test]
    fn test_command_add_file() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write to file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello World");

            // Track the file
            command::add(&repo, &hello_file)?;
            // Get status and make sure it is removed from the untracked, and added to the tracked
            let repo_status = command::status(&repo)?;
            assert_eq!(repo_status.added_dirs.len(), 0);
            assert_eq!(repo_status.added_files.len(), 1);
            assert_eq!(repo_status.untracked_files.len(), 0);
            assert_eq!(repo_status.untracked_dirs.len(), 0);

            Ok(())
        })
    }

    #[test]
    fn test_command_commit_file() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write to file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello World");

            // Track the file
            command::add(&repo, &hello_file)?;
            // Commit the file
            command::commit(&repo, "My message")?;

            // Get status and make sure it is removed from the untracked and added
            let repo_status = command::status(&repo)?;
            assert_eq!(repo_status.added_dirs.len(), 0);
            assert_eq!(repo_status.added_files.len(), 0);
            assert_eq!(repo_status.untracked_files.len(), 0);
            assert_eq!(repo_status.untracked_dirs.len(), 0);

            let commits = command::log(&repo)?;
            assert_eq!(commits.len(), 2);

            Ok(())
        })
    }


    #[test]
    fn test_command_checkout_current_branch_name_does_nothing() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write the first file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello");

            // Track & commit the file
            command::add(&repo, &hello_file)?;
            command::commit(&repo, "Added hello.txt")?;

            // Create and checkout branch
            let branch_name = "feature/world-explorer";
            command::create_checkout_branch(&repo, branch_name)?;
            command::checkout_branch(&repo, branch_name)?;

            Ok(())
        })
    }

    #[test]
    fn test_command_checkout_added_file() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write the first file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello");

            // Track & commit the file
            command::add(&repo, &hello_file)?;
            command::commit(&repo, "Added hello.txt")?;

            // Get the original branch name
            let orig_branch = command::current_branch(&repo)?;

            // Create and checkout branch
            let branch_name = "feature/world-explorer";
            command::create_checkout_branch(&repo, branch_name)?;

            // Write a second file
            let world_file = repo.path.join("world.txt");
            util::fs::write_to_path(&world_file, "World");

            // Track & commit the second file in the branch
            command::add(&repo, &world_file)?;
            command::commit(&repo, "Added world.txt")?;

            // Make sure we have both commits after the initial
            let commits = command::log(&repo)?;
            assert_eq!(commits.len(), 3);

            let branches = command::list_branches(&repo)?;
            assert_eq!(branches.len(), 2);

            // Make sure we have both files on disk in our repo dir
            assert!(hello_file.exists());
            assert!(world_file.exists());

            // Go back to the main branch
            command::checkout_branch(&repo, &orig_branch.name)?;

            // The world file should no longer be there
            assert!(hello_file.exists());
            assert!(!world_file.exists());

            // Go back to the world branch
            command::checkout_branch(&repo, &branch_name)?;
            assert!(hello_file.exists());
            assert!(world_file.exists());

            Ok(())
        })
    }

    #[test]
    fn test_command_checkout_added_file_keep_untracked() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write the first file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello");

            // Have another file lying around we will not remove
            let keep_file = repo.path.join("keep_me.txt");
            util::fs::write_to_path(&keep_file, "I am untracked, don't remove me");

            // Track & commit the file
            command::add(&repo, &hello_file)?;
            command::commit(&repo, "Added hello.txt")?;

            // Get the original branch name
            let orig_branch = command::current_branch(&repo)?;

            // Create and checkout branch
            let branch_name = "feature/world-explorer";
            command::create_checkout_branch(&repo, branch_name)?;

            // Write a second file
            let world_file = repo.path.join("world.txt");
            util::fs::write_to_path(&world_file, "World");

            // Track & commit the second file in the branch
            command::add(&repo, &world_file)?;
            command::commit(&repo, "Added world.txt")?;

            // Make sure we have both commits after the initial
            let commits = command::log(&repo)?;
            assert_eq!(commits.len(), 3);

            let branches = command::list_branches(&repo)?;
            assert_eq!(branches.len(), 2);

            // Make sure we have all files on disk in our repo dir
            assert!(hello_file.exists());
            assert!(world_file.exists());
            assert!(keep_file.exists());

            // Go back to the main branch
            command::checkout_branch(&repo, &orig_branch.name)?;

            // The world file should no longer be there
            assert!(hello_file.exists());
            assert!(!world_file.exists());
            assert!(keep_file.exists());

            // Go back to the world branch
            command::checkout_branch(&repo, &branch_name)?;
            assert!(hello_file.exists());
            assert!(world_file.exists());
            assert!(keep_file.exists());

            Ok(())
        })
    }

    #[test]
    fn test_command_checkout_modified_file() -> Result<(), OxenError> {
        test::run_empty_repo_test(|repo| {
            // Write the first file
            let hello_file = repo.path.join("hello.txt");
            util::fs::write_to_path(&hello_file, "Hello");

            // Track & commit the file
            command::add(&repo, &hello_file)?;
            command::commit(&repo, "Added hello.txt")?;

            // Get the original branch name
            let orig_branch = command::current_branch(&repo)?;

            // Create and checkout branch
            let branch_name = "feature/world-explorer";
            command::create_checkout_branch(&repo, branch_name)?;

            // Modify the file
            let hello_file = test::modify_txt_file(hello_file, "World")?;

            // Track & commit the change in the branch
            command::add(&repo, &hello_file)?;
            command::commit(&repo, "Changed file to world")?;

            // It should say World at this point
            assert_eq!(util::fs::read_from_path(&hello_file)?, "World");

            // Go back to the main branch
            command::checkout_branch(&repo, &orig_branch.name)?;

            // The file contents should be Hello, not World
            assert!(hello_file.exists());
            
            // It should be reverted back to Hello
            assert_eq!(util::fs::read_from_path(&hello_file)?, "Hello");

            Ok(())
        })
    }

    #[test]
    fn test_command_checkout_modified_file_in_subdirectory() -> Result<(), OxenError> {
        test::run_training_data_repo_test_no_commits(|repo| {
            // Get the original branch name
            let orig_branch = command::current_branch(&repo)?;

            // Track & commit the file
            let one_shot_path = repo.path.join("annotations/train/one_shot.txt");
            command::add(&repo, &one_shot_path)?;
            command::commit(&repo, "Adding one shot")?;

            // Get OG file contents
            let og_content = util::fs::read_from_path(&one_shot_path)?;

            let branch_name = "feature/change-the-shot";
            command::create_checkout_branch(&repo, branch_name)?;

            let new_contents = "train/cat_1.jpg 0";
            let one_shot_path = test::modify_txt_file(one_shot_path, new_contents)?;
            command::add(&repo, &one_shot_path)?;
            command::commit(&repo, "Changing one shot")?;

            // checkout OG and make sure it reverts
            command::checkout_branch(&repo, &orig_branch.name)?;
            let updated_content = util::fs::read_from_path(&one_shot_path)?;
            assert_eq!(og_content, updated_content);

            // checkout branch again and make sure it reverts
            command::checkout_branch(&repo, &branch_name)?;
            let updated_content = util::fs::read_from_path(&one_shot_path)?;
            assert_eq!(new_contents, updated_content);

            Ok(())
        })
    }

    #[test]
    fn test_command_add_top_level_dir_then_revert() -> Result<(), OxenError> {
        test::run_training_data_repo_test_no_commits(|repo| {
            // Get the original branch name
            let orig_branch = command::current_branch(&repo)?;

            // Create a branch to make the changes
            let branch_name = "feature/adding-train";
            command::create_checkout_branch(&repo, branch_name)?;

            // Track & commit (train dir already created in helper)
            let train_path = repo.path.join("train");
            let og_num_files = util::fs::rcount_files_in_dir(&train_path);

            command::add(&repo, &train_path)?;
            command::commit(&repo, "Adding train dir")?;

            // checkout OG and make sure it removes the train dir
            command::checkout_branch(&repo, &orig_branch.name)?;
            assert!(!train_path.exists());

            println!("-----------");

            // checkout branch again and make sure it reverts
            command::checkout_branch(&repo, &branch_name)?;
            assert!(train_path.exists());
            assert_eq!(util::fs::rcount_files_in_dir(&train_path), og_num_files);

            Ok(())
        })
    }
}
