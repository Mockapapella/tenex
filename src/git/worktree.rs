//! Git worktree management

use anyhow::{Context, Result, bail};
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Manager for git worktree operations
pub struct Manager<'a> {
    repo: &'a Repository,
}

impl std::fmt::Debug for Manager<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manager").finish_non_exhaustive()
    }
}

impl<'a> Manager<'a> {
    /// Create a new worktree manager for the given repository
    #[must_use]
    pub const fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    /// Create a new worktree for a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be created
    pub fn create(&self, path: &Path, branch: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory {}", parent.display())
            })?;
        }

        let branch_ref = self
            .repo
            .find_branch(branch, git2::BranchType::Local)
            .with_context(|| format!("Branch not found: {branch}"))?;

        let reference = branch_ref.into_reference();

        // Worktree name cannot contain slashes (it becomes a directory name in .git/worktrees/)
        let worktree_name = branch.replace('/', "-");

        self.repo
            .worktree(
                &worktree_name,
                path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&reference))),
            )
            .with_context(|| format!("Failed to create worktree at {}", path.display()))?;

        Ok(())
    }

    /// Create a worktree with a new branch from HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree or branch cannot be created
    pub fn create_with_new_branch(&self, path: &Path, branch: &str) -> Result<()> {
        debug!(branch, ?path, "Creating worktree with new branch");

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory {}", parent.display())
            })?;
        }

        let head = self.repo.head().context("Failed to get HEAD")?;
        let commit = head.peel_to_commit().context("Failed to get HEAD commit")?;

        let branch_ref = self
            .repo
            .branch(branch, &commit, false)
            .with_context(|| format!("Failed to create branch '{branch}'"))?;

        let reference = branch_ref.into_reference();

        // Worktree name cannot contain slashes (it becomes a directory name in .git/worktrees/)
        let worktree_name = branch.replace('/', "-");

        self.repo
            .worktree(
                &worktree_name,
                path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&reference))),
            )
            .with_context(|| format!("Failed to create worktree at {}", path.display()))?;

        info!(branch, ?path, "Worktree created");
        Ok(())
    }

    /// Remove a worktree and its associated branch
    ///
    /// Always attempts to delete the branch, even if the worktree is missing.
    /// This ensures cleanup works even if the worktree was manually removed.
    ///
    /// # Errors
    ///
    /// This function is idempotent and does not return errors for missing
    /// worktrees or branches.
    pub fn remove(&self, name: &str) -> Result<()> {
        debug!(name, "Removing worktree and branch");

        // Worktree name has slashes replaced with dashes
        let worktree_name = name.replace('/', "-");

        // Try to remove the worktree (may already be gone)
        if let Ok(worktree) = self.repo.find_worktree(&worktree_name) {
            let wt_path = worktree.path().to_path_buf();

            let _ = worktree.prune(Some(
                git2::WorktreePruneOptions::new()
                    .valid(true)
                    .working_tree(true),
            ));

            if wt_path.exists() {
                let _ = fs::remove_dir_all(&wt_path);
            }
            debug!(name, "Worktree pruned");
        }

        // Always try to delete the branch (critical for cleanup)
        // Ignore errors - branch may already be deleted
        let branch_mgr = super::BranchManager::new(self.repo);
        let _ = branch_mgr.delete(name);

        info!(name, "Worktree removed");
        Ok(())
    }

    /// List all worktrees
    ///
    /// # Errors
    ///
    /// Returns an error if worktrees cannot be listed
    pub fn list(&self) -> Result<Vec<Info>> {
        let worktrees = self.repo.worktrees().context("Failed to list worktrees")?;

        let mut infos = Vec::new();
        for name in worktrees.iter().flatten() {
            if let Ok(wt) = self.repo.find_worktree(name) {
                let is_locked = matches!(wt.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_)));
                infos.push(Info {
                    name: name.to_string(),
                    path: wt.path().to_path_buf(),
                    is_locked,
                });
            }
        }

        Ok(infos)
    }

    /// Check if a worktree exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        let worktree_name = name.replace('/', "-");
        self.repo.find_worktree(&worktree_name).is_ok()
    }

    /// Lock a worktree to prevent it from being pruned
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be locked
    pub fn lock(&self, name: &str, reason: Option<&str>) -> Result<()> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .with_context(|| format!("Worktree not found: {name}"))?;

        worktree
            .lock(reason)
            .with_context(|| format!("Failed to lock worktree '{name}'"))?;

        Ok(())
    }

    /// Unlock a worktree
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be unlocked
    pub fn unlock(&self, name: &str) -> Result<()> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .with_context(|| format!("Worktree not found: {name}"))?;

        let is_locked = matches!(
            worktree.is_locked(),
            Ok(git2::WorktreeLockStatus::Locked(_))
        );
        if !is_locked {
            bail!("Worktree '{name}' is not locked");
        }

        worktree
            .unlock()
            .with_context(|| format!("Failed to unlock worktree '{name}'"))?;

        Ok(())
    }

    /// Validate a worktree (check if it's valid)
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree is invalid
    pub fn validate(&self, name: &str) -> Result<()> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .with_context(|| format!("Worktree not found: {name}"))?;

        worktree
            .validate()
            .with_context(|| format!("Worktree '{name}' is invalid"))?;

        Ok(())
    }
}

/// Information about a worktree
#[derive(Debug, Clone)]
pub struct Info {
    /// Name of the worktree (usually branch name)
    pub name: String,
    /// Path to the worktree directory
    pub path: PathBuf,
    /// Whether the worktree is locked
    pub is_locked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use tempfile::TempDir;

    fn init_test_repo_with_commit() -> Result<(TempDir, Repository), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let repo = Repository::init(temp_dir.path())?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;

