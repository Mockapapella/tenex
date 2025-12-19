//! Git branch management (CLI-based, Windows)

use anyhow::{Context, Result, bail};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Repository, git_command, git_output, git_run};

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
    /// Repository handle
    pub repo: &'a Repository,
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
        git_run(&self.repo.root, &["branch", name])
            .with_context(|| format!("Failed to create branch '{name}'"))
    }

    /// Create a new branch from a specific commit
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be created
    pub fn create_from_commit(&self, name: &str, commit_id: &str) -> Result<()> {
        git_run(&self.repo.root, &["branch", name, commit_id])
            .with_context(|| format!("Failed to create branch '{name}' at {commit_id}"))
    }

    /// Delete a local branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be deleted
    pub fn delete(&self, name: &str) -> Result<()> {
        git_run(&self.repo.root, &["branch", "-D", name])
            .with_context(|| format!("Failed to delete branch '{name}'"))
    }

    /// Check if a branch exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        let status = git_command()
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ])
            .current_dir(&self.repo.root)
            .status();

        status.map(|s| s.success()).unwrap_or(false)
    }

    /// Get the current branch name
    ///
    /// # Errors
    ///
    /// Returns an error if HEAD is not a branch
    pub fn current(&self) -> Result<String> {
        let name = git_output(&self.repo.root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .context("Failed to get HEAD")?;
        if name == "HEAD" {
            bail!("HEAD is not a branch (detached HEAD state)");
        }
        Ok(name)
    }

    /// List all local branches
    ///
    /// # Errors
    ///
    /// Returns an error if branches cannot be listed
    pub fn list(&self) -> Result<Vec<String>> {
        let output = git_output(
            &self.repo.root,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )
        .context("Failed to list branches")?;

        Ok(output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect())
    }

    /// Checkout a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be checked out
    pub fn checkout(&self, name: &str) -> Result<()> {
        git_run(&self.repo.root, &["checkout", name])
            .with_context(|| format!("Failed to checkout branch '{name}'"))
    }

    /// Get the commit count on a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch or commits cannot be read
    pub fn commit_count(&self, name: &str) -> Result<usize> {
        let output = git_output(&self.repo.root, &["rev-list", "--count", name])
            .with_context(|| format!("Failed to read commit count for '{name}'"))?;
        let count = output
            .parse::<usize>()
            .with_context(|| format!("Invalid commit count '{output}'"))?;
        Ok(count)
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
        let output = git_output(
            &self.repo.root,
            &[
                "for-each-ref",
                "--format=%(refname)\t%(refname:short)\t%(committerdate:unix)",
                "refs/heads",
                "refs/remotes",
            ],
        )
        .context("Failed to list branches")?;

        let mut local_branches = Vec::new();
        let mut remote_branches = Vec::new();

        for line in output.lines() {
            let mut parts = line.splitn(3, '\t');
            let Some(ref_name) = parts.next() else {
                continue;
            };
            let Some(short_name) = parts.next() else {
                continue;
            };
            let commit_time = parts
                .next()
                .and_then(|t| t.parse::<u64>().ok())
                .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)));

            if ref_name.starts_with("refs/remotes/") {
                if short_name.ends_with("/HEAD") {
                    continue;
                }

                let mut short_parts = short_name.splitn(2, '/');
                let remote_name = short_parts.next().map(str::to_string);
                let branch_name = short_parts
                    .next()
                    .map_or_else(|| short_name.to_string(), str::to_string);

                remote_branches.push(BranchInfo {
                    name: branch_name,
                    full_name: ref_name.to_string(),
                    is_remote: true,
                    remote: remote_name,
                    last_commit_time: commit_time,
                });
            } else if ref_name.starts_with("refs/heads/") {
                local_branches.push(BranchInfo {
                    name: short_name.to_string(),
                    full_name: ref_name.to_string(),
                    is_remote: false,
                    remote: None,
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
                std::cmp::Ordering::Equal => b.last_commit_time.cmp(&a.last_commit_time),
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
