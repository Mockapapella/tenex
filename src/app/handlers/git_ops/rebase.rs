//! Rebase flow (branch selector + rebase execution).

use crate::git;
use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode, RebaseBranchSelectorMode, SuccessModalMode};

use super::super::Actions;

impl Actions {
    /// Start the rebase flow - show branch selector (Ctrl+r)
    ///
    /// # Errors
    ///
    /// Returns an error if the git repository cannot be opened or branches cannot be listed.
    pub fn rebase_branch(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to rebase.".to_string(),
            }
            .into());
        };

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        debug!(branch = %current_branch, "Starting rebase flow");

        // Fetch branches for selector
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_rebase(agent_id, current_branch);
        app_data.review.start(branches);
        Ok(RebaseBranchSelectorMode.into())
    }

    /// Execute the rebase operation
    ///
    /// # Errors
    ///
    /// Returns an error if the rebase operation fails
    pub fn execute_rebase(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for rebase".to_string(),
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

        let worktree_path = agent.worktree_path.clone();
        let current_branch = app_data.git_op.branch_name.clone();
        let target_branch = app_data.git_op.target_branch.clone();

        debug!(
            current = %current_branch,
            target = %target_branch,
            "Executing rebase"
        );

        // Execute git rebase
        let output = crate::git::git_command()
            .args(["rebase", &target_branch])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to execute rebase")?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}{stderr}");

            // Check if there are merge conflicts (git may output to stdout or stderr)
            if combined.contains("CONFLICT") || combined.contains("could not apply") {
                info!(
                    current = %current_branch,
                    target = %target_branch,
                    "Rebase has conflicts - spawning terminal"
                );
                // Spawn terminal for conflict resolution
                return Self::spawn_conflict_terminal(app_data, "Rebase Conflict", "git status");
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
                message: format!("Rebase failed: {error_msg}"),
            }
            .into());
        }

        info!(
            current = %current_branch,
            target = %target_branch,
            "Rebase successful"
        );
        app_data.git_op.clear();
        app_data.review.clear();
        Ok(SuccessModalMode {
            message: format!("Rebased {current_branch} onto {target_branch}"),
        }
        .into())
    }
}
