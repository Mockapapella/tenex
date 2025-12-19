//! Git worktree management (CLI-based, Windows)

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use super::{Repository, git_command, git_output, git_run};

#[expect(
    clippy::permissions_set_readonly_false,
    reason = "Windows worktree cleanup needs to clear the read-only attribute"
)]
fn clear_readonly_recursive(root: &Path) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if let Ok(metadata) = fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                if let Err(e) = fs::set_permissions(&path, permissions) {
                    warn!(path = ?path, error = %e, "Failed to clear read-only attribute");
                }
            }
        }

        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                let child = entry.path();
                if child.is_dir() {
                    stack.push(child);
                } else if let Ok(metadata) = entry.metadata() {
                    let mut permissions = metadata.permissions();
                    if permissions.readonly() {
                        permissions.set_readonly(false);
                        if let Err(e) = fs::set_permissions(&child, permissions) {
                            warn!(path = ?child, error = %e, "Failed to clear read-only attribute");
                        }
                    }
                }
            }
        }
    }
}

fn remove_dir_all_with_retries(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let mut last_err = None;
    for attempt in 0u64..10 {
        match fs::remove_dir_all(path) {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                last_err = None;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                clear_readonly_recursive(path);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));

        if !path.exists() {
            last_err = None;
            break;
        }
    }

    if path.exists() {
        let Some(e) = last_err else {
            bail!("Failed to remove directory at {}", path.display());
        };
        bail!("Failed to remove directory at {}: {e}", path.display());
    }

    Ok(())
}

