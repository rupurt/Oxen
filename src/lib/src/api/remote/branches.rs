use crate::api;
use crate::api::remote::client;
use crate::error::OxenError;
use crate::model::{Branch, Commit, LocalRepository, RemoteRepository};
use crate::view::{
    BranchLockResponse, BranchNewFromExisting, BranchRemoteMerge, BranchResponse, CommitResponse,
    ListBranchesResponse, StatusMessage,
};
use serde_json::json;

pub async fn get_by_name(
    repository: &RemoteRepository,
    branch_name: &str,
) -> Result<Option<Branch>, OxenError> {
    let uri = format!("/branches/{branch_name}");
    let url = api::endpoint::url_from_repo(repository, &uri)?;

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.get(&url).send().await {
        let status = res.status();
        if 404 == status {
            return Ok(None);
        }

        let body = client::parse_json_body(&url, res).await?;
        let response: Result<BranchResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(j_res) => Ok(Some(j_res.branch)),
            Err(err) => {
                log::debug!(
                    "remote::branches::get_by_name() Could not deserialize response [{}] {}",
                    err,
                    body
                );
                Ok(None)
            }
        }
    } else {
        let err = "Failed to get branch";
        log::error!("remote::branches::get_by_name() err: {}", err);
        Err(OxenError::basic_str(err))
    }
}

pub async fn create_from_or_get(
    repository: &RemoteRepository,
    new_name: &str,
    from_name: &str,
) -> Result<Branch, OxenError> {
    let url = api::endpoint::url_from_repo(repository, "/branches")?;
    log::debug!("create_from_or_get {}", url);

    let params = serde_json::to_string(&BranchNewFromExisting {
        new_name: new_name.to_string(),
        from_name: from_name.to_string(),
    })?;

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.post(&url).body(params).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<BranchResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(response) => Ok(response.branch),
            Err(err) => {
                let err = format!(
                    "Could not find branch [{}] or create it from branch [{}]: {}\n{}",
                    new_name, from_name, err, body
                );
                Err(OxenError::basic_str(err))
            }
        }
    } else {
        let msg = format!("Could not create branch {new_name}");
        log::error!("remote::branches::create_from_or_get() {}", msg);
        Err(OxenError::basic_str(&msg))
    }
}

pub async fn list(repository: &RemoteRepository) -> Result<Vec<Branch>, OxenError> {
    let url = api::endpoint::url_from_repo(repository, "/branches")?;

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.get(&url).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<ListBranchesResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(j_res) => Ok(j_res.branches),
            Err(err) => {
                log::debug!(
                    "remote::branches::list() Could not deserialize response [{}] {}",
                    err,
                    body
                );
                Err(OxenError::basic_str("Could not list remote branches"))
            }
        }
    } else {
        let err = "Failed to list branches";
        log::error!("remote::branches::list() err: {}", err);
        Err(OxenError::basic_str(err))
    }
}

pub async fn update(
    repository: &RemoteRepository,
    branch_name: &str,
    commit: &Commit,
) -> Result<Branch, OxenError> {
    let uri = format!("/branches/{branch_name}");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("remote::branches::update url: {}", url);

    let params = serde_json::to_string(&json!({ "commit_id": commit.id }))?;

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.put(&url).body(params).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<BranchResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(response) => Ok(response.branch),
            Err(err) => {
                let err = format!(
                    "Could not update branch [{}]: {}\n{}",
                    repository.name, err, body
                );
                Err(OxenError::basic_str(err))
            }
        }
    } else {
        let msg = format!("Could not update branch {branch_name}");
        log::error!("remote::branches::update() {}", msg);
        Err(OxenError::basic_str(&msg))
    }
}

// Creates a merge commit between two commits on the server if possible, returning the commit
pub async fn maybe_create_merge(
    repository: &RemoteRepository,
    branch_name: &str,
    local_head_id: &str,
    remote_head_id: &str, // Remote head pre-push - merge target
) -> Result<Commit, OxenError> {
    let uri = format!("/branches/{branch_name}/merge");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("remote::branches::maybe_create_merge url: {}", url);

    let commits = BranchRemoteMerge {
        client_commit_id: local_head_id.to_string(),
        server_commit_id: remote_head_id.to_string(),
    };
    let params = serde_json::to_string(&commits)?;

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.put(&url).body(params).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<CommitResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(response) => Ok(response.commit),
            Err(err) => {
                let err = format!(
                    "Could not create merge commit [{}]: {}\n{}",
                    repository.name, err, body
                );
                Err(OxenError::basic_str(err))
            }
        }
    } else {
        let msg = format!("Could not create merge commit {branch_name}");
        log::error!("remote::branches::update() {}", msg);
        Err(OxenError::basic_str(&msg))
    }
}

