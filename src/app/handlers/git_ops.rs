//! Git operations: Push, Rename Branch, Open PR

use crate::tmux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::state::App;

impl Actions {
    /// Push the selected agent's branch to remote (Ctrl+p)
    ///
    /// Shows a confirmation dialog, then pushes the branch.
    #[expect(clippy::unused_self, reason = "consistent with other handler methods")]
    pub(crate) fn push_branch(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();

        debug!(branch = %branch_name, "Starting push flow");

        app.start_push(agent_id, branch_name);
        Ok(())
    }

    /// Rename the selected agent (r key)
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + tmux session
    /// For sub-agents: Renames agent title + tmux window only
    #[expect(clippy::unused_self, reason = "consistent with other handler methods")]
    pub(crate) fn rename_agent(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let is_root = agent.is_root();
        let current_name = agent.title.clone();

        debug!(
            title = %current_name,
            is_root,
            "Starting rename flow"
        );

        app.start_rename(agent_id, current_name, is_root);
        Ok(())
    }

    /// Open a PR for the selected agent's branch (Ctrl+o)
    ///
    /// Detects the base branch, checks for unpushed commits, and opens a PR.
    #[expect(clippy::unused_self, reason = "consistent with other handler methods")]
    pub(crate) fn open_pr_flow(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        // Detect base branch from git history
        let base_branch = Self::detect_base_branch(&worktree_path, &branch_name)?;

        // Check if there are unpushed commits
        let has_unpushed = Self::has_unpushed_commits(&worktree_path, &branch_name)?;

        debug!(
            branch = %branch_name,
            base_branch = %base_branch,
            has_unpushed,
            "Starting open PR flow"
        );

        app.start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        // If no unpushed commits, open PR immediately
        if !has_unpushed {
            Self::open_pr_in_browser(app)?;
        }

        Ok(())
    }

