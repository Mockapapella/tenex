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

pub(super) fn git_failure_message(stdout: &str, stderr: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    "Unknown error".to_string()
}

fn output_indicates_merge_conflict(combined_output: &str) -> bool {
    let has_conflict_marker = combined_output.contains("CONFLICT");
    let has_automatic_failure = combined_output.contains("Automatic merge failed");
    has_conflict_marker || has_automatic_failure
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

        // Fetch branches for selector from the selected agent's repository.
        let repo_path = agent
            .repo_root
            .clone()
            .or_else(|| git::repository_workspace_root(&agent.worktree_path).ok())
            .unwrap_or_else(|| agent.worktree_path.clone());
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

        let Some(agent) = app_data.storage.get(agent_id) else {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "Agent not found".to_string(),
            }
            .into());
        };
        let repo_path = agent
            .repo_root
            .clone()
            .or_else(|| git::repository_workspace_root(&agent.worktree_path).ok())
            .unwrap_or_else(|| agent.worktree_path.clone());

        let source_branch = app_data.git_op.branch_name.clone(); // Agent's branch (e.g., tenex/feature)
        let target_branch = app_data.git_op.target_branch.clone(); // Branch to merge into (e.g., master)

        debug!(
            source = %source_branch,
            target = %target_branch,
            "Executing merge: {source_branch} -> {target_branch}"
        );

        // Check if target branch has a worktree
        if let Some(worktree_path) = Self::find_worktree_for_branch(&repo_path, &target_branch)? {
            Self::execute_merge_in_worktree(
                app_data,
                &source_branch,
                &target_branch,
                &worktree_path,
            )
        } else {
            Self::execute_merge_in_main_repo(app_data, &repo_path, &source_branch, &target_branch)
        }
    }

    /// Find the worktree path for a branch, if one exists
    fn find_worktree_for_branch(
        repo_path: &std::path::Path,
        branch: &str,
    ) -> Result<Option<std::path::PathBuf>> {
        let output = crate::git::git_command()
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
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
        debug!(source = %source_branch, target = %target_branch, worktree = %worktree_path.display(), "Merging in worktree");

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
            if output_indicates_merge_conflict(&combined) {
                info!(source = %source_branch, target = %target_branch, "Merge has conflicts - spawning terminal");

                return Self::spawn_merge_conflict_terminal_in_worktree(
                    app_data,
                    source_branch,
                    target_branch,
                    worktree_path,
                );
            }

            // Show error with both stdout and stderr for context
            let error_msg = git_failure_message(&stdout, &stderr);
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
        let worktree_path = worktree_path
            .canonicalize()
            .unwrap_or_else(|_| worktree_path.to_path_buf());
        let worktree_path_display = worktree_path.display().to_string();

        // Find the agent that owns this worktree
        let root_snapshot = app_data.storage.agents.iter().find_map(|agent| {
            let agent_worktree = agent
                .worktree_path
                .canonicalize()
                .unwrap_or_else(|_| agent.worktree_path.clone());
            if agent_worktree != worktree_path || !agent.is_root() {
                return None;
            }

            Some((
                agent.id,
                agent.mux_session.clone(),
                agent.branch.clone(),
                agent.repo_root.clone(),
                agent.runtime,
                agent.effective_runtime_scope().to_string(),
            ))
        });

        if let Some((root_id, root_session, branch, repo_root, runtime, runtime_scope)) =
            root_snapshot
        {
            let title = format!("Merge Conflict: {source_branch} -> {target_branch}");

            // Reserve a window index
            let window_index = app_data.storage.reserve_window_indices(root_id);

            // Create child agent marked as terminal
            let mut terminal = Agent::new_child(
                title.clone(),
                "terminal".to_string(),
                branch,
                worktree_path,
                ChildConfig {
                    parent_id: root_id,
                    mux_session: root_session.clone(),
                    window_index,
                    repo_root,
                },
            );
            terminal.is_terminal = true;
            terminal.runtime = runtime;
            terminal.runtime_scope = runtime_scope;

            // Create session manager and window
            let session_manager = SessionManager::new();
            crate::runtime::ensure_runtime_ready(&terminal, &app_data.settings)?;
            let terminal_command = crate::runtime::build_terminal_command(
                &terminal,
                Some("git status"),
                &app_data.settings,
            );
            let actual_index = session_manager.create_window(
                &root_session,
                &title,
                terminal.worktree_path.as_path(),
                terminal_command.as_deref(),
            )?;

            // Resize the new window to match preview dimensions
            if let Some((width, height)) = app_data.ui.preview_dimensions {
                let window_target = SessionManager::window_target(&root_session, actual_index);
                let _ = session_manager.resize_window(&window_target, width, height);
            }

            // Update window index if it differs
            terminal.window_index = Some(actual_index);

            let window_target = SessionManager::window_target(&root_session, actual_index);
            if terminal_command.is_none() {
                let _ = session_manager.send_keys_and_submit(&window_target, "git status");
            }

            app_data.storage.add(terminal);

            // Expand the parent to show the new terminal
            let _ = app_data.storage.set_collapsed(root_id, false);

            app_data.storage.save()?;

            app_data.set_status(format!("Opened terminal for conflict resolution: {title}"));
        } else {
            // No agent owns this worktree, just show a message
            app_data.set_status(format!(
                "Merge conflict in {target_branch}. Resolve in: {worktree_path_display}"
            ));
        }

        app_data.git_op.clear();
        app_data.review.clear();

        Ok(AppMode::normal())
    }

    /// Execute merge from main repo (when target branch has no worktree)
    fn execute_merge_in_main_repo(
        app_data: &mut AppData,
        repo_path: &std::path::Path,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<AppMode> {
        debug!(source = %source_branch, target = %target_branch, "Merging in main repo");

        // Prepare: stash changes and get current branch
        let did_stash = Self::git_stash_push(repo_path)?;
        let original_branch = Self::git_get_current_branch(repo_path)?;

        // Checkout target branch
        if !Self::git_checkout(repo_path, target_branch)? {
            Self::restore_git_state(repo_path, did_stash);
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: format!("Failed to checkout {target_branch}"),
            }
            .into());
        }

        // Attempt merge
        let merge_result = Self::git_merge(
            repo_path,
            source_branch,
            target_branch,
            &original_branch,
            did_stash,
        );

        let next = match merge_result {
            MergeResult::Success => {
                Self::restore_git_state(repo_path, did_stash);
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
                Self::git_checkout(repo_path, &original_branch)?;
                Self::restore_git_state(repo_path, did_stash);
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

        if output_indicates_merge_conflict(&combined) {
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

            let error_msg = git_failure_message(&stdout, &stderr);
            MergeResult::Failed(error_msg)
        }
    }
}
