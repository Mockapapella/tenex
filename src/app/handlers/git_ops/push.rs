//! Git push flow.

use anyhow::{Context, Result, bail};
use std::process::Output;
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::{AppMode, ConfirmPushMode, ErrorModalMode};

use super::super::Actions;

pub(super) struct ConfiguredUpstream {
    pub(super) remote: String,
    pub(super) merge_ref: String,
}

impl ConfiguredUpstream {
    fn refspec(&self, branch_name: &str) -> String {
        format!("{branch_name}:{}", self.merge_ref)
    }
}

fn branch_config_key(branch: &str, field: &str) -> String {
    format!("branch.{branch}.{field}")
}

pub(super) fn configured_upstream(
    worktree_path: &std::path::Path,
    branch_name: &str,
) -> Result<Option<ConfiguredUpstream>> {
    let remote = git_config_get(worktree_path, &branch_config_key(branch_name, "remote"))?;
    let merge_ref = git_config_get(worktree_path, &branch_config_key(branch_name, "merge"))?;

    match (remote, merge_ref) {
        (Some(remote), Some(merge_ref)) => Ok(Some(ConfiguredUpstream { remote, merge_ref })),
        (None, None) => Ok(None),
        _ => bail!("Incomplete upstream config for branch '{branch_name}'"),
    }
}

fn git_config_get(worktree_path: &std::path::Path, key: &str) -> Result<Option<String>> {
    let output = crate::git::git_command()
        .args(["config", "--get", key])
        .current_dir(worktree_path)
        .output()
        .with_context(|| format!("Failed to read git config key '{key}'"))?;

    if output.status.code() == Some(1) {
        return Ok(None);
    }

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to read git config key '{key}': {}", stderr.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let value = raw.trim_end_matches('\n').trim_end_matches('\r');
    if value.is_empty() {
        bail!("Git config key '{key}' is empty");
    }

    Ok(Some(value.to_string()))
}

fn command_args(worktree_path: &std::path::Path, branch_name: &str) -> Result<Vec<String>> {
    Ok(match configured_upstream(worktree_path, branch_name)? {
        Some(upstream) => {
            let refspec = upstream.refspec(branch_name);
            vec!["push".to_string(), upstream.remote, refspec]
        }
        None => vec![
            "push".to_string(),
            "-u".to_string(),
            "origin".to_string(),
            branch_name.to_string(),
        ],
    })
}

pub(super) fn run_push(worktree_path: &std::path::Path, branch_name: &str) -> Result<Output> {
    let args = command_args(worktree_path, branch_name).context("Failed to push to remote")?;
    crate::git::git_command()
        .args(args.iter().map(String::as_str))
        .current_dir(worktree_path)
        .output()
        .context("Failed to push to remote")
}

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

        let push_output = run_push(&worktree_path, &branch_name)?;

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
