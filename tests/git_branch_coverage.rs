//! Coverage tests for git branch operations in non-test builds.

use anyhow::{Context, Result};
use git2::{Repository, RepositoryInitOptions, Signature};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tenex::git::BranchManager;

fn init_repo_with_commit() -> Result<(TempDir, Repository)> {
    let temp_dir = TempDir::new().context("create temp dir")?;
    let mut init_opts = RepositoryInitOptions::new();
    init_opts.initial_head("master");
    let repo = Repository::init_opts(temp_dir.path(), &init_opts).context("init repository")?;
    repo.set_head("refs/heads/master").context("set HEAD")?;

    let sig = Signature::now("Test", "test@test.com").context("create signature")?;
    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Test").context("write README.md")?;

    let mut index = repo.index().context("open repository index")?;
    index
        .add_path(Path::new("README.md"))
        .context("add README.md")?;
    index.write().context("write index")?;

    let tree_id = index.write_tree().context("write tree")?;

    {
        let tree = repo.find_tree(tree_id).context("find tree")?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .context("commit README.md")?;
    }

    Ok((temp_dir, repo))
}

fn init_empty_repo() -> Result<(TempDir, Repository)> {
    let temp_dir = TempDir::new().context("create temp dir")?;
    let mut init_opts = RepositoryInitOptions::new();
    init_opts.initial_head("master");
    let repo = Repository::init_opts(temp_dir.path(), &init_opts).context("init repository")?;
    repo.set_head("refs/heads/master").context("set HEAD")?;
    Ok((temp_dir, repo))
}

#[test]
fn test_branch_manager_methods_are_exercised_in_non_test_build() -> Result<()> {
    let (_temp_dir, repo) = init_repo_with_commit()?;
    let manager = BranchManager::new(&repo);

    let debug = format!("{manager:?}");
    assert!(debug.contains("Manager"));

    manager.create("feature/test")?;
    assert!(manager.exists("feature/test"));

    let head = repo.head().context("read repository HEAD")?;
    let commit = head.peel_to_commit().context("peel HEAD to commit")?;
    let commit_id = commit.id().to_string();
    manager.create_from_commit("from-commit", &commit_id)?;
    assert!(manager.exists("from-commit"));

    assert_eq!(manager.current()?, "master");
    assert_eq!(manager.commit_count("master")?, 1);

    manager.checkout("feature/test")?;
    assert_eq!(manager.current()?, "feature/test");

    Ok(())
}

#[test]
fn test_branch_manager_error_paths_cover_more_regions_and_branches() -> Result<()> {
    let (_temp_dir, empty_repo) = init_empty_repo()?;
    let empty_manager = BranchManager::new(&empty_repo);

    let err = match empty_manager.create("feature/needs-head") {
        Ok(()) => anyhow::bail!("empty repo create unexpectedly succeeded"),
        Err(err) => err,
    };
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("HEAD") || err_msg.contains("commit"),
        "err: {err_msg}"
    );

    let (_temp_dir, repo) = init_repo_with_commit()?;
    let manager = BranchManager::new(&repo);

    manager.create("dup-create")?;
    let err = match manager.create("dup-create") {
        Ok(()) => anyhow::bail!("create unexpectedly succeeded for duplicate branch"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("Failed to create branch"),
        "err: {err}"
    );

    manager.create("dup-branch")?;
    let head = repo.head().context("read repository HEAD")?;
    let commit = head.peel_to_commit().context("peel HEAD to commit")?;
    let commit_id = commit.id().to_string();
    let err = match manager.create_from_commit("dup-branch", &commit_id) {
        Ok(()) => anyhow::bail!("create_from_commit unexpectedly succeeded for duplicate branch"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("Failed to create branch"),
        "err: {err}"
    );

    let err = match manager
        .create_from_commit("missing-commit", "0123456789012345678901234567890123456789")
    {
        Ok(()) => anyhow::bail!("create_from_commit unexpectedly succeeded for missing commit"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("Commit not found"), "err: {err}");

    let err = match manager.create_from_commit("invalid-commit", "invalid") {
        Ok(()) => anyhow::bail!("create_from_commit unexpectedly succeeded for invalid commit id"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("Invalid commit ID"), "err: {err}");

    let Err(err) = manager.commit_count("nonexistent") else {
        anyhow::bail!("commit_count unexpectedly succeeded for unknown branch");
    };
    assert!(err.to_string().contains("Branch not found"), "err: {err}");

    let err = match manager.checkout("missing-branch") {
        Ok(()) => anyhow::bail!("checkout unexpectedly succeeded for unknown branch"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("Branch not found"), "err: {err}");

    // Detach HEAD so current() exercises the detached HEAD error branch.
    let head = repo.head().context("read repository HEAD")?;
    let commit = head.peel_to_commit().context("peel HEAD to commit")?;
    repo.set_head_detached(commit.id())
        .context("detach repository HEAD")?;
    let Err(err) = manager.current() else {
        anyhow::bail!("current unexpectedly succeeded for detached HEAD");
    };
    assert!(err.to_string().contains("detached HEAD"), "err: {err}");

    Ok(())
}
