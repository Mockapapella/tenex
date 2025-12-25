//! Rebase flow (branch selector + rebase execution).

use crate::app::state::App;
use crate::git;
use anyhow::{Context, Result};
use tracing::{debug, info};

use super::super::Actions;

impl Actions {
    /// Start the rebase flow - show branch selector (Ctrl+r)
    pub(crate) fn rebase_branch(app: &mut App) -> Result<()> {
        let Some(agent) = app.selected_agent() else {
            app.set_error("No agent selected. Select an agent first to rebase.");
            return Ok(());
        };

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        debug!(branch = %current_branch, "Starting rebase flow");

        // Fetch branches for selector
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app.start_rebase(agent_id, current_branch, branches);
        Ok(())
    }

    /// Execute the rebase operation
    ///
    /// # Errors
    ///
    /// Returns an error if the rebase operation fails
    pub fn execute_rebase(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for rebase"))?;

        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let current_branch = app.git_op.branch_name.clone();
        let target_branch = app.git_op.target_branch.clone();

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
                Self::spawn_conflict_terminal(app, "Rebase Conflict", "git status")?;
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
            app.set_error(format!("Rebase failed: {error_msg}"));
            app.clear_git_op_state();
            app.clear_review_state();
            return Ok(());
        }

        info!(
            current = %current_branch,
            target = %target_branch,
            "Rebase successful"
        );
        app.show_success(format!("Rebased {current_branch} onto {target_branch}"));
        app.clear_git_op_state();
        app.clear_review_state();

        Ok(())
    }
}

