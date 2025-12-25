//! Git push flow.

use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::app::state::App;

use super::super::Actions;

impl Actions {
    /// Push the selected agent's branch to remote (Ctrl+p)
    ///
    /// Shows a confirmation dialog, then pushes the branch.
    pub(crate) fn push_branch(app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();

        debug!(branch = %branch_name, "Starting push flow");

        app.start_push(agent_id, branch_name);
        Ok(())
    }

    /// Execute the git push operation (after user confirms)
    ///
    /// # Errors
    ///
    /// Returns an error if the push operation fails
    pub fn execute_push(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for push"))?;

        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch_name = app.git_op.branch_name.clone();

        debug!(branch = %branch_name, "Executing push");

        // Push to remote with upstream tracking
        let push_output = crate::git::git_command()
            .args(["push", "-u", "origin", &branch_name])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to push to remote")?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            app.set_error(format!("Push failed: {}", stderr.trim()));
            app.clear_git_op_state();
            return Ok(());
        }

        info!(branch = %branch_name, "Push successful");
        app.set_status(format!("Pushed branch: {branch_name}"));
        app.clear_git_op_state();
        app.exit_mode();

        Ok(())
    }
}

