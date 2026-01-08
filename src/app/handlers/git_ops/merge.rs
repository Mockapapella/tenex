//! Merge flow (branch selector + merge execution).

use crate::agent::{Agent, ChildConfig};
use crate::git;
use crate::mux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode, MergeBranchSelectorMode, SuccessModalMode};

use super::super::Actions;

/// Result of a git merge operation
enum MergeResult {
    Success,
    Conflict,
    Failed(String),
}

impl Actions {
    /// Start the merge flow - show branch selector (Ctrl+m or Ctrl+n)
    ///
    /// # Errors
    ///
    /// Returns an error if the git repository cannot be opened or branches cannot be listed.
    pub fn merge_branch(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to merge.".to_string(),
            }
            .into());
        };

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        debug!(branch = %current_branch, "Starting merge flow");

        // Fetch branches for selector
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_merge(agent_id, current_branch);
        app_data.review.start(branches);
        Ok(MergeBranchSelectorMode.into())
    }

    /// Execute the merge operation
    ///
    /// Merges the agent's branch INTO the target branch (e.g., feature -> master).
    /// If the target branch has a worktree, merges directly there.
    /// Otherwise, merges from the main repo.
    ///
    /// # Errors
    ///
    /// Returns an error if the merge operation fails
    pub fn execute_merge(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for merge".to_string(),
            }
            .into());
        };

        // Verify agent exists
        if app_data.storage.get(agent_id).is_none() {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "Agent not found".to_string(),
            }
            .into());
        }

        let source_branch = app_data.git_op.branch_name.clone(); // Agent's branch (e.g., tenex/feature)
        let target_branch = app_data.git_op.target_branch.clone(); // Branch to merge into (e.g., master)

        debug!(
            source = %source_branch,
            target = %target_branch,
            "Executing merge: {source_branch} -> {target_branch}"
        );

        // Check if target branch has a worktree
        if let Some(worktree_path) = Self::find_worktree_for_branch(&target_branch)? {
            Self::execute_merge_in_worktree(
                app_data,
                &source_branch,
                &target_branch,
                &worktree_path,
            )
        } else {
            Self::execute_merge_in_main_repo(app_data, &source_branch, &target_branch)
        }
    }

    /// Find the worktree path for a branch, if one exists
    pub(super) fn find_worktree_for_branch(branch: &str) -> Result<Option<std::path::PathBuf>> {
        let output = crate::git::git_command()
            .args(["worktree", "list", "--porcelain"])
            .output()
            .context("Failed to list worktrees")?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut current_worktree: Option<std::path::PathBuf> = None;

        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                current_worktree = Some(std::path::PathBuf::from(path));
            } else if let Some(worktree_branch) = line.strip_prefix("branch refs/heads/")
                && worktree_branch == branch
            {
                return Ok(current_worktree);
            }
        }

        Ok(None)
    }

    /// Execute merge directly in a worktree (when target branch is checked out there)
    fn execute_merge_in_worktree(
        app_data: &mut AppData,
        source_branch: &str,
        target_branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<AppMode> {
        debug!(
            source = %source_branch,
            target = %target_branch,
            worktree = %worktree_path.display(),
            "Merging in worktree"
        );

        // Merge directly in the worktree
        let merge_output = crate::git::git_command()
            .args([
                "merge",
                source_branch,
                "-m",
                &format!("Merge {source_branch} into {target_branch}"),
            ])
            .current_dir(worktree_path)
            .output()
            .context("Failed to execute merge")?;

        if !merge_output.status.success() {
            let stdout = String::from_utf8_lossy(&merge_output.stdout);
            let stderr = String::from_utf8_lossy(&merge_output.stderr);
            let combined = format!("{stdout}{stderr}");

            // Check if there are merge conflicts (git outputs to stdout)
            if combined.contains("CONFLICT") || combined.contains("Automatic merge failed") {
                info!(
                    source = %source_branch,
                    target = %target_branch,
                    "Merge has conflicts - spawning terminal"
                );

                return Self::spawn_merge_conflict_terminal_in_worktree(
                    app_data,
                    source_branch,
                    target_branch,
                    worktree_path,
                );
            }

            // Show error with both stdout and stderr for context
            let error_msg = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                "Unknown error".to_string()
            };
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: format!("Merge failed: {error_msg}"),
            }
            .into());
        }

        info!(
            source = %source_branch,
            target = %target_branch,
            "Merge successful in worktree"
        );
        app_data.git_op.clear();
        app_data.review.clear();
        Ok(SuccessModalMode {
            message: format!("Merged {source_branch} into {target_branch}"),
        }
        .into())
    }

    /// Spawn a terminal for merge conflict resolution in a worktree
    fn spawn_merge_conflict_terminal_in_worktree(
        app_data: &mut AppData,
        source_branch: &str,
        target_branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<AppMode> {
        // Find the agent that owns this worktree
        let owner_agent = app_data
            .storage
            .agents
            .iter()
            .find(|a| a.worktree_path == worktree_path && a.is_root())
            .map(|a| a.id);

        if let Some(root_id) = owner_agent {
            let root = app_data
                .storage
                .get(root_id)
                .ok_or_else(|| anyhow::anyhow!("Root agent not found"))?;

            let root_session = root.mux_session.clone();
            let branch = root.branch.clone();

            let title = format!("Merge Conflict: {source_branch} -> {target_branch}");

            // Reserve a window index
            let window_index = app_data.storage.reserve_window_indices(root_id);

            // Create child agent marked as terminal
            let mut terminal = Agent::new_child(
                title.clone(),
                "terminal".to_string(),
                branch,
                worktree_path.to_path_buf(),
                ChildConfig {
                    parent_id: root_id,
                    mux_session: root_session.clone(),
                    window_index,
                },
            );
            terminal.is_terminal = true;

            // Create session manager and window
            let session_manager = SessionManager::new();
            let actual_index =
                session_manager.create_window(&root_session, &title, worktree_path, None)?;

            // Resize the new window to match preview dimensions
            if let Some((width, height)) = app_data.ui.preview_dimensions {
                let window_target = SessionManager::window_target(&root_session, actual_index);
                let _ = session_manager.resize_window(&window_target, width, height);
            }

            // Update window index if it differs
            terminal.window_index = Some(actual_index);

            // Send the startup command
            let window_target = SessionManager::window_target(&root_session, actual_index);
            session_manager.send_keys_and_submit(&window_target, "git status")?;

            app_data.storage.add(terminal);

            // Expand the parent to show the new terminal
            if let Some(parent) = app_data.storage.get_mut(root_id) {
                parent.collapsed = false;
            }

            app_data.storage.save()?;

            app_data.set_status(format!("Opened terminal for conflict resolution: {title}"));
        } else {
            // No agent owns this worktree, just show a message
            app_data.set_status(format!(
                "Merge conflict in {target_branch}. Resolve in: {}",
                worktree_path.display()
            ));
        }

        app_data.git_op.clear();
        app_data.review.clear();

        Ok(AppMode::normal())
    }

    /// Execute merge from main repo (when target branch has no worktree)
    fn execute_merge_in_main_repo(
        app_data: &mut AppData,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<AppMode> {
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;

        debug!(
            source = %source_branch,
            target = %target_branch,
            "Merging in main repo"
        );

        // Prepare: stash changes and get current branch
        let did_stash = Self::git_stash_push(&repo_path)?;
        let original_branch = Self::git_get_current_branch(&repo_path)?;

        // Checkout target branch
        if !Self::git_checkout(&repo_path, target_branch)? {
            Self::restore_git_state(&repo_path, did_stash);
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: format!("Failed to checkout {target_branch}"),
            }
            .into());
        }

        // Attempt merge
        let merge_result = Self::git_merge(
            &repo_path,
            source_branch,
            target_branch,
            &original_branch,
            did_stash,
        );

        let next = match merge_result {
            MergeResult::Success => {
                Self::restore_git_state(&repo_path, did_stash);
                info!(source = %source_branch, target = %target_branch, "Merge successful");
                SuccessModalMode {
                    message: format!("Merged {source_branch} into {target_branch}"),
                }
                .into()
            }
            MergeResult::Conflict => {
                // Stay on target branch, don't restore stash - user needs to resolve.
                return Self::spawn_conflict_terminal(
                    app_data,
                    &format!("Merge Conflict: {source_branch} -> {target_branch}"),
                    "git status",
                );
            }
            MergeResult::Failed(error_msg) => {
                Self::git_checkout(&repo_path, &original_branch)?;
                Self::restore_git_state(&repo_path, did_stash);
                ErrorModalMode {
                    message: format!("Merge failed: {error_msg}"),
                }
                .into()
            }
        };

        app_data.git_op.clear();
        app_data.review.clear();
        Ok(next)
    }

    /// Stash any uncommitted changes
    fn git_stash_push(repo_path: &std::path::Path) -> Result<bool> {
        let stash_output = crate::git::git_command()
            .args(["stash", "push", "-m", "tenex-merge-temp"])
            .current_dir(repo_path)
            .output()
            .context("Failed to stash changes")?;

        Ok(String::from_utf8_lossy(&stash_output.stdout).contains("Saved working"))
    }

    /// Get current branch name
    fn git_get_current_branch(repo_path: &std::path::Path) -> Result<String> {
        let output = crate::git::git_command()
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo_path)
            .output()
            .context("Failed to get current branch")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Checkout a branch, returns success status
    fn git_checkout(repo_path: &std::path::Path, branch: &str) -> Result<bool> {
        let output = crate::git::git_command()
            .args(["checkout", branch])
            .current_dir(repo_path)
            .output()
            .context("Failed to checkout branch")?;

        Ok(output.status.success())
    }

    /// Restore git state (checkout original branch and pop stash)
    fn restore_git_state(repo_path: &std::path::Path, did_stash: bool) {
        if did_stash {
            let _ = crate::git::git_command()
                .args(["stash", "pop"])
                .current_dir(repo_path)
                .output();
        }
    }

    /// Perform git merge and return result
    fn git_merge(
        repo_path: &std::path::Path,
        source_branch: &str,
        target_branch: &str,
        original_branch: &str,
        did_stash: bool,
    ) -> MergeResult {
        let merge_output = match crate::git::git_command()
            .args([
                "merge",
                source_branch,
                "-m",
                &format!("Merge {source_branch} into {target_branch}"),
            ])
            .current_dir(repo_path)
            .output()
        {
            Ok(output) => output,
            Err(e) => return MergeResult::Failed(e.to_string()),
        };

        if merge_output.status.success() {
            // Go back to original branch after successful merge
            let _ = crate::git::git_command()
                .args(["checkout", original_branch])
                .current_dir(repo_path)
                .output();
            return MergeResult::Success;
        }

        let stdout = String::from_utf8_lossy(&merge_output.stdout);
        let stderr = String::from_utf8_lossy(&merge_output.stderr);
        let combined = format!("{stdout}{stderr}");

        if combined.contains("CONFLICT") || combined.contains("Automatic merge failed") {
            MergeResult::Conflict
        } else {
            // Restore state before returning error
            let _ = crate::git::git_command()
                .args(["checkout", original_branch])
                .current_dir(repo_path)
                .output();
            if did_stash {
                let _ = crate::git::git_command()
                    .args(["stash", "pop"])
                    .current_dir(repo_path)
                    .output();
            }

            let error_msg = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                "Unknown error".to_string()
            };
            MergeResult::Failed(error_msg)
        }
    }
}
