//! Git push flow.

use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::{AppMode, ConfirmPushMode, ErrorModalMode};

use super::super::Actions;

impl Actions {
    /// Push the selected agent's branch to remote (Ctrl+p)
    ///
    /// Shows a confirmation dialog, then pushes the branch.
    ///
    /// # Errors
    ///
    /// Returns an error if no agent is selected.
    pub fn push_branch(app_data: &mut AppData) -> Result<AppMode> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();

        debug!(branch = %branch_name, "Starting push flow");

        app_data.git_op.start_push(agent_id, branch_name);
        Ok(ConfirmPushMode.into())
    }

    /// Execute the git push operation (after user confirms)
    ///
    /// # Errors
    ///
    /// Returns an error if the push operation fails
    pub fn execute_push(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for push".to_string(),
            }
            .into());
        };

        let Some(agent) = app_data.storage.get(agent_id) else {
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: "Agent not found".to_string(),
            }
            .into());
        };

        let worktree_path = agent.worktree_path.clone();
        let branch_name = app_data.git_op.branch_name.clone();

        debug!(branch = %branch_name, "Executing push");

        // Push to remote with upstream tracking
        let push_output = crate::git::git_command()
            .args(["push", "-u", "origin", &branch_name])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to push to remote")?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: format!("Push failed: {}", stderr.trim()),
            }
            .into());
        }

        info!(branch = %branch_name, "Push successful");
        app_data.set_status(format!("Pushed branch: {branch_name}"));
        app_data.git_op.clear();

        Ok(AppMode::normal())
    }
}
