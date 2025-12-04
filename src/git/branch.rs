//! Git branch management

use anyhow::{bail, Context, Result};
use git2::{BranchType, Repository};

/// Manager for git branch operations
pub struct Manager<'a> {
    repo: &'a Repository,
}

impl<'a> Manager<'a> {
    /// Create a new branch manager for the given repository
    #[must_use]
    pub const fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    /// Create a new branch from HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be created
    pub fn create(&self, name: &str) -> Result<()> {
        let head = self
            .repo
            .head()
            .context("Failed to get HEAD reference")?;
        let commit = head
            .peel_to_commit()
            .context("Failed to get HEAD commit")?;

        self.repo
            .branch(name, &commit, false)
            .with_context(|| format!("Failed to create branch '{name}'"))?;

        Ok(())
    }

    /// Create a new branch from a specific commit
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be created
    pub fn create_from_commit(&self, name: &str, commit_id: &str) -> Result<()> {
        let oid = git2::Oid::from_str(commit_id)
            .with_context(|| format!("Invalid commit ID: {commit_id}"))?;
        let commit = self
            .repo
            .find_commit(oid)
            .with_context(|| format!("Commit not found: {commit_id}"))?;

        self.repo
            .branch(name, &commit, false)
            .with_context(|| format!("Failed to create branch '{name}'"))?;

        Ok(())
    }

    /// Delete a local branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be deleted
    pub fn delete(&self, name: &str) -> Result<()> {
        let mut branch = self
            .repo
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("Branch not found: {name}"))?;

        branch
            .delete()
            .with_context(|| format!("Failed to delete branch '{name}'"))?;

        Ok(())
    }

    /// Check if a branch exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        self.repo.find_branch(name, BranchType::Local).is_ok()
    }

    /// Get the current branch name
    ///
    /// # Errors
    ///
    /// Returns an error if HEAD is not a branch
    pub fn current(&self) -> Result<String> {
        let head = self.repo.head().context("Failed to get HEAD")?;

        if head.is_branch() {
            let name = head
                .shorthand()
                .context("Branch name is not valid UTF-8")?;
            Ok(name.to_string())
        } else {
            bail!("HEAD is not a branch (detached HEAD state)")
        }
    }

    /// List all local branches
    ///
    /// # Errors
    ///
    /// Returns an error if branches cannot be listed
    pub fn list(&self) -> Result<Vec<String>> {
        let branches = self
            .repo
            .branches(Some(BranchType::Local))
            .context("Failed to list branches")?;

        let mut names = Vec::new();
        for branch_result in branches {
            let (branch, _) = branch_result.context("Failed to read branch")?;
            if let Some(name) = branch.name().context("Branch name is not valid UTF-8")? {
                names.push(name.to_string());
            }
        }

        Ok(names)
    }

    /// Checkout a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be checked out
    pub fn checkout(&self, name: &str) -> Result<()> {
        let refname = format!("refs/heads/{name}");
        let obj = self
            .repo
            .revparse_single(&refname)
            .with_context(|| format!("Branch not found: {name}"))?;

        self.repo
            .checkout_tree(&obj, None)
            .with_context(|| format!("Failed to checkout tree for branch '{name}'"))?;

        self.repo
            .set_head(&refname)
            .with_context(|| format!("Failed to set HEAD to branch '{name}'"))?;

        Ok(())
    }

    /// Get the commit count on a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch or commits cannot be read
    pub fn commit_count(&self, name: &str) -> Result<usize> {
        let branch = self
            .repo
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("Branch not found: {name}"))?;

        let commit = branch
            .get()
            .peel_to_commit()
            .context("Failed to get branch commit")?;

        let mut revwalk = self.repo.revwalk().context("Failed to create revwalk")?;
        revwalk
            .push(commit.id())
            .context("Failed to push commit to revwalk")?;

        Ok(revwalk.count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    fn init_test_repo_with_commit() -> (TempDir, Repository) {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();

        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();

        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (temp_dir, repo)
    }

    #[test]
    fn test_create_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").unwrap();
        assert!(manager.exists("feature/test"));
    }

    #[test]
    fn test_create_duplicate_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").unwrap();
        let result = manager.create("feature/test");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").unwrap();
        assert!(manager.exists("feature/test"));

        manager.delete("feature/test").unwrap();
        assert!(!manager.exists("feature/test"));
    }

    #[test]
    fn test_delete_nonexistent_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.delete("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_current_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let current = manager.current().unwrap();
        assert!(!current.is_empty());
    }

    #[test]
    fn test_list_branches() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/a").unwrap();
        manager.create("feature/b").unwrap();

        let branches = manager.list().unwrap();
        assert!(branches.len() >= 3);
        assert!(branches.iter().any(|b| b == "feature/a"));
        assert!(branches.iter().any(|b| b == "feature/b"));
    }

    #[test]
    fn test_exists() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        assert!(!manager.exists("nonexistent"));
        manager.create("new-branch").unwrap();
        assert!(manager.exists("new-branch"));
    }

    #[test]
    fn test_checkout() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").unwrap();
        manager.checkout("feature/test").unwrap();

        assert_eq!(manager.current().unwrap(), "feature/test");
    }

    #[test]
    fn test_checkout_nonexistent() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.checkout("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_count() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let current = manager.current().unwrap();
        let count = manager.commit_count(&current).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_create_from_commit() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        let commit_id = commit.id().to_string();

        manager
            .create_from_commit("from-commit", &commit_id)
            .unwrap();
        assert!(manager.exists("from-commit"));
    }

    #[test]
    fn test_create_from_invalid_commit() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.create_from_commit("test", "invalid");
        assert!(result.is_err());
    }
}
