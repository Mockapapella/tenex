//! Open PR flow (base branch detection, unpushed check, gh integration).

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::app::AppData;
use crate::state::{AppMode, ConfirmPushForPRMode, ErrorModalMode};

use super::super::Actions;

impl Actions {
    /// Open a PR for the selected agent's branch (Ctrl+o)
    ///
    /// Detects the base branch, checks for unpushed commits, and opens a PR.
    ///
    /// # Errors
    ///
    /// Returns an error if no agent is selected or PR creation fails.
    pub fn open_pr_flow(app_data: &mut AppData) -> Result<AppMode> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        // Detect base branch from git history (best-effort)
        let base_branch = Self::detect_base_branch(&worktree_path, &branch_name);

        // Check if there are unpushed commits
        let has_unpushed = Self::has_unpushed_commits(&worktree_path, &branch_name)?;

        debug!(
            branch = %branch_name,
            base_branch = %base_branch,
            has_unpushed,
            "Starting open PR flow"
        );

        app_data
            .git_op
            .start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        // If no unpushed commits, open PR immediately
        if has_unpushed {
            return Ok(ConfirmPushForPRMode.into());
        }

        if let Err(err) = Self::open_pr_in_browser(app_data) {
            return Ok(ErrorModalMode {
                message: format!("Failed to open PR: {err:#}"),
            }
            .into());
        }

        Ok(AppMode::normal())
    }

    /// Detect the base branch that this branch was created from
    pub(crate) fn detect_base_branch(worktree_path: &std::path::Path, branch_name: &str) -> String {
        // Prefer explicit "Created from <branch>" data in reflog when available.
        if let Ok(output) = crate::git::git_command()
            .args(["reflog", "show", "--no-abbrev", branch_name])
            .current_dir(worktree_path)
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(from_idx) = line.find("Created from ") {
                    let rest = &line[from_idx + 13..];
                    let base = rest.split_whitespace().next().unwrap_or("main");
                    return base.to_string();
                }
            }
        }

        // Fall back to the remote's default branch when available (e.g. origin/main).
        if let Ok(output) = crate::git::git_command()
            .args([
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ])
            .current_dir(worktree_path)
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(remote_ref) = stdout.lines().next()
                && let Some(base) = remote_ref.trim().split('/').next_back()
                && !base.is_empty()
            {
                return base.to_string();
            }
        }

        // Otherwise, use the first common default branch that exists locally or on origin.
        let candidates = ["main", "master", "develop"];
        for candidate in &candidates {
            let local_ref = format!("refs/heads/{candidate}");
            if crate::git::git_command()
                .args(["show-ref", "--verify", "--quiet", &local_ref])
                .current_dir(worktree_path)
                .status()
                .is_ok_and(|s| s.success())
            {
                return (*candidate).to_string();
            }

            let remote_ref = format!("refs/remotes/origin/{candidate}");
            if crate::git::git_command()
                .args(["show-ref", "--verify", "--quiet", &remote_ref])
                .current_dir(worktree_path)
                .status()
                .is_ok_and(|s| s.success())
            {
                return (*candidate).to_string();
            }
        }

        "main".to_string()
    }

    /// Check if there are unpushed commits on the branch
    pub(crate) fn has_unpushed_commits(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<bool> {
        // Check if remote tracking branch exists
        let remote_branch = format!("origin/{branch_name}");
        let output = crate::git::git_command()
            .args(["rev-parse", "--verify", &remote_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check remote branch")?;

        if !output.status.success() {
            // No remote branch means all commits are unpushed
            return Ok(true);
        }

        // Compare local and remote
        let output = crate::git::git_command()
            .args([
                "rev-list",
                "--count",
                &format!("{remote_branch}..{branch_name}"),
            ])
            .current_dir(worktree_path)
            .output()
            .context("Failed to count unpushed commits")?;

        let count: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(count > 0)
    }

    /// Execute push and then open PR (for Ctrl+o flow)
    ///
    /// # Errors
    ///
    /// Returns an error if the push or PR open fails
    pub fn execute_push_and_open_pr(app_data: &mut AppData) -> Result<AppMode> {
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

        debug!(branch = %branch_name, "Executing push before opening PR");

        // Push to remote
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

        info!(branch = %branch_name, "Push successful, opening PR");

        // Now open the PR
        if let Err(err) = Self::open_pr_in_browser(app_data) {
            return Ok(ErrorModalMode {
                message: format!("Failed to open PR: {err:#}"),
            }
            .into());
        }

        Ok(AppMode::normal())
    }

    /// Open PR in browser using gh CLI
    pub(crate) fn open_pr_in_browser(app_data: &mut AppData) -> Result<()> {
        let agent_id = app_data
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for PR"))?;

        let agent = app_data
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch = app_data.git_op.branch_name.clone();
        let base_branch = app_data.git_op.base_branch.clone();

        debug!(
            branch = %branch,
            base_branch = %base_branch,
            "Opening PR with gh CLI"
        );

        // Use gh pr create with --web flag to open in browser
        let output = std::process::Command::new("gh")
            .args(["pr", "create", "--web", "--base", &base_branch])
            .current_dir(&worktree_path)
            .output();

        match output {
            Ok(result) if result.status.success() => {
                info!(branch = %branch, base = %base_branch, "Opened PR creation page in browser");
                app_data.set_status(format!("Opening PR: {branch} → {base_branch}"));
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                warn!(error = %stderr, "gh pr create failed");
                app_data.git_op.clear();
                anyhow::bail!("{}", stderr.trim());
            }
            Err(e) => {
                warn!(error = %e, "gh CLI not found");
                app_data.git_op.clear();
                anyhow::bail!("gh CLI not found. Install it with: brew install gh");
            }
        }

        app_data.git_op.clear();
        Ok(())
    }
}