/// # Delete a remote branch
pub async fn delete_remote(
    repo: &LocalRepository,
    remote: &str,
    branch_name: &str,
) -> Result<Branch, OxenError> {
    if let Some(remote) = repo.get_remote(remote) {
        if let Some(remote_repo) = api::remote::repositories::get_by_remote(&remote).await? {
            if let Some(branch) =
                api::remote::branches::get_by_name(&remote_repo, branch_name).await?
            {
                api::remote::branches::delete(&remote_repo, &branch.name).await?;
                Ok(branch)
            } else {
                Err(OxenError::remote_branch_not_found(branch_name))
            }
        } else {
            Err(OxenError::remote_repo_not_found(&remote.url))
        }
    } else {
        Err(OxenError::remote_not_set(remote))
    }
}

pub async fn delete(
    repository: &RemoteRepository,
    branch_name: &str,
) -> Result<StatusMessage, OxenError> {
    let uri = format!("/branches/{branch_name}");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("Deleting branch: {}", url);

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.delete(&url).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<StatusMessage, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(val) => Ok(val),
            Err(_) => Err(OxenError::basic_str(format!(
                "could not delete branch \n\n{body}"
            ))),
        }
    } else {
        Err(OxenError::basic_str(
            "api::branches::delete() Request failed",
        ))
    }
}

pub async fn lock(
    repository: &RemoteRepository,
    branch_name: &str,
) -> Result<StatusMessage, OxenError> {
    let uri = format!("/branches/{branch_name}/lock");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("Locking branch: {}", url);

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.post(&url).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<StatusMessage, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(val) => Ok(val),
            Err(_) => Err(OxenError::remote_branch_locked()),
        }
    } else {
        Err(OxenError::basic_str("api::branches::lock() Request failed"))
    }
}

pub async fn unlock(
    repository: &RemoteRepository,
    branch_name: &str,
) -> Result<StatusMessage, OxenError> {
    let uri = format!("/branches/{branch_name}/unlock");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("Unlocking branch: {}", url);

    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.post(&url).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<StatusMessage, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(val) => Ok(val),
            Err(_) => Err(OxenError::basic_str(format!(
                "could not unlock branch \n\n{body}"
            ))),
        }
    } else {
        Err(OxenError::basic_str("api::branches::lock() Request failed"))
    }
}

pub async fn is_locked(
    repository: &RemoteRepository,
    branch_name: &str,
) -> Result<bool, OxenError> {
    let uri = format!("/branches/{branch_name}/lock");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("Checking if branch is locked: {}", url);
    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.get(&url).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<BranchLockResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(val) => Ok(val.is_locked),
            Err(_) => Err(OxenError::basic_str(format!(
                "could not check if branch is locked \n\n{body}"
            ))),
        }
    } else {
        Err(OxenError::basic_str(
            "api::branches::is_locked() Request failed",
        ))
    }
}

pub async fn latest_synced_commit(
    repository: &RemoteRepository,
    branch_name: &str,
) -> Result<Commit, OxenError> {
    let uri = format!("/branches/{branch_name}/latest_synced_commit");
    let url = api::endpoint::url_from_repo(repository, &uri)?;
    log::debug!("Retrieving latest synced commit for branch...");
    let client = client::new_for_url(&url)?;
    if let Ok(res) = client.get(&url).send().await {
        let body = client::parse_json_body(&url, res).await?;
        let response: Result<CommitResponse, serde_json::Error> = serde_json::from_str(&body);
        match response {
            Ok(res) => Ok(res.commit),
            Err(err) => Err(OxenError::basic_str(format!(
                "get_commit_by_id() Could not deserialize response [{err}]\n{body}"
            ))),
        }
    } else {
        Err(OxenError::basic_str(
            "api::branches::is_locked() Request failed",
        ))
    }
}

#[cfg(test)]
mod tests {

    use crate::api;
    use crate::command;
    use crate::config::UserConfig;
    use crate::constants;
    use crate::constants::DEFAULT_BRANCH_NAME;
    use crate::error::OxenError;
    use crate::model::NewCommitBody;
    use crate::test;

    #[tokio::test]
    async fn test_create_remote_branch() -> Result<(), OxenError> {
        test::run_empty_remote_repo_test(|_local_repo, remote_repo| async move {
            let name = "my-branch";
            let branch =
                api::remote::branches::create_from_or_get(&remote_repo, name, "main").await?;
            assert_eq!(branch.name, name);

            Ok(remote_repo)
        })
        .await
    }

