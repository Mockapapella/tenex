//! Merge flow (branch selector + merge execution).

use crate::agent::{Agent, ChildConfig};
use crate::app::state::App;
use crate::git;
use crate::mux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info};

use super::super::Actions;

/// Result of a git merge operation
enum MergeResult {
    Success,
    Conflict,
    Failed(String),
}

impl Actions {
    /// Start the merge flow - show branch selector (Ctrl+m or Ctrl+n)
    pub(crate) fn merge_branch(app: &mut App) -> Result<()> {
        let Some(agent) = app.selected_agent() else {
            app.set_error("No agent selected. Select an agent first to merge.");
            return Ok(());
        };

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        debug!(branch = %current_branch, "Starting merge flow");

        // Fetch branches for selector
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app.start_merge(agent_id, current_branch, branches);
        Ok(())
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
    pub fn execute_merge(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for merge"))?;

        // Verify agent exists
        if app.storage.get(agent_id).is_none() {
            anyhow::bail!("Agent not found");
        }

        let source_branch = app.git_op.branch_name.clone(); // Agent's branch (e.g., tenex/feature)
        let target_branch = app.git_op.target_branch.clone(); // Branch to merge into (e.g., master)

        debug!(
            source = %source_branch,
            target = %target_branch,
            "Executing merge: {source_branch} -> {target_branch}"
        );

        // Check if target branch has a worktree
        if let Some(worktree_path) = Self::find_worktree_for_branch(&target_branch)? {
            Self::execute_merge_in_worktree(app, &source_branch, &target_branch, &worktree_path)
        } else {
            Self::execute_merge_in_main_repo(app, &source_branch, &target_branch)
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
        app: &mut App,
        source_branch: &str,
        target_branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<()> {
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

                Self::spawn_merge_conflict_terminal_in_worktree(
                    app,
                    source_branch,
                    target_branch,
                    worktree_path,
                )?;
                return Ok(());
            }

            // Show error with both stdout and stderr for context
            let error_msg = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else if !stdout.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                "Unknown error".to_string()
            };
            app.set_error(format!("Merge failed: {error_msg}"));
            app.clear_git_op_state();
            app.clear_review_state();
            return Ok(());
        }

        info!(
            source = %source_branch,
            target = %target_branch,
            "Merge successful in worktree"
        );
        app.show_success(format!("Merged {source_branch} into {target_branch}"));
        app.clear_git_op_state();
        app.clear_review_state();

        Ok(())
    }

    /// Spawn a terminal for merge conflict resolution in a worktree
    fn spawn_merge_conflict_terminal_in_worktree(
        app: &mut App,
        source_branch: &str,
        target_branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<()> {
        // Find the agent that owns this worktree
        let owner_agent = app
            .storage
            .agents
            .iter()
            .find(|a| a.worktree_path == worktree_path && a.is_root())
            .map(|a| a.id);

        if let Some(root_id) = owner_agent {
            let root = app
                .storage
                .get(root_id)
                .ok_or_else(|| anyhow::anyhow!("Root agent not found"))?;

            let root_session = root.mux_session.clone();
            let branch = root.branch.clone();

            let title = format!("Merge Conflict: {source_branch} -> {target_branch}");

            // Reserve a window index
            let window_index = app.storage.reserve_window_indices(root_id);

            // Create child agent marked as terminal
            let mut terminal = Agent::new_child(
                title.clone(),
                "terminal".to_string(),
                branch,
                worktree_path.to_path_buf(),
                None,
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
            if let Some((width, height)) = app.ui.preview_dimensions {
                let window_target = SessionManager::window_target(&root_session, actual_index);
                let _ = session_manager.resize_window(&window_target, width, height);
            }

            // Update window index if it differs
            terminal.window_index = Some(actual_index);

            // Send the startup command
            let window_target = SessionManager::window_target(&root_session, actual_index);
            session_manager.send_keys_and_submit(&window_target, "git status")?;

            app.storage.add(terminal);

            // Expand the parent to show the new terminal
            if let Some(parent) = app.storage.get_mut(root_id) {
                parent.collapsed = false;
            }

            app.storage.save()?;

            app.set_status(format!("Opened terminal for conflict resolution: {title}"));
        } else {
            // No agent owns this worktree, just show a message
            app.set_status(format!(
                "Merge conflict in {target_branch}. Resolve in: {}",
                worktree_path.display()
            ));
        }

        app.clear_git_op_state();
        app.clear_review_state();
        app.exit_mode();

        Ok(())
    }

    /// Execute merge from main repo (when target branch has no worktree)
    fn execute_merge_in_main_repo(
        app: &mut App,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<()> {
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
            app.set_error(format!("Failed to checkout {target_branch}"));
            app.clear_git_op_state();
            app.clear_review_state();
            return Ok(());
        }

        // Attempt merge
        let merge_result = Self::git_merge(
            &repo_path,
            source_branch,
            target_branch,
            &original_branch,
            did_stash,
        );

        match merge_result {
            MergeResult::Success => {
                Self::restore_git_state(&repo_path, did_stash);
                info!(source = %source_branch, target = %target_branch, "Merge successful");
                app.show_success(format!("Merged {source_branch} into {target_branch}"));
            }
            MergeResult::Conflict => {
                // Stay on target branch, don't restore stash - user needs to resolve
                Self::spawn_conflict_terminal(
                    app,
                    &format!("Merge Conflict: {source_branch} -> {target_branch}"),
                    "git status",
                )?;
                return Ok(());
            }
            MergeResult::Failed(error_msg) => {
                Self::git_checkout(&repo_path, &original_branch)?;
                Self::restore_git_state(&repo_path, did_stash);
                app.set_error(format!("Merge failed: {error_msg}"));
            }
        }

        app.clear_git_op_state();
        app.clear_review_state();
        Ok(())
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