        {
            let tree = repo.find_tree(tree_id)?;
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
        }

        Ok((temp_dir, repo))
    }

    #[test]
    fn test_create_with_new_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-test");
        manager.create_with_new_branch(&wt_path, "feature-test")?;

        assert!(wt_path.exists());
        assert!(manager.exists("feature-test"));
        Ok(())
    }

    #[test]
    fn test_create_existing_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;

        let head = repo.head()?;
        let commit = head.peel_to_commit()?;
        repo.branch("existing-branch", &commit, false)?;

        let manager = Manager::new(&repo);
        let wt_path = temp_dir.path().join("worktrees").join("existing");
        manager.create(&wt_path, "existing-branch")?;

        assert!(wt_path.exists());
        Ok(())
    }

    #[test]
    fn test_create_nonexistent_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("test");
        let result = manager.create(&wt_path, "nonexistent");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_remove_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-remove");
        manager.create_with_new_branch(&wt_path, "feature-remove-test")?;
        assert!(manager.exists("feature-remove-test"));

        manager.remove("feature-remove-test")?;
        assert!(!manager.exists("feature-remove-test"));
        assert!(!wt_path.exists());
        Ok(())
    }

    #[test]
    fn test_remove_nonexistent() -> Result<(), Box<dyn std::error::Error>> {
        // Removing a non-existent worktree/branch should succeed (idempotent cleanup)
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let result = manager.remove("nonexistent");
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_remove_with_missing_worktree_but_existing_branch()
    -> Result<(), Box<dyn std::error::Error>> {
        // When worktree is manually removed but branch exists, cleanup should still delete branch
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("orphan-branch");
        manager.create_with_new_branch(&wt_path, "orphan-branch-test")?;

        // Verify branch exists
        assert!(
            repo.find_branch("orphan-branch-test", git2::BranchType::Local)
                .is_ok()
        );

        // Manually remove the worktree directory (simulating manual cleanup)
        fs::remove_dir_all(&wt_path)?;

        // Prune the worktree reference so git doesn't track it
        let worktree_name = "orphan-branch-test";
        if let Ok(wt) = repo.find_worktree(worktree_name) {
            let _ = wt.prune(Some(
                git2::WorktreePruneOptions::new()
                    .valid(true)
                    .working_tree(true),
            ));
        }

        // Now remove should still clean up the branch
        manager.remove("orphan-branch-test")?;

        // Branch should be deleted
        assert!(
            repo.find_branch("orphan-branch-test", git2::BranchType::Local)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_list_worktrees() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-list");
        manager.create_with_new_branch(&wt_path, "feature-list-test")?;

        let worktrees = manager.list()?;
        assert!(worktrees.iter().any(|wt| wt.name == "feature-list-test"));
        Ok(())
    }

    #[test]
    fn test_exists() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        assert!(!manager.exists("nonexistent"));

        let wt_path = temp_dir.path().join("worktrees").join("feature-exists");
        manager.create_with_new_branch(&wt_path, "feature-exists-test")?;

        assert!(manager.exists("feature-exists-test"));
        Ok(())
    }

    #[test]
    fn test_lock_and_unlock() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-lock");
        manager.create_with_new_branch(&wt_path, "feature-lock-test")?;

        manager.lock("feature-lock-test", Some("Testing lock"))?;

        let worktrees = manager.list()?;
        let locked_wt = worktrees
            .iter()
            .find(|wt| wt.name == "feature-lock-test")
            .ok_or("Expected worktree")?;
        assert!(locked_wt.is_locked);

        manager.unlock("feature-lock-test")?;

        let worktrees = manager.list()?;
        let unlocked_wt = worktrees
            .iter()
            .find(|wt| wt.name == "feature-lock-test")
            .ok_or("Expected worktree")?;
        assert!(!unlocked_wt.is_locked);
        Ok(())
    }

    #[test]
    fn test_unlock_not_locked() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-unlock");
        manager.create_with_new_branch(&wt_path, "feature-unlock-test")?;

        let result = manager.unlock("feature-unlock-test");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_validate() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-validate");
        manager.create_with_new_branch(&wt_path, "feature-validate-test")?;

        manager.validate("feature-validate-test")?;
        Ok(())
    }

    #[test]
    fn test_branch_name_with_slashes() -> Result<(), Box<dyn std::error::Error>> {
        // Integration test: branch names with slashes (like "tenex/feature-name")
        // should work correctly. The worktree name internally replaces slashes with dashes.
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        // Use a branch name with a slash (like tenex generates)
        let branch_name = "tenex/my-feature";
        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("tenex")
            .join("my-feature");

        // Create worktree with slashed branch name
        manager.create_with_new_branch(&wt_path, branch_name)?;

        // Verify worktree directory exists
        assert!(wt_path.exists());

        // Verify worktree can be found using original branch name
        assert!(manager.exists(branch_name));

        // Verify the worktree is a valid git worktree (has .git file)
        assert!(wt_path.join(".git").exists());

        // Verify the branch was created in the repository
        assert!(
            repo.find_branch(branch_name, git2::BranchType::Local)
                .is_ok()
        );

        // Verify we can validate the worktree using the branch name
        manager.validate(branch_name)?;

        // Verify we can remove the worktree using the branch name
        manager.remove(branch_name)?;
        assert!(!manager.exists(branch_name));
        assert!(!wt_path.exists());
        Ok(())
    }

    #[test]
    fn test_worktree_info() {
        let info = Info {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/test"),
            is_locked: false,
        };

        assert_eq!(info.name, "test");
        assert_eq!(info.path, PathBuf::from("/tmp/test"));
        assert!(!info.is_locked);
    }
}