    #[tokio::test]
    async fn test_create_remote_branch_from_existing() -> Result<(), OxenError> {
        test::run_empty_remote_repo_test(|_local_repo, remote_repo| async move {
            let name = "my-branch";
            let from = "old-branch";
            api::remote::branches::create_from_or_get(&remote_repo, from, DEFAULT_BRANCH_NAME)
                .await?;
            let branch =
                api::remote::branches::create_from_or_get(&remote_repo, name, from).await?;
            assert_eq!(branch.name, name);

            Ok(remote_repo)
        })
        .await
    }

    #[tokio::test]
    async fn test_get_branch_by_name() -> Result<(), OxenError> {
        test::run_empty_remote_repo_test(|_local_repo, remote_repo| async move {
            let branch_name = "my-branch";
            api::remote::branches::create_from_or_get(&remote_repo, branch_name, "main").await?;

            let branch = api::remote::branches::get_by_name(&remote_repo, branch_name).await?;
            assert!(branch.is_some());
            assert_eq!(branch.unwrap().name, branch_name);

            Ok(remote_repo)
        })
        .await
    }

    #[tokio::test]
    async fn test_list_remote_branches() -> Result<(), OxenError> {
        test::run_empty_remote_repo_test(|_local_repo, remote_repo| async move {
            api::remote::branches::create_from_or_get(
                &remote_repo,
                "branch-1",
                DEFAULT_BRANCH_NAME,
            )
            .await?;
            api::remote::branches::create_from_or_get(
                &remote_repo,
                "branch-2",
                DEFAULT_BRANCH_NAME,
            )
            .await?;

            let branches = api::remote::branches::list(&remote_repo).await?;
            assert_eq!(branches.len(), 3);

            assert!(branches.iter().any(|b| b.name == "branch-1"));
            assert!(branches.iter().any(|b| b.name == "branch-2"));
            assert!(branches.iter().any(|b| b.name == DEFAULT_BRANCH_NAME));

            Ok(remote_repo)
        })
        .await
    }

    #[tokio::test]
    async fn test_delete_branch() -> Result<(), OxenError> {
        test::run_empty_remote_repo_test(|_local_repo, remote_repo| async move {
            let branch_name = "my-branch";
            api::remote::branches::create_from_or_get(
                &remote_repo,
                branch_name,
                DEFAULT_BRANCH_NAME,
            )
            .await?;

            let branch = api::remote::branches::get_by_name(&remote_repo, branch_name).await?;
            assert!(branch.is_some());
            let branch = branch.unwrap();
            assert_eq!(branch.name, branch_name);

            api::remote::branches::delete(&remote_repo, branch_name).await?;

            let deleted_branch =
                api::remote::branches::get_by_name(&remote_repo, branch_name).await?;
            assert!(deleted_branch.is_none());

            Ok(remote_repo)
        })
        .await
    }

    #[tokio::test]
    async fn test_latest_synced_commit_no_lock() -> Result<(), OxenError> {
        test::run_empty_remote_repo_test(|_local_repo, remote_repo| async move {
            let branch_name = "my-branch";
            api::remote::branches::create_from_or_get(
                &remote_repo,
                branch_name,
                DEFAULT_BRANCH_NAME,
            )
            .await?;

            let branch = api::remote::branches::get_by_name(&remote_repo, branch_name)
                .await?
                .unwrap();
            let commit =
                api::remote::branches::latest_synced_commit(&remote_repo, branch_name).await?;
            assert_eq!(commit.id, branch.commit_id);
            Ok(remote_repo)
        })
        .await
    }

    #[test]
    fn test_rename_current_branch() -> Result<(), OxenError> {
        test::run_empty_local_repo_test(|repo| {
            // Create and checkout branch
            let og_branch_name = "feature/world-explorer";
            api::local::branches::create_checkout(&repo, og_branch_name)?;

            // Rename branch
            let new_branch_name = "feature/brave-new-world";
            api::local::branches::rename_current_branch(&repo, new_branch_name)?;

            // Check that the branch name has changed
            let current_branch = api::local::branches::current_branch(&repo)?.unwrap();
            assert_eq!(current_branch.name, new_branch_name);

            // Check that old branch no longer exists
            api::local::branches::list(&repo)?
                .iter()
                .for_each(|branch| {
                    assert_ne!(branch.name, og_branch_name);
                });

            Ok(())
        })
    }

