//! Git rename flow (agents/branches/worktrees/mux sessions).

use crate::mux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode, RenameBranchMode};

use super::super::Actions;

impl Actions {
    /// Rename the selected agent (r key)
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + mux session
    /// For sub-agents: Renames agent title + mux window only
    ///
    /// # Errors
    ///
    /// Returns an error if no agent is selected.
    pub fn rename_agent(app_data: &mut AppData) -> Result<AppMode> {
        let agent = app_data
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

        app_data
            .git_op
            .start_rename(agent_id, current_name.clone(), is_root);
        app_data.input.buffer = current_name;
        app_data.input.cursor = app_data.input.buffer.len();
        Ok(RenameBranchMode.into())
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

    /// Execute rename operation
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + mux session
    /// For sub-agents: Renames agent title + mux window only
    ///
    /// # Errors
    ///
    /// Returns an error if the rename operation fails
    pub fn execute_rename(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for rename".to_string(),
            }
            .into());
        };

        // Verify agent exists
        if app_data.storage.get(agent_id).is_none() {
            app_data.git_op.agent_id = None;
            return Ok(ErrorModalMode {
                message: "Agent not found".to_string(),
            }
            .into());
        }

        let old_name = app_data.git_op.original_branch.clone();
        let new_name = app_data.git_op.branch_name.clone();
        let is_root = app_data.git_op.is_root_rename;

        if old_name == new_name {
            app_data.set_status("Name unchanged");
            app_data.git_op.clear();
            return Ok(AppMode::normal());
        }

        debug!(
            old_name = %old_name,
            new_name = %new_name,
            is_root,
            "Executing rename"
        );

        let result = if is_root {
            // Root agent: rename branch + agent + mux session
            Self::execute_root_rename(app_data, agent_id, &old_name, &new_name)
        } else {
            // Sub-agent: rename agent title + mux window only
            Self::execute_subagent_rename(app_data, agent_id, &new_name)
        };

        if let Err(err) = result {
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: format!("Rename failed: {err:#}"),
            }
            .into());
        }

        app_data.git_op.clear();
        Ok(AppMode::normal())
    }

    /// Execute rename for a root agent (branch + agent + mux session + worktree path)
    fn execute_root_rename(
        app_data: &mut AppData,
        agent_id: uuid::Uuid,
        old_name: &str,
        new_name: &str,
    ) -> Result<()> {
        let agent = app_data
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let old_branch = agent.branch.clone();
        let mux_session = agent.mux_session.clone();

        // Generate new branch name from new title
        let new_branch = app_data.config.generate_branch_name(new_name);
        let new_worktree_path = app_data.config.worktree_dir.join(&new_branch);

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
            anyhow::bail!("Failed to rename branch: {}", stderr.trim());
        }

        // Move the worktree directory and update metadata
        let mut effective_worktree_path = worktree_path.clone();
        if worktree_path != new_worktree_path {
            if Self::move_worktree_directory(
                &worktree_path,
                &new_worktree_path,
                &old_branch,
                &new_branch,
            )? {
                effective_worktree_path.clone_from(&new_worktree_path);
            } else {
                effective_worktree_path.clone_from(&worktree_path);
            }
        }

        // Update agent records and mux session
        Self::update_agent_records(
            app_data,
            agent_id,
            new_name,
            &new_branch,
            &effective_worktree_path,
        )?;
        Self::rename_mux_session_for_agent(app_data, agent_id, &mux_session, new_name)?;

        // Handle remote branch rename if needed
        Self::handle_remote_branch_rename(
            app_data,
            &effective_worktree_path,
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
    ) -> Result<bool> {
        // Ensure parent directory exists
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create worktree parent directory")?;
        }

        // Move the worktree directory
        std::fs::rename(old_path, new_path).context("Failed to move worktree directory")?;

        // Update git worktree metadata
        let gitdir_file = new_path.join(".git");
        if gitdir_file.exists() {
            let git_path_string =
                |path: &std::path::Path| -> String { path.to_string_lossy().to_string() };

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

        Ok(true)
    }

    /// Update agent records after rename
    fn update_agent_records(
        app_data: &mut AppData,
        agent_id: uuid::Uuid,
        new_name: &str,
        new_branch: &str,
        new_worktree_path: &std::path::Path,
    ) -> Result<()> {
        // Update the agent's title, branch name, and worktree path
        if let Some(agent) = app_data.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
            agent.branch = new_branch.to_string();
            agent.worktree_path = new_worktree_path.to_path_buf();
        }

        // Update all descendants' worktree_path
        let descendant_ids: Vec<uuid::Uuid> = app_data
            .storage
            .descendants(agent_id)
            .iter()
            .map(|a| a.id)
            .collect();
        for desc_id in descendant_ids {
            if let Some(desc) = app_data.storage.get_mut(desc_id) {
                desc.worktree_path = new_worktree_path.to_path_buf();
            }
        }

        app_data.storage.save()
    }

    /// Rename mux session and update agent records
    fn rename_mux_session_for_agent(
        app_data: &mut AppData,
        agent_id: uuid::Uuid,
        old_session: &str,
        new_name: &str,
    ) -> Result<()> {
        let session_manager = SessionManager::new();
        let session_prefix = app_data.storage.instance_session_prefix();
        let new_session_name = format!("{session_prefix}{new_name}");

        if let Err(e) = session_manager.rename(old_session, &new_session_name) {
            warn!(error = %e, "Failed to rename mux session");
            return Ok(());
        }

        // Update root agent's mux_session
        if let Some(agent) = app_data.storage.get_mut(agent_id) {
            agent.mux_session.clone_from(&new_session_name);
        }

        // Update all descendants' mux_session
        let descendant_ids: Vec<uuid::Uuid> = app_data
            .storage
            .descendants(agent_id)
            .iter()
            .map(|a| a.id)
            .collect();
        for desc_id in descendant_ids {
            if let Some(desc) = app_data.storage.get_mut(desc_id) {
                desc.mux_session.clone_from(&new_session_name);
            }
        }

        app_data.storage.save()
    }

    /// Handle remote branch rename (delete old, push new)
    fn handle_remote_branch_rename(
        app_data: &mut AppData,
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
            app_data.set_status(format!("Renamed: {old_name} → {new_name}"));
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
            app_data.set_status(format!("Renamed: {old_name} → {new_name}"));
        } else {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            warn!(error = %stderr, "Failed to push renamed branch to remote");
            app_data.set_status(format!("Renamed to {new_name} (remote push failed)"));
        }

        Ok(())
    }

    /// Execute rename for a sub-agent (title + mux window only)
    fn execute_subagent_rename(
        app_data: &mut AppData,
        agent_id: uuid::Uuid,
        new_name: &str,
    ) -> Result<()> {
        let agent = app_data
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let old_name = agent.title.clone();
        let mux_session = agent.mux_session.clone();
        let window_index = agent.window_index;

        // Update the agent's title
        if let Some(agent) = app_data.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
        }
        app_data.storage.save()?;

        // Rename mux window if agent has a window index
        if let Some(idx) = window_index
            && let Err(e) = SessionManager::new().rename_window(&mux_session, idx, new_name)
        {
            warn!(error = %e, "Failed to rename mux window");
        }

        info!(
            old_name = %old_name,
            new_name = %new_name,
            "Sub-agent renamed successfully"
        );
        app_data.set_status(format!("Renamed: {old_name} → {new_name}"));

        Ok(())
    }
}
