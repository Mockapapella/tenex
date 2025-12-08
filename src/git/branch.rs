//! Git branch management

use anyhow::{Context, Result, bail};
use git2::{BranchType, Repository};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Information about a git branch for the branch selector
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Branch name (without remote prefix for remote branches)
    pub name: String,
    /// Full reference name (e.g., "refs/remotes/origin/main")
    pub full_name: String,
    /// Whether this is a remote branch
    pub is_remote: bool,
    /// Remote name (e.g., "origin") for remote branches
    pub remote: Option<String>,
    /// Last commit time (for sorting)
    pub last_commit_time: Option<SystemTime>,
}

/// Manager for git branch operations
pub struct Manager<'a> {
    repo: &'a Repository,
}

impl std::fmt::Debug for Manager<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manager").finish_non_exhaustive()
    }
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
        let head = self.repo.head().context("Failed to get HEAD reference")?;
        let commit = head.peel_to_commit().context("Failed to get HEAD commit")?;

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
            let name = head.shorthand().context("Branch name is not valid UTF-8")?;
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

    /// List all branches for the branch selector
    ///
    /// Returns branches sorted with:
    /// - "main" and "master" at the top (if they exist)
    /// - Local branches before remote branches
    /// - Within each section, sorted by most recent commit
    ///
    /// # Errors
    ///
    /// Returns an error if branches cannot be listed
    pub fn list_for_selector(&self) -> Result<Vec<BranchInfo>> {
        let mut local_branches = Vec::new();
        let mut remote_branches = Vec::new();

        // Get local branches
        let branches = self
            .repo
            .branches(Some(BranchType::Local))
            .context("Failed to list local branches")?;

        for branch_result in branches {
            let (branch, _) = branch_result.context("Failed to read branch")?;
            if let Some(name) = branch.name().context("Branch name is not valid UTF-8")? {
                #[expect(clippy::cast_sign_loss, reason = "Checked secs >= 0 before cast")]
                let commit_time = branch
                    .get()
                    .peel_to_commit()
                    .ok()
                    .map(|c| c.time())
                    .and_then(|t| {
                        let secs = t.seconds();
                        if secs >= 0 {
                            UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))
                        } else {
                            None
                        }
                    });

                local_branches.push(BranchInfo {
                    name: name.to_string(),
                    full_name: format!("refs/heads/{name}"),
                    is_remote: false,
                    remote: None,
                    last_commit_time: commit_time,
                });
            }
        }

        // Get remote branches
        let branches = self
            .repo
            .branches(Some(BranchType::Remote))
            .context("Failed to list remote branches")?;

        for branch_result in branches {
            let (branch, _) = branch_result.context("Failed to read branch")?;
            if let Some(full_name) = branch.name().context("Branch name is not valid UTF-8")? {
                // Skip HEAD references like "origin/HEAD"
                if full_name.ends_with("/HEAD") {
                    continue;
                }

                // Parse remote name and branch name (e.g., "origin/main" -> ("origin", "main"))
                let parts: Vec<&str> = full_name.splitn(2, '/').collect();
                let (remote_name, branch_name) = if parts.len() == 2 {
                    (Some(parts[0].to_string()), parts[1].to_string())
                } else {
                    (None, full_name.to_string())
                };

                #[expect(clippy::cast_sign_loss, reason = "Checked secs >= 0 before cast")]
                let commit_time = branch
                    .get()
                    .peel_to_commit()
                    .ok()
                    .map(|c| c.time())
                    .and_then(|t| {
                        let secs = t.seconds();
                        if secs >= 0 {
                            UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))
                        } else {
                            None
                        }
                    });

                remote_branches.push(BranchInfo {
                    name: branch_name,
                    full_name: format!("refs/remotes/{full_name}"),
                    is_remote: true,
                    remote: remote_name,
                    last_commit_time: commit_time,
                });
            }
        }

        // Sort local branches: main/master first, then by most recent commit
        local_branches.sort_by(|a, b| {
            let a_priority = Self::branch_priority(&a.name);
            let b_priority = Self::branch_priority(&b.name);

            // Higher priority first (main/master)
            match b_priority.cmp(&a_priority) {
                std::cmp::Ordering::Equal => {
                    // Then by most recent commit
                    b.last_commit_time.cmp(&a.last_commit_time)
                }
                other => other,
            }
        });

        // Sort remote branches: main/master first, then by most recent commit
        remote_branches.sort_by(|a, b| {
            let a_priority = Self::branch_priority(&a.name);
            let b_priority = Self::branch_priority(&b.name);

            match b_priority.cmp(&a_priority) {
                std::cmp::Ordering::Equal => b.last_commit_time.cmp(&a.last_commit_time),
                other => other,
            }
        });

        // Combine: local branches first, then remote
        let mut result = local_branches;
        result.extend(remote_branches);
        Ok(result)
    }

    /// Get priority for branch name sorting (higher = comes first)
    fn branch_priority(name: &str) -> u8 {
        match name {
            "main" => 2,
            "master" => 1,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    fn init_test_repo_with_commit() -> Result<(TempDir, Repository), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let repo = Repository::init(temp_dir.path())?;

        let sig = Signature::now("Test", "test@test.com")?;

        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test")?;

        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;

        {
            let tree = repo.find_tree(tree_id)?;
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
        }

        Ok((temp_dir, repo))
    }

    #[test]
    fn test_create_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        manager.create("feature/test")?;
        assert!(manager.exists("feature/test"));
        Ok(())
    }

    #[test]
    fn test_create_duplicate_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        manager.create("feature/test")?;
        let result = manager.create("feature/test");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_delete_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        manager.create("feature/test")?;
        assert!(manager.exists("feature/test"));

        manager.delete("feature/test")?;
        assert!(!manager.exists("feature/test"));
        Ok(())
    }

    #[test]
    fn test_delete_nonexistent_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let result = manager.delete("nonexistent");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_current_branch() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let current = manager.current()?;
        assert!(!current.is_empty());
        Ok(())
    }

    #[test]
    fn test_list_branches() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        manager.create("feature/a")?;
        manager.create("feature/b")?;

        let branches = manager.list()?;
        assert!(branches.len() >= 3);
        assert!(branches.iter().any(|b| b == "feature/a"));
        assert!(branches.iter().any(|b| b == "feature/b"));
        Ok(())
    }

    #[test]
    fn test_exists() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        assert!(!manager.exists("nonexistent"));
        manager.create("new-branch")?;
        assert!(manager.exists("new-branch"));
        Ok(())
    }

    #[test]
    fn test_checkout() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        manager.create("feature/test")?;
        manager.checkout("feature/test")?;

        assert_eq!(manager.current()?, "feature/test");
        Ok(())
    }

    #[test]
    fn test_checkout_nonexistent() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let result = manager.checkout("nonexistent");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_commit_count() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let current = manager.current()?;
        let count = manager.commit_count(&current)?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn test_create_from_commit() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let head = repo.head()?;
        let commit = head.peel_to_commit()?;
        let commit_id = commit.id().to_string();

        manager.create_from_commit("from-commit", &commit_id)?;
        assert!(manager.exists("from-commit"));
        Ok(())
    }

    #[test]
    fn test_create_from_invalid_commit() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let result = manager.create_from_commit("test", "invalid");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_manager_debug() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let debug = format!("{manager:?}");
        assert!(debug.contains("Manager"));
        Ok(())
    }

    #[test]
    fn test_branch_priority() {
        // main gets highest priority
        assert_eq!(Manager::branch_priority("main"), 2);
        // master gets second priority
        assert_eq!(Manager::branch_priority("master"), 1);
        // Other branches get zero priority
        assert_eq!(Manager::branch_priority("feature"), 0);
        assert_eq!(Manager::branch_priority("develop"), 0);
        assert_eq!(Manager::branch_priority("main-feature"), 0);
    }

    #[test]
    fn test_list_for_selector() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        // Create some test branches
        manager.create("feature/a")?;
        manager.create("main")?;
        manager.create("develop")?;

        let branches = manager.list_for_selector()?;

        // Should have branches
        assert!(!branches.is_empty());

        // Find the main branch index
        let main_idx = branches.iter().position(|b| b.name == "main");
        let feature_idx = branches.iter().position(|b| b.name == "feature/a");

        // main should come before feature branches (due to priority)
        if let (Some(main_i), Some(feature_i)) = (main_idx, feature_idx) {
            assert!(
                main_i < feature_i,
                "main should be sorted before feature branches"
            );
        }

        // All local branches should have is_remote = false
        for branch in &branches {
            if !branch.full_name.starts_with("refs/remotes/") {
                assert!(!branch.is_remote);
                assert!(branch.remote.is_none());
            }
        }

        Ok(())
    }

    #[test]
    fn test_list_for_selector_local_before_remote() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let manager = Manager::new(&repo);

        let branches = manager.list_for_selector()?;

        // Find first remote branch (if any)
        let first_remote_idx = branches.iter().position(|b| b.is_remote);

        // All branches before the first remote should be local
        if let Some(remote_idx) = first_remote_idx {
            for branch in &branches[..remote_idx] {
                assert!(
                    !branch.is_remote,
                    "Local branches should come before remote"
                );
            }
        }

        Ok(())
    }

    #[test]
    fn test_branch_info_debug() {
        let info = BranchInfo {
            name: "test".to_string(),
            full_name: "refs/heads/test".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("test"));
        assert!(debug.contains("BranchInfo"));
    }

    #[test]
    fn test_branch_info_clone() {
        let info = BranchInfo {
            name: "test".to_string(),
            full_name: "refs/heads/test".to_string(),
            is_remote: false,
            remote: Some("origin".to_string()),
            last_commit_time: Some(SystemTime::now()),
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, info.name);
        assert_eq!(cloned.full_name, info.full_name);
        assert_eq!(cloned.is_remote, info.is_remote);
        assert_eq!(cloned.remote, info.remote);
    }
}