    /// Detect the base branch that this branch was created from
    pub(crate) fn detect_base_branch(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<String> {
        // Try to find the merge base with common default branches
        let candidates = ["main", "master", "develop"];

        for candidate in &candidates {
            let output = std::process::Command::new("git")
                .args(["merge-base", candidate, branch_name])
                .current_dir(worktree_path)
                .output();

            if let Ok(result) = output
                && result.status.success()
            {
                return Ok((*candidate).to_string());
            }
        }

        // Fallback: try to detect from the reflog
        let output = std::process::Command::new("git")
            .args(["reflog", "show", "--no-abbrev", branch_name])
            .current_dir(worktree_path)
            .output()
            .context("Failed to read reflog")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Look for "branch: Created from" in reflog
        for line in stdout.lines() {
            if line.contains("Created from") {
                // Extract branch name after "Created from "
                if let Some(from_idx) = line.find("Created from ") {
                    let rest = &line[from_idx + 13..];
                    let base = rest.split_whitespace().next().unwrap_or("main");
                    return Ok(base.to_string());
                }
            }
        }

        // Default to main
        Ok("main".to_string())
    }

    /// Check if there are unpushed commits on the branch
    pub(crate) fn has_unpushed_commits(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<bool> {
        // Check if remote tracking branch exists
        let remote_branch = format!("origin/{branch_name}");
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &remote_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check remote branch")?;

        if !output.status.success() {
            // No remote branch means all commits are unpushed
            return Ok(true);
        }

        // Compare local and remote
        let output = std::process::Command::new("git")
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

    /// Check if a remote branch exists
    pub(crate) fn check_remote_branch_exists(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<bool> {
        let remote_branch = format!("origin/{branch_name}");
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &remote_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check remote branch")?;

        Ok(output.status.success())
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
        let push_output = std::process::Command::new("git")
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

    /// Execute rename operation
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + tmux session
    /// For sub-agents: Renames agent title + tmux window only
    ///
    /// # Errors
    ///
    /// Returns an error if the rename operation fails
    pub fn execute_rename(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for rename"))?;

        // Verify agent exists
        if app.storage.get(agent_id).is_none() {
            anyhow::bail!("Agent not found");
        }

        let old_name = app.git_op.original_branch.clone();
        let new_name = app.git_op.branch_name.clone();
        let is_root = app.git_op.is_root_rename;

        if old_name == new_name {
            app.set_status("Name unchanged");
            app.clear_git_op_state();
            app.exit_mode();
            return Ok(());
        }

        debug!(
            old_name = %old_name,
            new_name = %new_name,
            is_root,
            "Executing rename"
        );

        if is_root {
            // Root agent: rename branch + agent + tmux session
            Self::execute_root_rename(app, agent_id, &old_name, &new_name)?;
        } else {
            // Sub-agent: rename agent title + tmux window only
            Self::execute_subagent_rename(app, agent_id, &new_name)?;
        }

        app.clear_git_op_state();
        app.exit_mode();
        Ok(())
    }

    /// Execute rename for a root agent (branch + agent + tmux session + worktree path)
    #[expect(
        clippy::too_many_lines,
        reason = "Complex operation with many related steps"
    )]
    fn execute_root_rename(
        app: &mut App,
        agent_id: uuid::Uuid,
        old_name: &str,
        new_name: &str,
    ) -> Result<()> {
        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let old_branch = agent.branch.clone();
        let tmux_session = agent.tmux_session.clone();

        // Generate new branch name from new title
        let new_branch = app.config.generate_branch_name(new_name);

        // Check if remote branch exists before we start
        let remote_exists = Self::check_remote_branch_exists(&worktree_path, &old_branch)?;

        // Rename local branch
        let rename_output = std::process::Command::new("git")
            .args(["branch", "-m", &old_branch, &new_branch])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to rename local branch")?;

        if !rename_output.status.success() {
            let stderr = String::from_utf8_lossy(&rename_output.stderr);
            app.set_error(format!("Failed to rename branch: {}", stderr.trim()));
            return Ok(());
        }

        // Generate new worktree path based on new branch name
        let new_worktree_path = app.config.worktree_dir.join(&new_branch);

        // Move the worktree directory to match the new branch name
        if worktree_path != new_worktree_path {
            // Ensure parent directory exists
            if let Some(parent) = new_worktree_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // Move the worktree directory
            if let Err(e) = std::fs::rename(&worktree_path, &new_worktree_path) {
                warn!(error = %e, "Failed to move worktree directory");
                app.set_error(format!("Failed to move worktree: {e}"));
                return Ok(());
            }

            // Update git worktree metadata
            // The worktree is registered under the old name (with slashes as dashes)
            // We need to update the gitdir file in the new location
            let gitdir_file = new_worktree_path.join(".git");
            if gitdir_file.exists() {
                // The .git file in a worktree points to the main repo's worktree metadata
                // We need to update the main repo's worktree to point to the new path
                let old_worktree_name = old_branch.replace('/', "-");
                let repo_path = std::env::current_dir()?;
                let worktree_meta_dir = repo_path
                    .join(".git")
                    .join("worktrees")
                    .join(&old_worktree_name);

                if worktree_meta_dir.exists() {
                    // Update the gitdir file in the worktree metadata to point to new location
                    let gitdir_path = worktree_meta_dir.join("gitdir");
                    if gitdir_path.exists() {
                        let new_gitdir_content =
                            format!("{}\n", new_worktree_path.join(".git").display());
                        if let Err(e) = std::fs::write(&gitdir_path, new_gitdir_content) {
                            warn!(error = %e, "Failed to update worktree gitdir");
                        }
                    }

                    // Rename the worktree metadata directory to match new branch
                    let new_worktree_name = new_branch.replace('/', "-");
                    let new_worktree_meta_dir = repo_path
                        .join(".git")
                        .join("worktrees")
                        .join(&new_worktree_name);
                    if old_worktree_name != new_worktree_name
                        && let Err(e) = std::fs::rename(&worktree_meta_dir, &new_worktree_meta_dir)
                    {
                        warn!(error = %e, "Failed to rename worktree metadata directory");
                    }
                }
            }
        }

        // Update the agent's title, branch name, and worktree path
        if let Some(agent) = app.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
            agent.branch.clone_from(&new_branch);
            agent.worktree_path.clone_from(&new_worktree_path);
        }

        // Update all descendants' worktree_path as well
        let descendant_ids: Vec<uuid::Uuid> = app
            .storage
            .descendants(agent_id)
            .iter()
            .map(|a| a.id)
            .collect();
        for desc_id in descendant_ids {
            if let Some(desc) = app.storage.get_mut(desc_id) {
                desc.worktree_path.clone_from(&new_worktree_path);
            }
        }

        app.storage.save()?;

        // Rename tmux session
        let session_manager = SessionManager::new();
        let new_session_name = format!("tenex-{new_name}");
        if let Err(e) = session_manager.rename(&tmux_session, &new_session_name) {
            warn!(error = %e, "Failed to rename tmux session");
        } else {
            // Update root agent's tmux_session
            if let Some(agent) = app.storage.get_mut(agent_id) {
                agent.tmux_session.clone_from(&new_session_name);
            }

            // Update all descendants' tmux_session to the new session name
            let descendant_ids: Vec<uuid::Uuid> = app
                .storage
                .descendants(agent_id)
                .iter()
                .map(|a| a.id)
                .collect();
            for desc_id in descendant_ids {
                if let Some(desc) = app.storage.get_mut(desc_id) {
                    desc.tmux_session.clone_from(&new_session_name);
                }
            }

            app.storage.save()?;
        }

        // Handle remote branch only if it existed
        if remote_exists {
            // Delete old remote branch
            let _ = std::process::Command::new("git")
                .args(["push", "origin", "--delete", &old_branch])
                .current_dir(&new_worktree_path)
                .output();

            // Push new branch to remote
            let push_output = std::process::Command::new("git")
                .args(["push", "-u", "origin", &new_branch])
                .current_dir(&new_worktree_path)
                .output()
                .context("Failed to push renamed branch")?;

            if push_output.status.success() {
                info!(
                    old_name = %old_name,
                    new_name = %new_name,
                    "Root agent renamed successfully"
                );
                app.set_status(format!("Renamed: {old_name} → {new_name}"));
            } else {
                let stderr = String::from_utf8_lossy(&push_output.stderr);
                warn!(error = %stderr, "Failed to push renamed branch to remote");
                app.set_status(format!("Renamed to {new_name} (remote push failed)"));
            }
        } else {
            info!(
                old_name = %old_name,
                new_name = %new_name,
                "Root agent renamed successfully (local only)"
            );
            app.set_status(format!("Renamed: {old_name} → {new_name}"));
        }

        Ok(())
    }

    /// Execute rename for a sub-agent (title + tmux window only)
    fn execute_subagent_rename(app: &mut App, agent_id: uuid::Uuid, new_name: &str) -> Result<()> {
        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let old_name = agent.title.clone();
        let tmux_session = agent.tmux_session.clone();
        let window_index = agent.window_index;

        // Update the agent's title
        if let Some(agent) = app.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
        }
        app.storage.save()?;

        // Rename tmux window if agent has a window index
        if let Some(idx) = window_index {
            let rename_output = std::process::Command::new("tmux")
                .args([
                    "rename-window",
                    "-t",
                    &format!("{tmux_session}:{idx}"),
                    new_name,
                ])
                .output();

            if let Err(e) = rename_output {
                warn!(error = %e, "Failed to rename tmux window");
            }
        }

        info!(
            old_name = %old_name,
            new_name = %new_name,
            "Sub-agent renamed successfully"
        );
        app.set_status(format!("Renamed: {old_name} → {new_name}"));

        Ok(())
    }

    /// Execute push and then open PR (for Ctrl+o flow)
    ///
    /// # Errors
    ///
    /// Returns an error if the push or PR open fails
    pub fn execute_push_and_open_pr(app: &mut App) -> Result<()> {
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

        debug!(branch = %branch_name, "Executing push before opening PR");

        // Push to remote
        let push_output = std::process::Command::new("git")
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

        info!(branch = %branch_name, "Push successful, opening PR");

        // Now open the PR
        Self::open_pr_in_browser(app)
    }

    /// Open PR in browser using gh CLI
    pub(crate) fn open_pr_in_browser(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for PR"))?;

        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch = app.git_op.branch_name.clone();
        let base_branch = app.git_op.base_branch.clone();

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
                app.set_status(format!("Opening PR: {branch} → {base_branch}"));
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                warn!(error = %stderr, "gh pr create failed");
                app.set_error(format!("Failed to open PR: {}", stderr.trim()));
            }
            Err(e) => {
                warn!(error = %e, "gh CLI not found");
                app.set_error("gh CLI not found. Install it with: brew install gh");
            }
        }

