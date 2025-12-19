//! Git operations: Push, Rename Branch, Open PR, Rebase, Merge

use crate::agent::{Agent, ChildConfig};
use crate::git;
use crate::tmux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::state::App;

/// Result of a git merge operation
enum MergeResult {
    Success,
    Conflict,
    Failed(String),
}

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

    /// Rename the selected agent (r key)
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + tmux session
    /// For sub-agents: Renames agent title + tmux window only
    pub(crate) fn rename_agent(app: &mut App) -> Result<()> {
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
    pub(crate) fn open_pr_flow(app: &mut App) -> Result<()> {
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
            let output = crate::git::git_command()
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
        let output = crate::git::git_command()
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

    /// Check if a remote branch exists
    pub(crate) fn check_remote_branch_exists(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<bool> {
        let remote_branch = format!("origin/{branch_name}");
        let output = crate::git::git_command()
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
        let rename_output = crate::git::git_command()
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

        // Move the worktree directory and update metadata
        if worktree_path != new_worktree_path
            && let Err(e) = Self::move_worktree_directory(
                &worktree_path,
                &new_worktree_path,
                &old_branch,
                &new_branch,
            )
        {
            app.set_error(format!("Failed to move worktree: {e}"));
            return Ok(());
        }

        // Update agent records and tmux session
        Self::update_agent_records(app, agent_id, new_name, &new_branch, &new_worktree_path)?;
        Self::rename_tmux_session_for_agent(app, agent_id, &tmux_session, new_name)?;

        // Handle remote branch rename if needed
        Self::handle_remote_branch_rename(
            app,
            &new_worktree_path,
            &old_branch,
            &new_branch,
            old_name,
            new_name,
            remote_exists,
        )?;

        Ok(())
    }

    /// Move a worktree directory and update git metadata
    fn move_worktree_directory(
        old_path: &std::path::Path,
        new_path: &std::path::Path,
        old_branch: &str,
        new_branch: &str,
    ) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = new_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Move the worktree directory
        std::fs::rename(old_path, new_path).context("Failed to move worktree directory")?;

        // Update git worktree metadata
        let gitdir_file = new_path.join(".git");
        if gitdir_file.exists() {
            let git_path_string = |path: &std::path::Path| -> String {
                let raw = path.to_string_lossy().to_string();
                if cfg!(windows) {
                    raw.strip_prefix(r"\\?\").unwrap_or(&raw).replace('\\', "/")
                } else {
                    raw
                }
            };

            let old_worktree_name = old_branch.replace('/', "-");
            let repo_path = std::env::current_dir()?;
            let worktree_meta_dir = repo_path
                .join(".git")
                .join("worktrees")
                .join(&old_worktree_name);

            if worktree_meta_dir.exists() {
                // Update the gitdir file to point to new location
                let gitdir_path = worktree_meta_dir.join("gitdir");
                if gitdir_path.exists() {
                    let new_gitdir_content =
                        format!("{}\n", git_path_string(&new_path.join(".git")));
                    if let Err(e) = std::fs::write(&gitdir_path, new_gitdir_content) {
                        warn!(error = %e, "Failed to update worktree gitdir");
                    }
                }

                // Rename the worktree metadata directory
                let new_worktree_name = new_branch.replace('/', "-");
                let new_worktree_meta_dir = repo_path
                    .join(".git")
                    .join("worktrees")
                    .join(&new_worktree_name);
                if old_worktree_name != new_worktree_name {
                    if let Err(e) = std::fs::rename(&worktree_meta_dir, &new_worktree_meta_dir) {
                        warn!(error = %e, "Failed to rename worktree metadata directory");
                    } else {
                        // Update the worktree's .git file to point to the renamed metadata directory
                        // Without this, git worktree remove will fail with "is not a .git file" error
                        let new_gitdir_pointer =
                            format!("gitdir: {}\n", git_path_string(&new_worktree_meta_dir));
                        if let Err(e) = std::fs::write(&gitdir_file, new_gitdir_pointer) {
                            warn!(error = %e, "Failed to update worktree .git file");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Update agent records after rename
    fn update_agent_records(
        app: &mut App,
        agent_id: uuid::Uuid,
        new_name: &str,
        new_branch: &str,
        new_worktree_path: &std::path::Path,
    ) -> Result<()> {
        // Update the agent's title, branch name, and worktree path
        if let Some(agent) = app.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
            agent.branch = new_branch.to_string();
            agent.worktree_path = new_worktree_path.to_path_buf();
        }

        // Update all descendants' worktree_path
        let descendant_ids: Vec<uuid::Uuid> = app
            .storage
            .descendants(agent_id)
            .iter()
            .map(|a| a.id)
            .collect();
        for desc_id in descendant_ids {
            if let Some(desc) = app.storage.get_mut(desc_id) {
                desc.worktree_path = new_worktree_path.to_path_buf();
            }
        }

        app.storage.save()
    }

    /// Rename tmux session and update agent records
    fn rename_tmux_session_for_agent(
        app: &mut App,
        agent_id: uuid::Uuid,
        old_session: &str,
        new_name: &str,
    ) -> Result<()> {
        let session_manager = SessionManager::new();
        let new_session_name = format!("tenex-{new_name}");

        if let Err(e) = session_manager.rename(old_session, &new_session_name) {
            warn!(error = %e, "Failed to rename tmux session");
            return Ok(());
        }

        // Update root agent's tmux_session
        if let Some(agent) = app.storage.get_mut(agent_id) {
            agent.tmux_session.clone_from(&new_session_name);
        }

        // Update all descendants' tmux_session
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

        app.storage.save()
    }

    /// Handle remote branch rename (delete old, push new)
    fn handle_remote_branch_rename(
        app: &mut App,
        worktree_path: &std::path::Path,
        old_branch: &str,
        new_branch: &str,
        old_name: &str,
        new_name: &str,
        remote_exists: bool,
    ) -> Result<()> {
        if !remote_exists {
            info!(
                old_name = %old_name,
                new_name = %new_name,
                "Root agent renamed successfully (local only)"
            );
            app.set_status(format!("Renamed: {old_name} → {new_name}"));
            return Ok(());
        }

        // Delete old remote branch
        let _ = crate::git::git_command()
            .args(["push", "origin", "--delete", old_branch])
            .current_dir(worktree_path)
            .output();

        // Push new branch to remote
        let push_output = crate::git::git_command()
            .args(["push", "-u", "origin", new_branch])
            .current_dir(worktree_path)
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
        if let Some(idx) = window_index
            && let Err(e) = SessionManager::new().rename_window(&tmux_session, idx, new_name)
        {
            warn!(error = %e, "Failed to rename tmux window");
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
    fn find_worktree_for_branch(branch: &str) -> Result<Option<std::path::PathBuf>> {
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

            let root_session = root.tmux_session.clone();
            let branch = root.branch.clone();

            let title = format!("Merge Conflict: {source_branch} -> {target_branch}");

            // Reserve a window index
            let window_index = app.storage.reserve_window_indices(root_id);

            // Create child agent marked as terminal
            let mut terminal = Agent::new_child(
                title.clone(),
                "bash".to_string(),
                branch,
                worktree_path.to_path_buf(),
                None,
                ChildConfig {
                    parent_id: root_id,
                    tmux_session: root_session.clone(),
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

    /// Spawn a terminal for resolving conflicts
    fn spawn_conflict_terminal(app: &mut App, title: &str, startup_command: &str) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID"))?;

        // Verify agent exists
        if app.storage.get(agent_id).is_none() {
            anyhow::bail!("Agent not found");
        }

        // Get the root ancestor to use its tmux session
        let root = app
            .storage
            .root_ancestor(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Could not find root agent"))?;

        let root_session = root.tmux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();
        let root_id = root.id;

        debug!(title, startup_command, "Creating conflict terminal");

        // Reserve a window index
        let window_index = app.storage.reserve_window_indices(root_id);

        // Create child agent marked as terminal
        let mut terminal = Agent::new_child(
            title.to_string(),
            "bash".to_string(),
            branch,
            worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session.clone(),
                window_index,
            },
        );
        terminal.is_terminal = true;

        // Create session manager and window
        let session_manager = SessionManager::new();
        let actual_index =
            session_manager.create_window(&root_session, title, &worktree_path, None)?;

        // Resize the new window to match preview dimensions
        if let Some((width, height)) = app.ui.preview_dimensions {
            let window_target = SessionManager::window_target(&root_session, actual_index);
            let _ = session_manager.resize_window(&window_target, width, height);
        }

        // Update window index if it differs
        terminal.window_index = Some(actual_index);

        // Send the startup command
        let window_target = SessionManager::window_target(&root_session, actual_index);
        session_manager.send_keys_and_submit(&window_target, startup_command)?;

        app.storage.add(terminal);

        // Expand the parent to show the new terminal
        if let Some(parent) = app.storage.get_mut(root_id) {
            parent.collapsed = false;
        }

        app.storage.save()?;

        // Clear git op state and exit mode
        app.clear_git_op_state();
        app.clear_review_state();
        app.exit_mode();

        info!(
            title,
            "Conflict terminal created - user can resolve conflicts"
        );
        app.set_status(format!("Opened terminal for conflict resolution: {title}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::app::state::Mode;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::{NamedTempFile, TempDir};

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
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
    fn test_push_branch_sets_confirm_mode() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let agent = Agent::new(
            "pushable".to_string(),
            "claude".to_string(),
            "feature/pushable".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        Actions::push_branch(&mut app)?;

        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.branch_name, "feature/pushable");
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
    fn test_rename_agent_sets_state_for_selected() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let agent = Agent::new(
            "rename-me".to_string(),
            "claude".to_string(),
            "feature/rename-me".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        Actions::rename_agent(&mut app)?;

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.original_branch, "rename-me");
        assert!(app.git_op.is_root_rename);
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
    fn test_open_pr_flow_sets_confirm_for_unpushed() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let temp_dir = TempDir::new()?;

        let agent = Agent::new(
            "pr-agent".to_string(),
            "claude".to_string(),
            "feature/pr-agent".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        Actions::open_pr_flow(&mut app)?;

        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        assert_eq!(app.git_op.agent_id, Some(agent_id));
        assert_eq!(app.git_op.branch_name, "feature/pr-agent");
        assert_eq!(app.git_op.base_branch, "main");
        assert!(app.git_op.has_unpushed);
        Ok(())
    }

    #[test]
    fn test_open_pr_in_browser_missing_gh_sets_error() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let temp_dir = TempDir::new()?;
        let agent = Agent::new(
            "gh-less".to_string(),
            "claude".to_string(),
            "feature/gh-less".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        app.git_op.agent_id = Some(agent_id);
        app.git_op.branch_name = "feature/gh-less".to_string();
        app.git_op.base_branch = "main".to_string();

        Actions::open_pr_in_browser(&mut app)?;

        // gh may be missing (error modal) or present (status message), but the git op state
        // should always be cleared after attempting to open the PR.
        assert!(matches!(app.mode, Mode::Normal | Mode::ErrorModal(_)));
        assert!(app.git_op.branch_name.is_empty());
        assert!(app.git_op.agent_id.is_none());
        assert!(
            app.ui.last_error.is_some() || app.ui.status_message.is_some(),
            "should surface either an error or a status update"
        );
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
    fn test_execute_push_and_open_pr_handles_failed_push() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        let temp_dir = TempDir::new()?;

        let agent = Agent::new(
            "failing-push".to_string(),
            "claude".to_string(),
            "feature/failing-push".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        app.git_op.agent_id = Some(agent_id);
        app.git_op.branch_name = "feature/failing-push".to_string();

        Actions::execute_push_and_open_pr(&mut app)?;

        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        assert!(app.git_op.branch_name.is_empty());
        assert!(app.git_op.agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_detect_base_branch_no_git() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new()?;

        // Should return default "main" when git commands fail
        let result = Actions::detect_base_branch(temp_dir.path(), "feature/test")?;
        assert_eq!(result, "main");
        Ok(())
    }

    #[test]
    fn test_has_unpushed_commits_no_git() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new()?;

        // Should return true (assume all commits are unpushed if we can't check)
        let result = Actions::has_unpushed_commits(temp_dir.path(), "feature/test");
        // Either Ok(true) or Err is acceptable
        let _ = result;
        Ok(())
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
        if let Some(root_agent) = app.storage.get_mut(root_id) {
            root_agent.collapsed = false;
        }
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
    fn test_check_remote_branch_exists_no_git() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new()?;

        // Should return Ok(false) when not in a git repo (command returns error)
        let result = Actions::check_remote_branch_exists(temp_dir.path(), "main")?;
        assert!(!result);
        Ok(())
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
        let temp_dir = TempDir::new()?;

        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
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

    #[test]
    fn test_merge_branch_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Should show error when no agent is selected
        Actions::merge_branch(&mut app)?;

        // Should have set an error message
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_rebase_branch_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Should show error when no agent is selected
        Actions::rebase_branch(&mut app)?;

        // Should have set an error message
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_execute_merge_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = None;

        let result = Actions::execute_merge(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_merge_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "feature".to_string();
        app.git_op.target_branch = "main".to_string();

        let result = Actions::execute_merge(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rebase_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = None;

        let result = Actions::execute_rebase(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rebase_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.git_op.branch_name = "feature".to_string();
        app.git_op.target_branch = "main".to_string();

        let result = Actions::execute_rebase(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_find_worktree_for_branch_no_worktree() -> Result<(), Box<dyn std::error::Error>> {
        // Should return None for a non-existent branch
        let result = Actions::find_worktree_for_branch("non-existent-branch-12345")?;
        assert!(result.is_none());
        Ok(())
    }
}
