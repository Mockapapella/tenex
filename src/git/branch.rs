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
            .context(format!("Failed to delete branch '{name}'"))?;

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
            if let Some(name) = branch.name().ok().flatten() {
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
            .context(format!("Failed to checkout tree for branch '{name}'"))?;

        self.repo
            .set_head(&refname)
            .context(format!("Failed to set HEAD to branch '{name}'"))?;

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
        let mut remote_branch_infos = Vec::new();

        // Get local branches
        let branches = self
            .repo
            .branches(Some(BranchType::Local))
            .context("Failed to list local branches")?;

        for branch_result in branches {
            let (branch, _) = branch_result.context("Failed to read branch")?;
            let Some(name) = branch.name().ok().flatten() else {
                continue;
            };

            let commit_time = branch
                .get()
                .peel_to_commit()
                .ok()
                .map(|c| c.time())
                .and_then(|t| {
                    u64::try_from(t.seconds())
                        .ok()
                        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
                });

            local_branches.push(BranchInfo {
                name: name.to_string(),
                full_name: format!("refs/heads/{name}"),
                is_remote: false,
                remote: None,
                last_commit_time: commit_time,
            });
        }

        // Get remote branches
        let branches = self
            .repo
            .branches(Some(BranchType::Remote))
            .context("Failed to list remote branches")?;

        for branch_result in branches {
            let (branch, _) = branch_result.context("Failed to read branch")?;
            let Some(full_name) = branch.name().ok().flatten() else {
                continue;
            };

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

            let commit_time = branch
                .get()
                .peel_to_commit()
                .ok()
                .map(|c| c.time())
                .and_then(|t| {
                    u64::try_from(t.seconds())
                        .ok()
                        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
                });

            remote_branch_infos.push(BranchInfo {
                name: branch_name,
                full_name: format!("refs/remotes/{full_name}"),
                is_remote: true,
                remote: remote_name,
                last_commit_time: commit_time,
            });
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
        remote_branch_infos.sort_by(|a, b| {
            let a_priority = Self::branch_priority(&a.name);
            let b_priority = Self::branch_priority(&b.name);

            match b_priority.cmp(&a_priority) {
                std::cmp::Ordering::Equal => b.last_commit_time.cmp(&a.last_commit_time),
                other => other,
            }
        });

        // Combine: local branches first, then remote
        let mut result = local_branches;
        result.extend(remote_branch_infos);
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