    #[tokio::test]
    async fn test_delete_remote_branch() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed_async(|mut repo| async move {
            // Set the proper remote
            let remote = test::repo_remote_url_from(&repo.dirname());
            command::config::set_remote(&mut repo, constants::DEFAULT_REMOTE_NAME, &remote)?;

            // Create Remote
            let remote_repo = test::create_remote_repo(&repo).await?;

            // Push it
            command::push(&repo).await?;

            // Create new branch
            let new_branch_name = "my-branch";
            api::local::branches::create_checkout(&repo, new_branch_name)?;

            // Push new branch
            command::push_remote_branch(&repo, constants::DEFAULT_REMOTE_NAME, new_branch_name)
                .await?;

            // Delete the branch
            api::remote::branches::delete(&remote_repo, new_branch_name).await?;

            let remote_branches = api::remote::branches::list(&remote_repo).await?;
            assert_eq!(1, remote_branches.len());

            api::remote::repositories::delete(&remote_repo).await?;

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn test_should_not_push_branch_that_does_not_exist() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed_async(|mut repo| async move {
            // Set the proper remote
            let remote = test::repo_remote_url_from(&repo.dirname());
            command::config::set_remote(&mut repo, constants::DEFAULT_REMOTE_NAME, &remote)?;

            // Create Remote
            let remote_repo = test::create_remote_repo(&repo).await?;

            // Push main branch first
            if command::push_remote_branch(&repo, constants::DEFAULT_REMOTE_NAME, "main")
                .await
                .is_err()
            {
                panic!("Pushing main branch should work");
            }

            // Then try to push branch that doesn't exist
            if command::push_remote_branch(
                &repo,
                constants::DEFAULT_REMOTE_NAME,
                "branch-does-not-exist",
            )
            .await
            .is_ok()
            {
                panic!("Should not be able to push branch that does not exist");
            }

            let remote_branches = api::remote::branches::list(&remote_repo).await?;
            assert_eq!(1, remote_branches.len());

            api::remote::repositories::delete(&remote_repo).await?;

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn test_cannot_delete_branch_you_are_on() -> Result<(), OxenError> {
        test::run_select_data_repo_test_no_commits_async("labels", |repo| async move {
            let branch_name = "my-branch";
            api::local::branches::create_checkout(&repo, branch_name)?;

            // Add another commit on this branch that moves us ahead of main
            if api::local::branches::delete(&repo, branch_name).is_ok() {
                panic!("Should not be able to delete the branch you are on");
            }

            Ok(())
        })
        .await
    }

    #[test]
    fn test_cannot_force_delete_branch_you_are_on() -> Result<(), OxenError> {
        test::run_training_data_repo_test_no_commits(|repo| {
            let branch_name = "my-branch";
            api::local::branches::create_checkout(&repo, branch_name)?;

            // Add another commit on this branch that moves us ahead of main
            if api::local::branches::force_delete(&repo, branch_name).is_ok() {
                panic!("Should not be able to force delete the branch you are on");
            }

            Ok(())
        })
    }

    #[tokio::test]
    async fn test_cannot_delete_branch_that_is_ahead_of_current() -> Result<(), OxenError> {
        test::run_select_data_repo_test_no_commits_async("labels", |repo| async move {
            let og_branches = api::local::branches::list(&repo)?;
            let og_branch = api::local::branches::current_branch(&repo)?.unwrap();

            let branch_name = "my-branch";
            api::local::branches::create_checkout(&repo, branch_name)?;

            // Add another commit on this branch
            let labels_path = repo.path.join("labels.txt");
            command::add(&repo, labels_path)?;
            command::commit(&repo, "adding initial labels file")?;

            // Checkout main again
            command::checkout(&repo, og_branch.name).await?;

            // Should not be able to delete `my-branch` because it is ahead of `main`
            if api::local::branches::delete(&repo, branch_name).is_ok() {
                panic!(
                    "Should not be able to delete the branch that is ahead of the one you are on"
                );
            }

            // Should be one less branch
            let leftover_branches = api::local::branches::list(&repo)?;
            assert_eq!(og_branches.len(), leftover_branches.len() - 1);

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn test_force_delete_branch_that_is_ahead_of_current() -> Result<(), OxenError> {
        test::run_select_data_repo_test_no_commits_async("labels", |repo| async move {
            let og_branches = api::local::branches::list(&repo)?;
            let og_branch = api::local::branches::current_branch(&repo)?.unwrap();

            let branch_name = "my-branch";
            api::local::branches::create_checkout(&repo, branch_name)?;

            // Add another commit on this branch
            let labels_path = repo.path.join("labels.txt");
            command::add(&repo, labels_path)?;
            command::commit(&repo, "adding initial labels file")?;

            // Checkout main again
            command::checkout(&repo, og_branch.name).await?;

            // Force delete
            api::local::branches::force_delete(&repo, branch_name)?;

            // Should be one less branch
            let leftover_branches = api::local::branches::list(&repo)?;
            assert_eq!(og_branches.len(), leftover_branches.len());

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn test_branch_latest_synced_commit_no_lock() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed_async(|mut repo| async move {
            // Set remote
            let remote = test::repo_remote_url_from(&repo.dirname());
            command::config::set_remote(&mut repo, constants::DEFAULT_REMOTE_NAME, &remote)?;

            // Create Remote
            let remote_repo = test::create_remote_repo(&repo).await?;

            // Push it
            command::push(&repo).await?;
            let remote_main = api::remote::branches::get_by_name(&remote_repo, DEFAULT_BRANCH_NAME)
                .await?
                .unwrap();

            // Save commit
            let main_head_before = remote_main.commit_id.clone();
            // Check latest synced
            let latest_synced =
                api::remote::branches::latest_synced_commit(&remote_repo, DEFAULT_BRANCH_NAME)
                    .await?;
            assert_eq!(latest_synced.id, main_head_before);

            // Now push a new commit
            let labels_path = repo.path.join("labels.txt");
            test::write_txt_file_to_path(&labels_path, "I am the labels file")?;
            command::add(&repo, labels_path)?;
            command::commit(&repo, "adding labels file")?;
            command::push(&repo).await?;

            // Get main again, latest should have moved
            let remote_main = api::remote::branches::get_by_name(&remote_repo, DEFAULT_BRANCH_NAME)
                .await?
                .unwrap();
            let main_head_after = remote_main.commit_id.clone();
            let latest_synced =
                api::remote::branches::latest_synced_commit(&remote_repo, DEFAULT_BRANCH_NAME)
                    .await?;
            assert_eq!(latest_synced.id, main_head_after);

            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn test_branch_latest_synced_commit_with_lock() -> Result<(), OxenError> {
        test::run_training_data_repo_test_fully_committed_async(|mut repo| async move {
            // Set remote
            let remote = test::repo_remote_url_from(&repo.dirname());
            command::config::set_remote(&mut repo, constants::DEFAULT_REMOTE_NAME, &remote)?;

            // Create Remote
            let remote_repo = test::create_remote_repo(&repo).await?;

            // Push it
            command::push(&repo).await?;
            let remote_main = api::remote::branches::get_by_name(&remote_repo, DEFAULT_BRANCH_NAME)
                .await?
                .unwrap();

            // Save commit
            let main_head_before = remote_main.commit_id.clone();
            // Check latest synced
            let latest_synced =
                api::remote::branches::latest_synced_commit(&remote_repo, DEFAULT_BRANCH_NAME)
                    .await?;
            assert_eq!(latest_synced.id, main_head_before);

            // Lock up the branch
            api::remote::branches::lock(&remote_repo, DEFAULT_BRANCH_NAME).await?;

            let identifier = UserConfig::identifier()?;

            // Use remote staging to commit without releasing lock (push releases lock)
            let labels_path = repo.path.join("labels.txt");
            test::write_txt_file_to_path(&labels_path, "I am the labels file")?;
            api::remote::staging::add_files(
                &remote_repo,
                DEFAULT_BRANCH_NAME,
                &identifier,
                "./",
                vec![labels_path],
            )
            .await?;
            api::remote::staging::commit(
                &remote_repo,
                DEFAULT_BRANCH_NAME,
                &identifier,
                &NewCommitBody {
                    message: "adding labels file".to_string(),
                    author: "me".to_string(),
                    email: "me&aol.gov".to_string(),
                },
            )
            .await?;

            // Get main again, latest should still be behind
            let remote_main = api::remote::branches::get_by_name(&remote_repo, DEFAULT_BRANCH_NAME)
                .await?
                .unwrap();
            let main_head_after = remote_main.commit_id.clone();
            let latest_synced =
                api::remote::branches::latest_synced_commit(&remote_repo, DEFAULT_BRANCH_NAME)
                    .await?;
            assert!(latest_synced.id != main_head_after);
            assert_eq!(latest_synced.id, main_head_before);

            // Release the lock (as if push is complete)
            api::remote::branches::unlock(&remote_repo, DEFAULT_BRANCH_NAME).await?;
            let latest_synced_updated =
                api::remote::branches::latest_synced_commit(&remote_repo, DEFAULT_BRANCH_NAME)
                    .await?;
            assert_eq!(latest_synced_updated.id, main_head_after);

            Ok(())
        })
        .await
    }
}