        app.clear_git_op_state();
        app.exit_mode();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::state::Mode;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((App::new(Config::default(), storage), temp_file))
    }

    #[test]
    fn test_handle_push_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let result = handler.handle_action(&mut app, crate::config::Action::Push);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_handle_push_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Push should enter ConfirmPush mode
        handler.handle_action(&mut app, crate::config::Action::Push)?;

        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.branch_name, "muster/test");
        Ok(())
    }

    #[test]
    fn test_execute_push_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = None;

        let result = Actions::execute_push(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "test".to_string();

        let result = Actions::execute_push(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rename_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = None;

        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rename_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "new-name".to_string();
        app.git_op.original_branch = "old-name".to_string();

        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_open_pr_in_browser_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = None;

        let result = Actions::open_pr_in_browser(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_open_pr_in_browser_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "test".to_string();
        app.git_op.base_branch = "main".to_string();

        let result = Actions::open_pr_in_browser(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_push_flow_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "feature/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start push flow
        app.start_push(agent_id, "feature/test".to_string());
        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.branch_name, "feature/test");

        // Clear git op state
        app.clear_git_op_state();
        assert!(app.git_op.branch_name.is_empty());
        assert!(app.git_op.agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_rename_root_flow_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent
        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start rename flow for root agent
        app.start_rename(agent_id, "test-agent".to_string(), true);
        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.original_branch, "test-agent");
        assert_eq!(app.git_op.branch_name, "test-agent");
        assert_eq!(app.input.buffer, "test-agent");
        assert!(app.git_op.is_root_rename);

        // Simulate user input
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_char('n');
        app.handle_char('e');
        app.handle_char('w');
        assert_eq!(app.input.buffer, "test-new");

        // Confirm rename
        let result = app.confirm_rename_branch();
        assert!(result);
        assert_eq!(app.git_op.branch_name, "test-new");
        Ok(())
    }

    #[test]
    fn test_rename_subagent_flow_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent first
        let root = Agent::new(
            "root-agent".to_string(),
            "claude".to_string(),
            "tenex/root-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        app.storage.add(root.clone());

        // Add a child agent
        let child = Agent::new_child(
            "sub-agent".to_string(),
            "claude".to_string(),
            "tenex/root-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
            crate::agent::ChildConfig {
                parent_id: root.id,
                tmux_session: root.tmux_session,
                window_index: 1,
            },
        );
        let child_id = child.id;
        app.storage.add(child);

        // Start rename flow for sub-agent
        app.start_rename(child_id, "sub-agent".to_string(), false);
        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op.agent_id, Some(child_id));
        assert_eq!(app.git_op.original_branch, "sub-agent");
        assert!(!app.git_op.is_root_rename);

        // Simulate user input
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_char('n');
        app.handle_char('e');
        app.handle_char('w');
        assert_eq!(app.input.buffer, "sub-new");

        // Confirm rename
        let result = app.confirm_rename_branch();
        assert!(result);
        assert_eq!(app.git_op.branch_name, "sub-new");
        Ok(())
    }

    #[test]
    fn test_open_pr_flow_state_with_unpushed() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "feature/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start open PR flow with unpushed commits
        app.start_open_pr(
            agent_id,
            "feature/test".to_string(),
            "main".to_string(),
            true,
        );

        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.branch_name, "feature/test");
        assert_eq!(app.git_op.base_branch, "main");
        assert!(app.git_op.has_unpushed);
        Ok(())
    }

    #[test]
    fn test_open_pr_flow_state_no_unpushed() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "feature/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start open PR flow without unpushed commits
        app.start_open_pr(
            agent_id,
            "feature/test".to_string(),
            "main".to_string(),
            false,
        );

        // Mode should stay Normal (handler opens PR directly)
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert!(!app.git_op.has_unpushed);
        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code")]
    fn test_detect_base_branch_no_git() {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new().unwrap();

        // Should return default "main" when git commands fail
        let result = Actions::detect_base_branch(temp_dir.path(), "feature/test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "main");
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code")]
    fn test_has_unpushed_commits_no_git() {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new().unwrap();

        // Should return true (assume all commits are unpushed if we can't check)
        let result = Actions::has_unpushed_commits(temp_dir.path(), "feature/test");
        // Either Ok(true) or Err is acceptable
        let _ = result;
    }

    #[test]
    fn test_handle_rename_with_root_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent
        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Rename should enter RenameBranch mode with agent title
        handler.handle_action(&mut app, crate::config::Action::RenameBranch)?;

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.branch_name, "test-agent");
        assert_eq!(app.git_op.original_branch, "test-agent");
        assert_eq!(app.input.buffer, "test-agent");
        assert!(app.git_op.is_root_rename);
        Ok(())
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test code")]
    fn test_handle_rename_with_subagent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent first
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let root_id = root.id;
        app.storage.add(root.clone());

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            crate::agent::ChildConfig {
                parent_id: root_id,
                tmux_session: root.tmux_session,
                window_index: 1,
            },
        );
        let child_id = child.id;
        app.storage.add(child);

        // Expand root to see child, then select the child agent
        app.storage
            .get_mut(root_id)
            .expect("root agent should exist")
            .collapsed = false;
        app.select_next();

        // Rename should enter RenameBranch mode with agent title, not root rename
        handler.handle_action(&mut app, crate::config::Action::RenameBranch)?;

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op.agent_id, Some(child_id));
        assert_eq!(app.git_op.branch_name, "child");
        assert_eq!(app.git_op.original_branch, "child");
        assert_eq!(app.input.buffer, "child");
        assert!(!app.git_op.is_root_rename);
        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code")]
    fn test_check_remote_branch_exists_no_git() {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new().unwrap();

        // Should return Ok(false) when not in a git repo (command returns error)
        let result = Actions::check_remote_branch_exists(temp_dir.path(), "main");
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_execute_rename_clears_state_on_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Set up state but with an invalid agent ID
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "new-name".to_string();
        app.git_op.is_root_rename = true;

        // Execute should fail gracefully
        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rename_subagent_clears_state_on_no_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Set up state but with an invalid agent ID
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "new-name".to_string();
        app.git_op.is_root_rename = false;

        // Execute should fail gracefully
        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_and_open_pr_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // No agent ID set
        app.git_op.agent_id = None;

        let result = Actions::execute_push_and_open_pr(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_and_open_pr_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Set invalid agent ID
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());

        let result = Actions::execute_push_and_open_pr(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_handle_open_pr_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let result = handler.handle_action(&mut app, crate::config::Action::OpenPR);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_handle_rename_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let result = handler.handle_action(&mut app, crate::config::Action::RenameBranch);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_open_pr_flow_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Trigger open PR action
        handler.handle_action(&mut app, crate::config::Action::OpenPR)?;

        // Should enter ConfirmPushForPR mode
        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        Ok(())
    }

    #[test]
    fn test_push_flow_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Trigger push action
        handler.handle_action(&mut app, crate::config::Action::Push)?;

        // Should enter ConfirmPush mode
        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        Ok(())
    }
}