/// Manager for git worktree operations
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

        let path_str = path.to_str().context("Worktree path is not valid UTF-8")?;
        git_run(&self.repo.root, &["worktree", "add", path_str, branch]).with_context(|| {
            format!(
                "Failed to create worktree at {} for {branch}",
                path.display()
            )
        })
    }

    /// Create a worktree with a new branch from HEAD
    ///
    /// If the branch already exists (e.g., from a previous run), it will be deleted
    /// and recreated from HEAD.
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

        if self.exists(branch) {
            debug!(branch, "Removing existing worktree before recreation");
            self.remove(branch)?;
        }

        let branch_mgr = super::BranchManager::new(self.repo);
        if branch_mgr.exists(branch) {
            debug!(branch, "Deleting existing branch before recreation");
            branch_mgr.delete(branch)?;
        }

        let path_str = path.to_str().context("Worktree path is not valid UTF-8")?;
        git_run(
            &self.repo.root,
            &["worktree", "add", "-b", branch, path_str],
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
    /// Returns an error if the worktree exists but cannot be removed after retries.
    /// Does not return errors for missing worktrees or branches.
    pub fn remove(&self, name: &str) -> Result<()> {
        debug!(name, "Removing worktree and branch");

        if let Some(record) = find_worktree_by_branch(&self.repo.root, name)? {
            let path_str = record
                .path
                .to_str()
                .context("Worktree path is not valid UTF-8")?;

            let output = git_command()
                .args(["worktree", "remove", "--force", path_str])
                .current_dir(&self.repo.root)
                .output()
                .context("Failed to execute git worktree remove")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(name, path = %path_str, "git worktree remove failed: {stderr}");
            }

            if let Err(e) = remove_dir_all_with_retries(&record.path) {
                warn!(name, path = ?record.path, error = %e, "Failed to remove worktree directory");
                return Err(e);
            }

            if let Err(e) = git_run(&self.repo.root, &["worktree", "prune"]) {
                warn!(name, error = %e, "Failed to prune worktrees after removal");
            }
        }

        let branch_mgr = super::BranchManager::new(self.repo);
        if let Err(e) = branch_mgr.delete(name) {
            warn!(name, error = %e, "Failed to delete branch during worktree cleanup");
        }

        info!(name, "Worktree removed");
        Ok(())
    }

    /// List all worktrees
    ///
    /// # Errors
    ///
    /// Returns an error if worktrees cannot be listed
    pub fn list(&self) -> Result<Vec<Info>> {
        let records = list_worktrees(&self.repo.root)?;
        let mut infos = Vec::new();
        for record in records {
            let name = record
                .branch
                .as_deref()
                .and_then(|branch| branch.strip_prefix("refs/heads/"))
                .map(str::to_string)
                .or_else(|| {
                    record
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "(unknown)".to_string());

            infos.push(Info {
                name,
                path: record.path,
                is_locked: record.is_locked,
            });
        }

        Ok(infos)
    }

    /// Check if a worktree exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        find_worktree_by_branch(&self.repo.root, name)
            .map(|entry| entry.is_some())
            .unwrap_or(false)
    }

    /// Lock a worktree to prevent it from being pruned
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be locked
    pub fn lock(&self, name: &str, reason: Option<&str>) -> Result<()> {
        let record = find_worktree_by_branch(&self.repo.root, name)?
            .with_context(|| format!("Worktree not found: {name}"))?;

        let path_str = record
            .path
            .to_str()
            .context("Worktree path is not valid UTF-8")?;

        let mut args = vec!["worktree", "lock"];
        let mut owned = Vec::new();
        if let Some(reason) = reason {
            owned.push("--reason".to_string());
            owned.push(reason.to_string());
        }
        for item in owned.iter() {
            args.push(item);
        }
        args.push(path_str);

        git_run(&self.repo.root, &args).with_context(|| format!("Failed to lock worktree '{name}'"))
    }

    /// Unlock a worktree
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be unlocked
    pub fn unlock(&self, name: &str) -> Result<()> {
        let record = find_worktree_by_branch(&self.repo.root, name)?
            .with_context(|| format!("Worktree not found: {name}"))?;

        if !record.is_locked {
            bail!("Worktree '{name}' is not locked");
        }

        let path_str = record
            .path
            .to_str()
            .context("Worktree path is not valid UTF-8")?;

        git_run(&self.repo.root, &["worktree", "unlock", path_str])
            .with_context(|| format!("Failed to unlock worktree '{name}'"))
    }

    /// Validate a worktree (check if it's valid)
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree is invalid
    pub fn validate(&self, name: &str) -> Result<()> {
        let record = find_worktree_by_branch(&self.repo.root, name)?
            .with_context(|| format!("Worktree not found: {name}"))?;

        if record.is_prunable {
            bail!("Worktree '{name}' is invalid");
        }

        if !record.path.exists() {
            bail!("Worktree '{name}' path does not exist");
        }

        Ok(())
    }

    /// Get the HEAD commit information for the main repository
    ///
    /// Returns (`branch_name`, `short_commit_hash`)
    ///
    /// # Errors
    ///
    /// Returns an error if HEAD cannot be read
    pub fn head_info(&self) -> Result<(String, String)> {
        let branch = git_output(&self.repo.root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .context("Failed to get HEAD")?;
        let short = git_output(&self.repo.root, &["rev-parse", "--short", "HEAD"])
            .context("Failed to get HEAD commit")?;

        let branch_name = if branch == "HEAD" {
            "HEAD (detached)".to_string()
        } else {
            branch
        };

        Ok((branch_name, short))
    }

    /// Get the HEAD commit information for an existing worktree
    ///
    /// Returns (`branch_name`, `short_commit_hash`) if the worktree exists and has a valid HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree or its HEAD cannot be read
    pub fn worktree_head_info(&self, name: &str) -> Result<(String, String)> {
        let record = find_worktree_by_branch(&self.repo.root, name)?
            .with_context(|| format!("Worktree not found: {name}"))?;

        let branch = git_output(&record.path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .context("Failed to get worktree HEAD")?;
        let short = git_output(&record.path, &["rev-parse", "--short", "HEAD"])
            .context("Failed to get worktree HEAD commit")?;

        let branch_name = if branch == "HEAD" {
            "HEAD (detached)".to_string()
        } else {
            branch
        };

        Ok((branch_name, short))
    }
}

#[derive(Debug, Clone)]
struct WorktreeRecord {
    path: PathBuf,
    branch: Option<String>,
    is_locked: bool,
    is_prunable: bool,
}

fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeRecord>> {
    let output = git_output(repo_root, &["worktree", "list", "--porcelain"])
        .context("Failed to list worktrees")?;

    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(WorktreeRecord {
                path: PathBuf::from(path.trim()),
                branch: None,
                is_locked: false,
                is_prunable: false,
            });
            continue;
        }

        let Some(record) = current.as_mut() else {
            continue;
        };

        if let Some(branch) = line.strip_prefix("branch ") {
            record.branch = Some(branch.trim().to_string());
            continue;
        }

        if line.starts_with("locked") {
            record.is_locked = true;
            continue;
        }

        if line.starts_with("prunable") {
            record.is_prunable = true;
        }
    }

    if let Some(record) = current.take() {
        records.push(record);
    }

    Ok(records)
}

fn find_worktree_by_branch(repo_root: &Path, name: &str) -> Result<Option<WorktreeRecord>> {
    let records = list_worktrees(repo_root)?;
    let target = format!("refs/heads/{name}");
    Ok(records
        .into_iter()
        .find(|record| record.branch.as_deref() == Some(target.as_str())))
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
