//! Git rename flow (agents/branches/worktrees/mux sessions).

use crate::agent::AgentRuntime;
use crate::mux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode, RenameBranchMode};

use super::super::Actions;

impl Actions {
    /// Rename the selected agent (r key)
    ///
    /// For root agents: Renames local branch + agent title + mux session. If a remote branch exists,
    /// Tenex pushes the new branch name but keeps the old remote branch to avoid closing PRs.
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
    /// For root agents: Renames local branch + agent title + mux session. If a remote branch exists,
    /// Tenex pushes the new branch name but keeps the old remote branch to avoid closing PRs.
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

        let repo_root = agent
            .repo_root
            .clone()
            .or_else(|| crate::git::repository_workspace_root(&agent.worktree_path).ok())
            .unwrap_or_else(|| agent.worktree_path.clone());
        let worktree_path = agent.worktree_path.clone();
        let old_branch = agent.branch.clone();
        let mux_session = agent.mux_session.clone();

        // Generate new branch name from new title
        let new_branch = app_data.config.generate_branch_name(new_name);
        let new_worktree_path = app_data
            .config
            .worktree_path_for_repo_root(&repo_root, &new_branch);

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
            Self::move_worktree_directory(
                &worktree_path,
                &new_worktree_path,
                &old_branch,
                &new_branch,
                &repo_root,
            )?;
            effective_worktree_path.clone_from(&new_worktree_path);
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
        repo_root: &std::path::Path,
    ) -> Result<()> {
        // Ensure parent directory exists
        let Some(parent) = new_path.parent() else {
            anyhow::bail!("Worktree path has no parent directory");
        };
        std::fs::create_dir_all(parent).context("Failed to create worktree parent directory")?;

        // Move the worktree directory
        std::fs::rename(old_path, new_path).context("Failed to move worktree directory")?;

        // Update git worktree metadata
        let gitdir_file = new_path.join(".git");
        if gitdir_file.exists() {
            let git_path_string =
                |path: &std::path::Path| -> String { path.to_string_lossy().to_string() };

            let old_worktree_name = old_branch.replace('/', "-");
            let worktree_meta_dir = repo_root
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
                let new_worktree_meta_dir = repo_root
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
        let descendant_ids: std::collections::HashSet<uuid::Uuid> = app_data
            .storage
            .descendant_ids(agent_id)
            .into_iter()
            .collect();
        if !descendant_ids.is_empty() {
            let new_path = new_worktree_path.to_path_buf();
            for agent in app_data.storage.iter_mut() {
                if descendant_ids.contains(&agent.id) {
                    agent.worktree_path.clone_from(&new_path);
                }
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

        let runtime_scope = app_data.storage.get(agent_id).and_then(|agent| {
            (agent.runtime == AgentRuntime::Docker).then(|| {
                if agent.runtime_scope.is_empty() {
                    old_session.to_string()
                } else {
                    agent.runtime_scope.clone()
                }
            })
        });

        // Update root agent's mux_session
        if let Some(agent) = app_data.storage.get_mut(agent_id) {
            if let Some(runtime_scope) = runtime_scope.as_ref() {
                agent.runtime_scope.clone_from(runtime_scope);
            }
            agent.mux_session.clone_from(&new_session_name);
        }

        // Update all descendants' mux_session
        let descendant_ids: std::collections::HashSet<uuid::Uuid> = app_data
            .storage
            .descendant_ids(agent_id)
            .into_iter()
            .collect();
        for agent in app_data.storage.iter_mut() {
            if descendant_ids.contains(&agent.id) {
                if let Some(runtime_scope) = runtime_scope.as_ref()
                    && agent.runtime == AgentRuntime::Docker
                {
                    agent.runtime_scope.clone_from(runtime_scope);
                }
                agent.mux_session.clone_from(&new_session_name);
            }
        }

        app_data.storage.save()
    }

    /// Handle remote branch rename (push new; preserve old)
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
            app_data.set_status(format!(
                "Renamed: {old_name} → {new_name} (kept origin/{old_branch})"
            ));
        } else {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            warn!(error = %stderr, "Failed to push renamed branch to remote");
            app_data.set_status(format!(
                "Renamed to {new_name} (remote push failed; origin/{old_branch} kept)"
            ));
        }

        Ok(())
    }

    /// Execute rename for a sub-agent (title + mux window only)
    fn execute_subagent_rename(
        app_data: &mut AppData,
        agent_id: uuid::Uuid,
        new_name: &str,
    ) -> Result<()> {
        let (old_name, mux_session, window_index) = {
            let agent = app_data
                .storage
                .get_mut(agent_id)
                .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
            let old_name = agent.title.clone();
            let mux_session = agent.mux_session.clone();
            let window_index = agent.window_index;
            agent.title = new_name.to_string();
            (old_name, mux_session, window_index)
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentRuntime, Storage};
    use crate::app::Settings;
    use crate::app::state::App;
    use crate::config::Config;
    use crate::mux::SessionManager;
    use crate::state::AppMode;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use tempfile::{NamedTempFile, TempDir};

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
    }

    fn error_modal_message(mode: &AppMode) -> Option<&str> {
        match mode {
            AppMode::ErrorModal(state) => Some(state.message.as_str()),
            _ => None,
        }
    }

    fn git_ok(repo: &Path, args: &[&str]) {
        let output = crate::git::git_command()
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should spawn");
        assert!(output.status.success());
    }

    fn init_repo_with_commit() -> TempDir {
        let repo_dir = TempDir::new().expect("temp dir");
        git_ok(repo_dir.path(), &["init", "-q", "-b", "master"]);
        git_ok(
            repo_dir.path(),
            &["config", "user.email", "test@example.com"],
        );
        git_ok(repo_dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(repo_dir.path().join("README.md"), "test").expect("write README");
        git_ok(repo_dir.path(), &["add", "."]);
        git_ok(
            repo_dir.path(),
            &["commit", "-q", "--no-verify", "-m", "init"],
        );
        repo_dir
    }

    #[test]
    fn test_rename_agent_emits_debug_log_when_enabled() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let agent = Agent::new(
            "rename-me".to_string(),
            "claude".to_string(),
            "feature/rename-me".to_string(),
            PathBuf::from("/tmp"),
        );
        app.data.storage.add(agent);

        let next =
            with_tracing_dispatch(|| Actions::rename_agent(&mut app.data).expect("rename ok"));
        app.apply_mode(next);
        assert_eq!(app.mode, RenameBranchMode.into());
        assert!(error_modal_message(&app.mode).is_none());
    }

    #[test]
    fn test_execute_rename_returns_error_modal_when_subagent_save_fails() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let failing_state_path = temp_file.path().join("state.json");
        let storage = Storage::with_path(failing_state_path);
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let agent = Agent::new(
            "old-name".to_string(),
            "claude".to_string(),
            "feature/branch".to_string(),
            PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.original_branch = "old-name".to_string();
        app.data.git_op.branch_name = "new-name".to_string();
        app.data.git_op.is_root_rename = false;

        let next = with_tracing_dispatch(|| Actions::execute_rename(&mut app.data).expect("mode"));
        app.apply_mode(next);

        assert!(error_modal_message(&app.mode).is_some());
        assert!(app.data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_rename_agent_returns_error_when_no_agent_selected() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let err = Actions::rename_agent(&mut app.data).expect_err("expected error");
        assert!(err.to_string().contains("No agent selected"));
    }

    #[test]
    fn test_execute_rename_returns_error_modal_when_agent_id_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        app.data.git_op.agent_id = None;
        app.data.git_op.original_branch = "old-name".to_string();
        app.data.git_op.branch_name = "new-name".to_string();
        app.data.git_op.is_root_rename = false;

        let next = Actions::execute_rename(&mut app.data).expect("mode");
        app.apply_mode(next);

        assert_eq!(
            error_modal_message(&app.mode),
            Some("No agent ID for rename")
        );
    }

    #[test]
    fn test_execute_rename_returns_error_modal_when_agent_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.original_branch = "old-name".to_string();
        app.data.git_op.branch_name = "new-name".to_string();
        app.data.git_op.is_root_rename = false;

        let next = Actions::execute_rename(&mut app.data).expect("mode");
        app.apply_mode(next);

        assert_eq!(error_modal_message(&app.mode), Some("Agent not found"));
        assert!(app.data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_execute_root_rename_bails_when_git_branch_rename_fails() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let temp_dir = TempDir::new().expect("temp dir");
        let agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            temp_dir.path().to_path_buf(),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = with_tracing_dispatch(|| {
            Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
        })
        .expect_err("expected git rename failure");
        let message = err.to_string();
        assert!(message.contains("Failed to rename branch:"));
    }

    #[test]
    fn test_execute_root_rename_propagates_check_remote_branch_exists_errors() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let temp_dir = TempDir::new().expect("temp dir");
        let missing_worktree = temp_dir.path().join("missing");

        let agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            missing_worktree,
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = with_tracing_dispatch(|| {
            Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
        })
        .expect_err("expected remote branch check to fail");
        assert!(err.to_string().contains("Failed to check remote branch"));
    }

    #[cfg(unix)]
    #[test]
    fn test_execute_root_rename_reports_error_when_local_branch_rename_spawn_fails() {
        let temp_dir = TempDir::new().expect("temp dir");
        let script_dir = TempDir::new().expect("temp dir");
        let script_path = script_dir.path().join("fake-git.sh");

        std::fs::write(&script_path, "#!/bin/sh\nrm -- \"$0\"\nexit 0\n").expect("write script");
        let mut perms = std::fs::metadata(&script_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("chmod");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            temp_dir.path().to_path_buf(),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = crate::git::with_git_program_override_for_tests(script_path, || {
            Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
        })
        .expect_err("expected local branch rename spawn to fail");
        assert!(err.to_string().contains("Failed to rename local branch"));
    }

    #[test]
    fn test_execute_root_rename_propagates_rename_mux_session_save_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-rename-root-save-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let repo_root = init_repo_with_commit();
        let repo = crate::git::open_repository(repo_root.path()).expect("open repo");
        let worktree_root = TempDir::new().expect("worktree dir");
        let config = Config {
            worktree_dir: worktree_root.path().to_path_buf(),
            ..Config::default()
        };

        let old_branch = "tenex/old-name";
        let worktree_path = config.worktree_path_for_repo_root(repo_root.path(), old_branch);
        let worktree_mgr = crate::git::WorktreeManager::new(&repo);
        worktree_mgr
            .create_with_new_branch(&worktree_path, old_branch)
            .expect("create worktree");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(config, storage, Settings::default(), false);

        let mut agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            old_branch.to_string(),
            worktree_path,
        );
        agent.repo_root = Some(repo_root.path().to_path_buf());
        let agent_id = agent.id;
        let old_session = agent.mux_session.clone();
        app.data.storage.add(agent);

        let session_manager = SessionManager::new();
        session_manager
            .create(&old_session, repo_root.path(), None)
            .expect("create mux session");

        let err = Storage::with_forced_save_error_after_successes_for_tests(1, || {
            with_tracing_dispatch(|| {
                Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
            })
        })
        .expect_err("expected forced save error");
        assert!(
            err.to_string()
                .contains("forced storage save error for test")
        );

        let prefix = app.data.storage.instance_session_prefix();
        let renamed_session = format!("{prefix}new-agent");
        let _ = session_manager.kill(&renamed_session);
    }

    #[test]
    fn test_execute_root_rename_returns_error_when_agent_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let err = Actions::execute_root_rename(
            &mut app.data,
            uuid::Uuid::new_v4(),
            "old-agent",
            "new-agent",
        )
        .expect_err("expected missing agent error");

        assert!(err.to_string().contains("Agent not found"));
    }

    #[test]
    fn test_execute_root_rename_propagates_move_worktree_directory_errors() {
        let repo_dir = init_repo_with_commit();
        let worktree_dir = TempDir::new().expect("temp dir");
        let worktree_dir_file = worktree_dir.path().join("worktrees-file");
        std::fs::write(&worktree_dir_file, "not a directory").expect("write worktrees file");

        let config = Config {
            worktree_dir: worktree_dir_file,
            ..Config::default()
        };

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(config, storage, Settings::default(), false);

        let mut agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
        );
        agent.repo_root = Some(repo_dir.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = with_tracing_dispatch(|| {
            Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
        })
        .expect_err("expected move worktree failure");

        assert!(
            err.to_string()
                .contains("Failed to create worktree parent directory")
        );
    }

    #[test]
    fn test_execute_root_rename_propagates_update_agent_records_errors() {
        let worktree_dir = TempDir::new().expect("temp dir");
        let repo_root = TempDir::new().expect("temp dir");

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let new_branch = config.generate_branch_name("new-agent");
        let worktree_path = config.worktree_path_for_repo_root(repo_root.path(), &new_branch);
        std::fs::create_dir_all(&worktree_path).expect("worktree path");
        git_ok(&worktree_path, &["init", "-q", "-b", "master"]);
        git_ok(
            &worktree_path,
            &["config", "user.email", "tenex@test.invalid"],
        );
        git_ok(&worktree_path, &["config", "user.name", "Tenex Test"]);
        std::fs::write(worktree_path.join("README.md"), "test\n").expect("write readme");
        git_ok(&worktree_path, &["add", "."]);
        git_ok(
            &worktree_path,
            &["commit", "-q", "--no-verify", "-m", "init"],
        );

        let temp_file = NamedTempFile::new().expect("temp file");
        let failing_state_path = temp_file.path().join("state.json");
        let storage = Storage::with_path(failing_state_path);
        let mut app = App::new(config, storage, Settings::default(), false);

        let mut agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            worktree_path,
        );
        agent.repo_root = Some(repo_root.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = with_tracing_dispatch(|| {
            Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
        })
        .expect_err("expected save failure");

        assert!(err.to_string().contains("Failed to create state directory"));
    }

    #[test]
    fn test_update_agent_records_propagates_storage_save_errors() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let failing_state_path = temp_file.path().join("state.json");
        let storage = Storage::with_path(failing_state_path);
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let agent = Agent::new(
            "agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = Actions::update_agent_records(
            &mut app.data,
            agent_id,
            "new-title",
            "agent/new-title",
            Path::new("/tmp/new-worktree"),
        )
        .expect_err("expected save failure");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_update_agent_records_noops_when_agent_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        Actions::update_agent_records(
            &mut app.data,
            uuid::Uuid::new_v4(),
            "new-title",
            "agent/new-title",
            Path::new("/tmp/new-worktree"),
        )
        .expect("update ok");
    }

    #[test]
    fn test_handle_remote_branch_rename_sets_status_when_remote_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        with_tracing_dispatch(|| {
            Actions::handle_remote_branch_rename(
                &mut app.data,
                Path::new("/tmp"),
                "old-branch",
                "new-branch",
                "Old Name",
                "New Name",
                false,
            )
            .expect("rename ok");
        });

        let status = app.data.ui.status_message.as_deref().unwrap_or("");
        assert!(status.contains("Renamed:"));
    }

    #[test]
    fn test_handle_remote_branch_rename_sets_status_when_push_fails() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        with_tracing_dispatch(|| {
            Actions::handle_remote_branch_rename(
                &mut app.data,
                Path::new("/tmp"),
                "old-branch",
                "new-branch",
                "Old Name",
                "New Name",
                true,
            )
            .expect("rename ok");
        });

        let status = app.data.ui.status_message.as_deref().unwrap_or("");
        assert!(status.contains("remote push failed"));
    }

    #[test]
    fn test_handle_remote_branch_rename_propagates_spawn_errors() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let missing_dir = TempDir::new().expect("temp dir");
        let worktree_path = missing_dir.path().join("missing");

        let err = Actions::handle_remote_branch_rename(
            &mut app.data,
            &worktree_path,
            "old-branch",
            "new-branch",
            "Old Name",
            "New Name",
            true,
        )
        .expect_err("expected spawn error");

        assert!(err.to_string().contains("Failed to push renamed branch"));
    }

    #[test]
    fn test_handle_remote_branch_rename_push_success_sets_status() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let temp_dir = TempDir::new().expect("temp dir");
        let remote_repo = temp_dir.path().join("remote.git");
        let local_repo = temp_dir.path().join("local");
        std::fs::create_dir_all(&remote_repo).expect("create remote");
        std::fs::create_dir_all(&local_repo).expect("create local");

        git_ok(&remote_repo, &["init", "--bare"]);
        git_ok(&local_repo, &["init", "-q", "-b", "master"]);
        git_ok(&local_repo, &["config", "user.email", "tenex@test.invalid"]);
        git_ok(&local_repo, &["config", "user.name", "Tenex Test"]);
        std::fs::write(local_repo.join("README.md"), "test\n").expect("write readme");
        git_ok(&local_repo, &["add", "."]);
        git_ok(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"]);
        git_ok(
            &local_repo,
            &[
                "remote",
                "add",
                "origin",
                remote_repo.to_string_lossy().as_ref(),
            ],
        );
        git_ok(&local_repo, &["checkout", "-q", "-b", "new-branch"]);

        with_tracing_dispatch(|| {
            Actions::handle_remote_branch_rename(
                &mut app.data,
                &local_repo,
                "old-branch",
                "new-branch",
                "Old Name",
                "New Name",
                true,
            )
            .expect("push ok");
        });

        let status = app.data.ui.status_message.as_deref().unwrap_or("");
        assert!(status.contains("kept origin/old-branch"));
    }

    #[test]
    fn test_check_remote_branch_exists_propagates_spawn_errors() {
        let temp_dir = TempDir::new().expect("temp dir");
        let worktree_path = temp_dir.path().join("missing");
        let err = Actions::check_remote_branch_exists(&worktree_path, "master")
            .expect_err("expected error");
        assert!(err.to_string().contains("Failed to check remote branch"));
    }

    #[test]
    fn test_check_remote_branch_exists_returns_true_when_remote_tracking_ref_present() {
        let repo_dir = init_repo_with_commit();
        git_ok(
            repo_dir.path(),
            &["update-ref", "refs/remotes/origin/master", "HEAD"],
        );

        let result = Actions::check_remote_branch_exists(repo_dir.path(), "master")
            .expect("check remote branch exists");
        assert!(result);
    }

    #[test]
    fn test_rename_mux_session_for_agent_updates_runtime_scope_and_descendants_for_docker() {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-rename-mux-session-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let repo_dir = init_repo_with_commit();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.runtime = AgentRuntime::Docker;
        root.runtime_scope.clear();
        let root_id = root.id;
        let old_session = root.mux_session.clone();

        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: old_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        let child_id = child.id;
        let mut child = child;
        child.runtime = AgentRuntime::Docker;
        app.data.storage.add(root);
        app.data.storage.add(child);

        let session_manager = SessionManager::new();
        session_manager
            .create(&old_session, repo_dir.path(), None)
            .expect("create mux session");

        let first_expected_prefix = app.data.storage.instance_session_prefix();
        let first_expected = format!("{first_expected_prefix}renamed");

        tracing::dispatcher::with_default(&dispatch, || {
            Actions::rename_mux_session_for_agent(&mut app.data, root_id, &old_session, "renamed")
                .expect("rename mux session");
        });

        let root_agent = app.data.storage.get(root_id).expect("root agent");
        assert_eq!(root_agent.mux_session, first_expected);
        assert_eq!(root_agent.runtime_scope, old_session);

        let child_agent = app.data.storage.get(child_id).expect("child agent");
        assert_eq!(child_agent.mux_session, first_expected);
        assert_eq!(child_agent.runtime_scope, old_session);

        let second_expected_prefix = app.data.storage.instance_session_prefix();
        let second_expected = format!("{second_expected_prefix}second");

        tracing::dispatcher::with_default(&dispatch, || {
            Actions::rename_mux_session_for_agent(
                &mut app.data,
                root_id,
                &first_expected,
                "second",
            )
            .expect("rename mux session");
        });

        let root_agent = app.data.storage.get(root_id).expect("root agent");
        assert_eq!(root_agent.mux_session, second_expected);
        assert_eq!(root_agent.runtime_scope, old_session);

        let _ = session_manager.kill(&second_expected);
    }

    #[test]
    fn test_rename_mux_session_for_agent_updates_runtime_scope_when_non_empty() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-rename-mux-session-scope-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let repo_dir = init_repo_with_commit();
        let old_session = format!("tenex-scope-{}", uuid::Uuid::new_v4());
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.runtime = AgentRuntime::Docker;
        root.runtime_scope = "custom-scope".to_string();
        root.mux_session.clone_from(&old_session);
        let root_id = root.id;

        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: old_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        let child_id = child.id;
        let mut child = child;
        child.runtime = AgentRuntime::Docker;

        app.data.storage.add(root);
        app.data.storage.add(child);

        let session_manager = SessionManager::new();
        session_manager
            .create(&old_session, repo_dir.path(), None)
            .expect("create mux session");

        let expected_prefix = app.data.storage.instance_session_prefix();
        let expected_session = format!("{expected_prefix}renamed");
        Actions::rename_mux_session_for_agent(&mut app.data, root_id, &old_session, "renamed")
            .expect("rename ok");

        let root_agent = app.data.storage.get(root_id).expect("root");
        assert_eq!(root_agent.runtime_scope, "custom-scope");

        let child_agent = app.data.storage.get(child_id).expect("child");
        assert_eq!(child_agent.runtime_scope, "custom-scope");

        let _ = session_manager.kill(&expected_session);
    }

    #[test]
    fn test_rename_mux_session_for_agent_does_not_touch_runtime_scope_when_not_docker() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-rename-mux-session-host-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let repo_dir = init_repo_with_commit();
        let old_session = format!("tenex-host-{}", uuid::Uuid::new_v4());
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.runtime = AgentRuntime::Host;
        root.runtime_scope = "should-stay".to_string();
        root.mux_session.clone_from(&old_session);
        let root_id = root.id;
        app.data.storage.add(root);

        let session_manager = SessionManager::new();
        session_manager
            .create(&old_session, repo_dir.path(), None)
            .expect("create mux session");

        let expected_prefix = app.data.storage.instance_session_prefix();
        let expected_session = format!("{expected_prefix}renamed");
        Actions::rename_mux_session_for_agent(&mut app.data, root_id, &old_session, "renamed")
            .expect("rename ok");

        let root_agent = app.data.storage.get(root_id).expect("root");
        assert_eq!(root_agent.runtime_scope, "should-stay");

        let _ = session_manager.kill(&expected_session);
    }

    #[test]
    fn test_rename_mux_session_for_agent_returns_ok_when_session_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let repo_dir = init_repo_with_commit();
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.runtime = AgentRuntime::Docker;
        let root_id = root.id;
        let old_session = root.mux_session.clone();
        app.data.storage.add(root);

        Actions::rename_mux_session_for_agent(&mut app.data, root_id, &old_session, "renamed")
            .expect("rename ok");

        let root_agent = app.data.storage.get(root_id).expect("root");
        assert_eq!(root_agent.mux_session, old_session);
    }

    #[test]
    fn test_rename_mux_session_for_agent_updates_descendants_when_root_missing() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-rename-mux-orphan-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let repo_dir = init_repo_with_commit();
        let old_session = format!("tenex-orphan-{}", uuid::Uuid::new_v4());
        let missing_root_id = uuid::Uuid::new_v4();

        let mut orphan = Agent::new_child(
            "orphan".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: missing_root_id,
                mux_session: old_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        orphan.runtime = AgentRuntime::Docker;
        let orphan_id = orphan.id;
        app.data.storage.add(orphan);
        app.data.storage.add(Agent::new(
            "other".to_string(),
            "claude".to_string(),
            "master".to_string(),
            repo_dir.path().to_path_buf(),
        ));

        let session_manager = SessionManager::new();
        session_manager
            .create(&old_session, repo_dir.path(), None)
            .expect("create mux session");

        let expected_prefix = app.data.storage.instance_session_prefix();
        let expected_session = format!("{expected_prefix}renamed");
        Actions::rename_mux_session_for_agent(
            &mut app.data,
            missing_root_id,
            &old_session,
            "renamed",
        )
        .expect("rename ok");

        let orphan_agent = app.data.storage.get(orphan_id).expect("orphan");
        assert_eq!(orphan_agent.mux_session, expected_session);
        assert!(orphan_agent.runtime_scope.is_empty());

        let _ = session_manager.kill(&expected_session);
    }

    #[test]
    fn test_execute_subagent_rename_warns_when_rename_window_fails() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let mut agent = Agent::new(
            "old".to_string(),
            "claude".to_string(),
            "master".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.window_index = Some(1);
        let agent_id = agent.id;
        app.data.storage.add(agent);

        with_tracing_dispatch(|| {
            Actions::execute_subagent_rename(&mut app.data, agent_id, "new").expect("rename ok");
        });

        let renamed = app.data.storage.get(agent_id).expect("agent");
        assert_eq!(renamed.title, "new");
    }

    #[test]
    fn test_execute_subagent_rename_returns_error_when_agent_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let err = Actions::execute_subagent_rename(&mut app.data, uuid::Uuid::new_v4(), "new")
            .expect_err("expected error");
        assert!(err.to_string().contains("Agent not found"));
    }

    #[test]
    fn test_execute_subagent_rename_skips_rename_window_when_index_missing() {
        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let agent = Agent::new(
            "old".to_string(),
            "claude".to_string(),
            "master".to_string(),
            PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        Actions::execute_subagent_rename(&mut app.data, agent_id, "new").expect("rename ok");

        let renamed = app.data.storage.get(agent_id).expect("agent");
        assert_eq!(renamed.title, "new");
    }

    #[test]
    fn test_move_worktree_directory_propagates_parent_creation_error() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("repo root");

        let old_path = temp_dir.path().join("old");
        std::fs::create_dir_all(&old_path).expect("old worktree");

        let parent_path = temp_dir.path().join("parent");
        std::fs::write(&parent_path, "not a dir").expect("parent file");
        let new_path = parent_path.join("new");

        let err = Actions::move_worktree_directory(
            &old_path,
            &new_path,
            "agent/old",
            "agent/new",
            &repo_root,
        )
        .expect_err("expected error");

        assert!(
            err.to_string()
                .contains("Failed to create worktree parent directory")
        );
    }

    #[test]
    fn test_move_worktree_directory_propagates_rename_error() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("repo root");

        let old_path = temp_dir.path().join("old");
        let new_path = temp_dir.path().join("new");

        let err = Actions::move_worktree_directory(
            &old_path,
            &new_path,
            "agent/old",
            "agent/new",
            &repo_root,
        )
        .expect_err("expected error");

        assert!(
            err.to_string()
                .contains("Failed to move worktree directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_move_worktree_directory_errors_when_new_path_has_no_parent() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("repo root");

        let old_path = temp_dir.path().join("old");
        std::fs::create_dir_all(&old_path).expect("old worktree");

        let err = Actions::move_worktree_directory(
            &old_path,
            Path::new("/"),
            "agent/old",
            "agent/new",
            &repo_root,
        )
        .expect_err("expected error");

        assert!(
            err.to_string()
                .contains("Worktree path has no parent directory")
        );
    }

    #[test]
    fn test_move_worktree_directory_skips_gitdir_update_when_gitdir_metadata_missing() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        let old_path = temp_dir.path().join("old-worktree");
        let new_path = temp_dir.path().join("new-worktree");
        std::fs::create_dir_all(&repo_root).expect("repo root");
        std::fs::create_dir_all(&old_path).expect("old worktree");
        std::fs::create_dir_all(new_path.parent().expect("parent")).expect("parent");

        std::fs::write(old_path.join(".git"), "gitdir: /tmp/old\n").expect("git file");

        let old_branch = "agent/old";
        let new_branch = "agent/new";
        let old_worktree_name = old_branch.replace('/', "-");
        let new_worktree_name = new_branch.replace('/', "-");
        let worktrees_dir = repo_root.join(".git").join("worktrees");
        let meta_dir = worktrees_dir.join(&old_worktree_name);
        std::fs::create_dir_all(&meta_dir).expect("meta dir");

        Actions::move_worktree_directory(&old_path, &new_path, old_branch, new_branch, &repo_root)
            .expect("move ok");

        let renamed_meta_dir = worktrees_dir.join(&new_worktree_name);
        assert!(renamed_meta_dir.exists());
        assert!(!renamed_meta_dir.join("gitdir").exists());

        let git_pointer = std::fs::read_to_string(new_path.join(".git")).expect("read git pointer");
        assert!(git_pointer.contains("gitdir:"));
        assert!(git_pointer.contains(renamed_meta_dir.to_string_lossy().as_ref()));
    }

    #[test]
    fn test_move_worktree_directory_skips_meta_dir_rename_when_worktree_name_unchanged() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        let old_path = temp_dir.path().join("old-worktree");
        let new_path = temp_dir.path().join("new-worktree");
        std::fs::create_dir_all(&repo_root).expect("repo root");
        std::fs::create_dir_all(&old_path).expect("old worktree");
        std::fs::create_dir_all(new_path.parent().expect("parent")).expect("parent");

        std::fs::write(old_path.join(".git"), "gitdir: /tmp/old\n").expect("git file");

        let old_branch = "agent/old";
        let new_branch = "agent-old";
        let worktree_name = old_branch.replace('/', "-");
        assert_eq!(worktree_name, new_branch);

        let meta_dir = repo_root
            .join(".git")
            .join("worktrees")
            .join(&worktree_name);
        std::fs::create_dir_all(&meta_dir).expect("meta dir");
        let gitdir_path = meta_dir.join("gitdir");
        std::fs::write(&gitdir_path, "old\n").expect("gitdir file");

        Actions::move_worktree_directory(&old_path, &new_path, old_branch, new_branch, &repo_root)
            .expect("move ok");

        let gitdir = std::fs::read_to_string(&gitdir_path).expect("read gitdir");
        assert!(gitdir.contains(new_path.join(".git").to_string_lossy().as_ref()));
        assert!(meta_dir.exists());
    }

    #[test]
    fn test_move_worktree_directory_skips_metadata_update_when_worktree_has_no_git_pointer() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_root).expect("repo root");

        let old_path = temp_dir.path().join("old-worktree");
        let new_path = temp_dir.path().join("new-worktree");
        std::fs::create_dir_all(&old_path).expect("old worktree");

        Actions::move_worktree_directory(
            &old_path,
            &new_path,
            "agent/old",
            "agent/new",
            &repo_root,
        )
        .expect("move ok");

        assert!(new_path.exists());
        assert!(!new_path.join(".git").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_execute_root_rename_propagates_handle_remote_branch_rename_errors() {
        let worktree_dir = TempDir::new().expect("temp dir");
        let repo_root = TempDir::new().expect("temp dir");

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let new_branch = config.generate_branch_name("new-agent");
        let worktree_path = config.worktree_path_for_repo_root(repo_root.path(), &new_branch);
        std::fs::create_dir_all(&worktree_path).expect("worktree path");
        git_ok(&worktree_path, &["init", "-q", "-b", "master"]);
        git_ok(
            &worktree_path,
            &["config", "user.email", "tenex@test.invalid"],
        );
        git_ok(&worktree_path, &["config", "user.name", "Tenex Test"]);
        std::fs::write(worktree_path.join("README.md"), "test\n").expect("write readme");
        git_ok(&worktree_path, &["add", "."]);
        git_ok(
            &worktree_path,
            &["commit", "-q", "--no-verify", "-m", "init"],
        );

        let script_dir = TempDir::new().expect("temp dir");
        let script_path = script_dir.path().join("fake-git.sh");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
set -eu
if [ $# -ge 1 ] && [ "$1" = "rev-parse" ]; then
  exit 0
fi
if [ $# -ge 2 ] && [ "$1" = "branch" ] && [ "$2" = "-m" ]; then
  cwd="$PWD"
  cd /
  rm -rf "$cwd"
  exit 0
fi
exit 0
"#,
        )
        .expect("write script");
        let mut perms = std::fs::metadata(&script_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).expect("chmod");

        let temp_file = NamedTempFile::new().expect("temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(config, storage, Settings::default(), false);

        let mut agent = Agent::new(
            "old-agent".to_string(),
            "claude".to_string(),
            "master".to_string(),
            worktree_path,
        );
        agent.repo_root = Some(repo_root.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        let err = crate::git::with_git_program_override_for_tests(script_path, || {
            Actions::execute_root_rename(&mut app.data, agent_id, "old-agent", "new-agent")
        })
        .expect_err("expected push error");

        assert!(err.to_string().contains("Failed to push renamed branch"));
    }

    #[test]
    fn test_move_worktree_directory_warns_when_gitdir_write_and_meta_dir_rename_fail() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        let old_path = temp_dir.path().join("old-worktree");
        let new_path = temp_dir.path().join("new-worktree");
        std::fs::create_dir_all(&repo_root).expect("repo root");
        std::fs::create_dir_all(&old_path).expect("old worktree");
        std::fs::create_dir_all(new_path.parent().expect("parent")).expect("parent");

        let old_branch = "agent/old";
        let new_branch = "agent/new";
        let old_worktree_name = old_branch.replace('/', "-");
        let new_worktree_name = new_branch.replace('/', "-");

        let worktrees_dir = repo_root.join(".git").join("worktrees");
        let meta_dir = worktrees_dir.join(&old_worktree_name);
        std::fs::create_dir_all(&meta_dir).expect("meta dir");

        let gitdir_path = meta_dir.join("gitdir");
        std::fs::create_dir_all(&gitdir_path).expect("gitdir as dir");

        let destination_meta = worktrees_dir.join(&new_worktree_name);
        std::fs::write(&destination_meta, "not-a-dir").expect("create destination file");

        std::fs::write(old_path.join(".git"), "gitdir: /tmp/old\n").expect("git file");

        with_tracing_dispatch(|| {
            Actions::move_worktree_directory(
                &old_path, &new_path, old_branch, new_branch, &repo_root,
            )
            .expect("move ok");
        });

        assert!(new_path.exists());
    }

    #[test]
    fn test_move_worktree_directory_warns_when_git_pointer_update_fails() {
        let temp_dir = TempDir::new().expect("temp dir");
        let repo_root = temp_dir.path().join("repo");
        let old_path = temp_dir.path().join("old-worktree");
        let new_path = temp_dir.path().join("new-worktree");
        std::fs::create_dir_all(&repo_root).expect("repo root");
        std::fs::create_dir_all(&old_path).expect("old worktree");

        let old_branch = "agent/old";
        let new_branch = "agent/new";
        let old_worktree_name = old_branch.replace('/', "-");
        let new_worktree_name = new_branch.replace('/', "-");

        let worktrees_dir = repo_root.join(".git").join("worktrees");
        let meta_dir = worktrees_dir.join(&old_worktree_name);
        std::fs::create_dir_all(&meta_dir).expect("meta dir");
        std::fs::write(meta_dir.join("gitdir"), "gitdir").expect("gitdir file");

        std::fs::create_dir_all(old_path.join(".git")).expect("git dir");

        with_tracing_dispatch(|| {
            Actions::move_worktree_directory(
                &old_path, &new_path, old_branch, new_branch, &repo_root,
            )
            .expect("move ok");
        });

        let renamed_meta_dir = worktrees_dir.join(&new_worktree_name);
        assert!(renamed_meta_dir.exists());
    }
}
