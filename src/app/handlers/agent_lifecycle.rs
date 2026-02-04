//! Agent lifecycle operations: create, kill, reconnect

use crate::agent::{Agent, ChildConfig};
use crate::git::{self, WorktreeManager};
use crate::mux::SessionManager;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::Actions;
use super::swarm::SpawnConfig;
use crate::app::{AppData, WorktreeConflictInfo};
use crate::config::Config;
use crate::state::{AppMode, ConfirmAction, ConfirmingMode, ErrorModalMode};

#[derive(Debug)]
struct BranchSwitchTarget {
    branch: String,
    worktree_path: std::path::PathBuf,
}

impl Actions {
    /// Create a new agent
    ///
    /// If a worktree with the same name already exists, this will prompt the user
    /// to either reconnect to the existing worktree or recreate it from scratch.
    ///
    /// # Errors
    ///
    /// Returns an error if agent creation fails
    pub fn create_agent(
        self,
        app_data: &mut AppData,
        title: &str,
        prompt: Option<&str>,
    ) -> Result<AppMode> {
        debug!(title, prompt, "Creating new agent");

        let repo_path = app_data
            .selected_project_root()
            .or_else(|| std::env::current_dir().ok())
            .context("Failed to resolve target directory")?;
        let Ok(repo) = git::open_repository(&repo_path) else {
            self.create_agent_in_plain_dir(app_data, title, prompt, &repo_path)?;
            return Ok(AppMode::normal());
        };
        let branch = app_data.config.generate_branch_name(title);
        let worktree_path = app_data
            .config
            .worktree_path_for_repo_root(&repo_path, &branch);

        let worktree_mgr = WorktreeManager::new(&repo);

        // Check if worktree/branch already exists - prompt user for action
        if worktree_mgr.exists(&branch) {
            debug!(branch, "Worktree already exists, prompting user");
            let conflict_worktree_path = worktree_mgr
                .worktree_path(&branch)
                .unwrap_or_else(|| worktree_path.clone());

            // Get current HEAD info for new worktree context
            let (current_branch, current_commit) = worktree_mgr
                .head_info()
                .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));

            // Try to get existing worktree info
            let (existing_branch, existing_commit) = worktree_mgr
                .worktree_head_info(&branch)
                .map(|(b, c)| (Some(b), Some(c)))
                .unwrap_or((None, None));

            app_data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
                title: title.to_string(),
                prompt: prompt.map(String::from),
                branch: branch.clone(),
                worktree_path: conflict_worktree_path,
                repo_root: repo_path.clone(),
                existing_branch,
                existing_commit,
                current_branch,
                current_commit,
                swarm_child_count: None, // Not a swarm creation
            });
            return Ok(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into());
        }

        self.create_agent_internal(app_data, &repo_path, title, prompt, &branch, &worktree_path)?;
        Ok(AppMode::normal())
    }

    fn create_agent_in_plain_dir(
        self,
        app_data: &mut AppData,
        title: &str,
        prompt: Option<&str>,
        workdir: &Path,
    ) -> Result<()> {
        let program = app_data.agent_spawn_command();
        let branch = app_data.config.generate_branch_name(title);

        let mut agent = Agent::new(
            title.to_string(),
            program.clone(),
            branch,
            workdir.to_path_buf(),
        );
        agent.workspace_kind = crate::agent::WorkspaceKind::PlainDir;
        agent.repo_root = Some(workdir.to_path_buf());

        let cli = crate::conversation::detect_agent_cli(&program);
        if cli == crate::conversation::AgentCli::Claude {
            agent.conversation_id = Some(agent.id.to_string());
        }
        let session_prefix = app_data.storage.instance_session_prefix();
        agent.mux_session = format!("{session_prefix}{}", agent.short_id());

        let command = crate::conversation::build_spawn_argv(
            &program,
            prompt,
            agent.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&agent.mux_session, workdir, Some(&command))?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                workdir,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&agent.mux_session, width, height);
        }

        app_data.storage.add(agent);
        app_data.storage.save()?;

        info!(title, "Agent created in plain directory");
        app_data.set_status(format!("Created agent: {title}"));
        Ok(())
    }

    /// Internal function to actually create the agent after conflict resolution
    pub(crate) fn create_agent_internal(
        self,
        app_data: &mut AppData,
        repo_path: &Path,
        title: &str,
        prompt: Option<&str>,
        branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<()> {
        let repo = git::open_repository(repo_path)?;
        let worktree_mgr = WorktreeManager::new(&repo);

        worktree_mgr.create_with_new_branch(worktree_path, branch)?;

        let program = app_data.agent_spawn_command();
        let mut agent = Agent::new(
            title.to_string(),
            program.clone(),
            branch.to_string(),
            worktree_path.to_path_buf(),
        );
        agent.repo_root = Some(repo_path.to_path_buf());
        let cli = crate::conversation::detect_agent_cli(&program);
        if cli == crate::conversation::AgentCli::Claude {
            agent.conversation_id = Some(agent.id.to_string());
        }
        let session_prefix = app_data.storage.instance_session_prefix();
        agent.mux_session = format!("{session_prefix}{}", agent.short_id());

        let command = crate::conversation::build_spawn_argv(
            &program,
            prompt,
            agent.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&agent.mux_session, worktree_path, Some(&command))?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        // Resize the new session to match preview dimensions
        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&agent.mux_session, width, height);
        }

        app_data.storage.add(agent);
        app_data.storage.save()?;

        info!(title, %branch, "Agent created successfully");
        app_data.set_status(format!("Created agent: {title}"));
        Ok(())
    }

    /// Reconnect to an existing worktree (user chose to keep it)
    ///
    /// # Errors
    ///
    /// Returns an error if the mux session cannot be created or storage fails
    pub fn reconnect_to_worktree(self, app_data: &mut AppData) -> Result<AppMode> {
        let conflict = app_data
            .spawn
            .worktree_conflict
            .take()
            .ok_or_else(|| anyhow::anyhow!("No worktree conflict info available"))?;

        debug!(branch = %conflict.branch, swarm_child_count = ?conflict.swarm_child_count, "Reconnecting to existing worktree");

        let program = app_data.agent_spawn_command();

        let removed_agents = self.remove_conflicting_agents(app_data, &conflict);
        if removed_agents > 0 {
            debug!(
                branch = %conflict.branch,
                removed_agents,
                "Removed existing agents before reconnect"
            );
        }

        if let Some(child_count) = conflict.swarm_child_count {
            self.reconnect_swarm_to_worktree(app_data, &conflict, &program, child_count)?;
        } else {
            self.reconnect_single_to_worktree(app_data, &conflict, &program)?;
        }

        app_data.storage.save()?;
        Ok(AppMode::normal())
    }

    fn remove_conflicting_agents(
        self,
        app_data: &mut AppData,
        conflict: &WorktreeConflictInfo,
    ) -> usize {
        let mut ids_to_remove: Vec<Uuid> = Vec::new();
        let mut sessions_to_consider: HashSet<String> = HashSet::new();

        for agent in app_data.storage.iter() {
            if agent.branch == conflict.branch && agent.worktree_path == conflict.worktree_path {
                ids_to_remove.push(agent.id);
                sessions_to_consider.insert(agent.mux_session.clone());
            }
        }

        if ids_to_remove.is_empty() {
            return 0;
        }

        let ids_set: HashSet<Uuid> = ids_to_remove.iter().copied().collect();

        for session in sessions_to_consider {
            let session_used_elsewhere = app_data
                .storage
                .iter()
                .any(|agent| agent.mux_session == session && !ids_set.contains(&agent.id));
            if session_used_elsewhere {
                continue;
            }
            let _ = self.session_manager.kill(&session);
        }

        let mut removed = 0;
        for id in ids_to_remove {
            if app_data.storage.remove(id).is_some() {
                removed += 1;
            }
        }

        if removed > 0 {
            app_data.validate_selection();
        }

        removed
    }

    fn reconnect_swarm_to_worktree(
        self,
        app_data: &mut AppData,
        conflict: &WorktreeConflictInfo,
        program: &str,
        child_count: usize,
    ) -> Result<()> {
        let mut root_agent = Agent::new(
            conflict.title.clone(),
            program.to_string(),
            conflict.branch.clone(),
            conflict.worktree_path.clone(),
        );
        root_agent.repo_root = Some(conflict.repo_root.clone());
        let cli = crate::conversation::detect_agent_cli(program);
        if cli == crate::conversation::AgentCli::Claude {
            root_agent.conversation_id = Some(root_agent.id.to_string());
        }
        let session_prefix = app_data.storage.instance_session_prefix();
        root_agent.mux_session = format!("{session_prefix}{}", root_agent.short_id());

        let root_session = root_agent.mux_session.clone();
        let root_id = root_agent.id;

        let command = crate::conversation::build_spawn_argv(
            program,
            None,
            root_agent.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&root_session, &conflict.worktree_path, Some(&command))?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            root_agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                &conflict.worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&root_session, width, height);
        }

        app_data.storage.add(root_agent);

        let task = conflict.prompt.as_deref().unwrap_or("");
        let spawn_config = SpawnConfig {
            root_session,
            worktree_path: conflict.worktree_path.clone(),
            branch: conflict.branch.clone(),
            workspace_kind: crate::agent::WorkspaceKind::GitWorktree,
            parent_agent_id: root_id,
        };
        self.spawn_children_for_root(app_data, &spawn_config, child_count, task)?;

        info!(
            title = %conflict.title,
            branch = %conflict.branch,
            child_count,
            "Reconnected swarm to existing worktree"
        );
        app_data.set_status(format!("Reconnected swarm: {}", conflict.title));

        Ok(())
    }

    fn reconnect_single_to_worktree(
        self,
        app_data: &mut AppData,
        conflict: &WorktreeConflictInfo,
        program: &str,
    ) -> Result<()> {
        let mut agent = Agent::new(
            conflict.title.clone(),
            program.to_string(),
            conflict.branch.clone(),
            conflict.worktree_path.clone(),
        );
        agent.repo_root = Some(conflict.repo_root.clone());
        let cli = crate::conversation::detect_agent_cli(program);
        if cli == crate::conversation::AgentCli::Claude {
            agent.conversation_id = Some(agent.id.to_string());
        }
        let session_prefix = app_data.storage.instance_session_prefix();
        agent.mux_session = format!("{session_prefix}{}", agent.short_id());

        let command = crate::conversation::build_spawn_argv(
            program,
            conflict.prompt.as_deref(),
            agent.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&agent.mux_session, &conflict.worktree_path, Some(&command))?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                &conflict.worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&agent.mux_session, width, height);
        }

        app_data.storage.add(agent);

        info!(
            title = %conflict.title,
            branch = %conflict.branch,
            "Reconnected to existing worktree"
        );
        app_data.set_status(format!("Reconnected to: {}", conflict.title));

        Ok(())
    }

    /// Recreate the worktree (user chose to delete and start fresh)
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be removed/recreated or agent creation fails
    pub fn recreate_worktree(self, app_data: &mut AppData) -> Result<AppMode> {
        let conflict = app_data
            .spawn
            .worktree_conflict
            .take()
            .ok_or_else(|| anyhow::anyhow!("No worktree conflict info available"))?;

        debug!(branch = %conflict.branch, swarm_child_count = ?conflict.swarm_child_count, "Recreating worktree from scratch");

        // Remove existing worktree first
        let repo = git::open_repository(&conflict.repo_root)?;
        let worktree_mgr = WorktreeManager::new(&repo);
        worktree_mgr.remove(&conflict.branch)?;

        // Check if this is a swarm creation
        if let Some(child_count) = conflict.swarm_child_count {
            // Set up app state for spawn_children
            app_data.spawn.spawning_under = None;
            app_data.spawn.child_count = child_count;
            app_data.spawn.root_repo_path = Some(conflict.repo_root.clone());

            // Call spawn_children with the task/prompt (if any)
            self.spawn_children(app_data, conflict.prompt.as_deref())
        } else {
            // Single agent creation
            self.create_agent_internal(
                app_data,
                &conflict.repo_root,
                &conflict.title,
                conflict.prompt.as_deref(),
                &conflict.branch,
                &conflict.worktree_path,
            )?;
            Ok(AppMode::normal())
        }
    }

    /// Kill the selected agent (and all its descendants)
    pub(crate) fn kill_agent(self, app_data: &mut AppData) -> Result<()> {
        if let Some(agent) = app_data.selected_agent() {
            let agent_id = agent.id;
            let is_root = agent.is_root();
            let session = agent.mux_session.clone();
            let worktree_name = agent.branch.clone();
            let window_index = agent.window_index;
            let title = agent.title.clone();
            let repo_root = agent.repo_root.clone();

            info!(
                %title,
                %agent_id,
                is_root,
                %session,
                "Killing agent"
            );

            if is_root {
                // Root agent: kill entire session and worktree
                let pane_pids = self
                    .session_manager
                    .list_pane_pids(&session)
                    .unwrap_or_default();

                // First kill all descendant windows in descending order
                // (in case any are in other sessions, and to handle renumbering)
                let descendants = app_data.storage.descendants(agent_id);
                let mut indices: Vec<u32> = descendants
                    .iter()
                    .filter_map(|desc| desc.window_index)
                    .collect();
                indices.sort_unstable_by(|a, b| b.cmp(a));
                for idx in indices {
                    let _ = self.session_manager.kill_window(&session, idx);
                }

                // Kill the session
                let _ = self.session_manager.kill(&session);

                // Ensure any remaining pane processes are terminated before removing the worktree.
                for pid in pane_pids {
                    let _ = std::process::Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }

                // Brief delay to allow mux-managed processes to terminate
                // mux kill-session sends SIGTERM and returns immediately,
                // but processes may still be running and have files open
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Remove worktree
                if let Some(repo_path) = repo_root.or_else(|| std::env::current_dir().ok())
                    && let Ok(repo) = git::open_repository(&repo_path)
                {
                    let worktree_mgr = WorktreeManager::new(&repo);
                    let delete_branch = worktree_name.starts_with(&app_data.config.branch_prefix)
                        || worktree_name.starts_with("tenex/");
                    let result = if delete_branch {
                        worktree_mgr.remove(&worktree_name)
                    } else {
                        worktree_mgr.remove_worktree_only(&worktree_name)
                    };
                    if let Err(e) = result {
                        warn!("Failed to remove worktree: {e}");
                        app_data.set_status(format!("Warning: {e}"));
                    }
                }
            } else {
                // Child agent: kill just this window and its descendants
                // Get the root's session for killing windows
                let root = app_data.storage.root_ancestor(agent_id);
                let root_session = root.map_or_else(|| session.clone(), |r| r.mux_session.clone());
                let root_id = root.map(|r| r.id);

                // Collect all window indices being deleted
                let mut deleted_indices: Vec<u32> = Vec::new();
                let descendants = app_data.storage.descendants(agent_id);
                for desc in &descendants {
                    if let Some(idx) = desc.window_index {
                        deleted_indices.push(idx);
                    }
                }

                // Add this agent's window
                if let Some(idx) = window_index {
                    deleted_indices.push(idx);
                }

                // Sort in descending order and kill windows from highest to lowest
                // This prevents window renumbering from affecting indices we haven't killed yet
                deleted_indices.sort_unstable_by(|a, b| b.cmp(a));
                for idx in &deleted_indices {
                    let _ = self.session_manager.kill_window(&root_session, *idx);
                }

                // Update window indices for remaining agents under the same root
                // When the mux renumbers windows, indices shift down
                if let Some(rid) = root_id {
                    super::window::adjust_window_indices_after_deletion(
                        app_data,
                        rid,
                        agent_id,
                        &deleted_indices,
                    );
                }
            }

            // Remove agent and all descendants from storage
            app_data.storage.remove_with_descendants(agent_id);

            app_data.validate_selection();
            app_data.storage.save()?;

            app_data.set_status("Agent killed");
        }
        Ok(())
    }

    /// Switch the root agent to a different branch.
    ///
    /// This is a restart-on-branch operation: it kills the root agent and all children, deletes the
    /// old worktree, and starts a fresh root agent in a worktree for the selected branch.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the new agent fails.
    pub fn switch_branch(self, app_data: &mut AppData) -> Result<AppMode> {
        self.try_switch_branch(app_data).or_else(|err| {
            Self::clear_switch_branch_state(app_data);
            Ok(ErrorModalMode {
                message: format!("Switch branch failed: {err:#}"),
            }
            .into())
        })
    }

    fn try_switch_branch(self, app_data: &mut AppData) -> Result<AppMode> {
        let Some(root_id) = app_data.git_op.agent_id else {
            return Ok(Self::switch_branch_user_error(
                app_data,
                "No agent selected for branch switch.",
            ));
        };

        let target_raw = app_data.git_op.target_branch.trim().to_string();
        if target_raw.is_empty() {
            return Ok(Self::switch_branch_user_error(
                app_data,
                "No target branch selected.",
            ));
        }

        let current_branch = app_data.git_op.branch_name.clone();
        if target_raw == current_branch {
            Self::clear_switch_branch_state(app_data);
            app_data.set_status(format!("Already on branch: {current_branch}"));
            return Ok(AppMode::normal());
        }

        let Some(root) = app_data.storage.get(root_id) else {
            return Ok(Self::switch_branch_user_error(
                app_data,
                "Root agent not found.",
            ));
        };

        let program = root.program.clone();
        let repo_root = root
            .repo_root
            .clone()
            .unwrap_or_else(|| root.worktree_path.clone());

        let Some(target) =
            Self::prepare_branch_switch_target(&app_data.config, &repo_root, &target_raw)?
        else {
            Self::clear_switch_branch_state(app_data);
            return Ok(ErrorModalMode {
                message: format!("Branch not found: {target_raw}"),
            }
            .into());
        };

        let title = target
            .worktree_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&target.branch)
            .to_string();

        if !target.worktree_path.exists() {
            Self::clear_switch_branch_state(app_data);
            return Ok(ErrorModalMode {
                message: format!(
                    "Worktree path does not exist: {}",
                    target.worktree_path.display()
                ),
            }
            .into());
        }

        self.kill_root_agent_tree(app_data, root_id, false)?;

        let new_id = self.spawn_root_agent_in_worktree(
            app_data,
            title,
            program,
            repo_root,
            target.branch.clone(),
            target.worktree_path,
        )?;

        app_data.select_agent_by_id(new_id);
        Self::clear_switch_branch_state(app_data);
        app_data.set_status(format!("Switched to branch: {}", target.branch));
        Ok(AppMode::normal())
    }

    fn switch_branch_user_error(app_data: &mut AppData, message: &str) -> AppMode {
        Self::clear_switch_branch_state(app_data);
        ErrorModalMode {
            message: message.to_string(),
        }
        .into()
    }

    fn clear_switch_branch_state(app_data: &mut AppData) {
        app_data.git_op.clear();
        app_data.review.clear();
    }

    fn prepare_branch_switch_target(
        config: &Config,
        repo_root: &Path,
        target_raw: &str,
    ) -> Result<Option<BranchSwitchTarget>> {
        let repo = git::open_repository(repo_root)?;
        let worktree_mgr = WorktreeManager::new(&repo);
        let branch_mgr = git::BranchManager::new(&repo);

        let Some(branch) = Self::resolve_target_branch(&repo, &branch_mgr, target_raw)? else {
            return Ok(None);
        };

        if !branch_mgr.exists(&branch) {
            return Ok(None);
        }

        let worktree_path =
            Self::ensure_worktree_for_branch(config, repo_root, &worktree_mgr, &branch)?;
        Ok(Some(BranchSwitchTarget {
            branch,
            worktree_path,
        }))
    }

    fn resolve_target_branch(
        repo: &git2::Repository,
        branch_mgr: &git::BranchManager<'_>,
        target_raw: &str,
    ) -> Result<Option<String>> {
        if branch_mgr.exists(target_raw) {
            return Ok(Some(target_raw.to_string()));
        }

        let Ok(remote_branch) = repo.find_branch(target_raw, git2::BranchType::Remote) else {
            return Ok(None);
        };

        let local_name = target_raw
            .split_once('/')
            .map_or(target_raw, |(_, name)| name)
            .to_string();

        if !branch_mgr.exists(&local_name) {
            let commit = remote_branch.get().peel_to_commit().with_context(|| {
                format!("Failed to read commit for remote branch '{target_raw}'")
            })?;

            let mut created = repo
                .branch(&local_name, &commit, false)
                .with_context(|| format!("Failed to create local branch '{local_name}'"))?;

            let _ = created.set_upstream(Some(target_raw));
        }

        Ok(Some(local_name))
    }

    fn ensure_worktree_for_branch(
        config: &Config,
        repo_root: &Path,
        worktree_mgr: &WorktreeManager<'_>,
        branch: &str,
    ) -> Result<std::path::PathBuf> {
        if worktree_mgr.exists(branch) {
            return Ok(worktree_mgr
                .worktree_path(branch)
                .unwrap_or_else(|| config.worktree_path_for_repo_root(repo_root, branch)));
        }

        let worktree_path = config.worktree_path_for_repo_root(repo_root, branch);
        worktree_mgr.create(&worktree_path, branch)?;
        Ok(worktree_path)
    }

    fn spawn_root_agent_in_worktree(
        self,
        app_data: &mut AppData,
        title: String,
        program: String,
        repo_root: std::path::PathBuf,
        branch: String,
        worktree_path: std::path::PathBuf,
    ) -> Result<Uuid> {
        let mut agent = Agent::new(title, program, branch, worktree_path);
        agent.repo_root = Some(repo_root);

        let cli = crate::conversation::detect_agent_cli(&agent.program);
        if cli == crate::conversation::AgentCli::Claude {
            agent.conversation_id = Some(agent.id.to_string());
        }
        let session_prefix = app_data.storage.instance_session_prefix();
        agent.mux_session = format!("{session_prefix}{}", agent.short_id());

        let command = crate::conversation::build_spawn_argv(
            &agent.program,
            None,
            agent.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&agent.mux_session, &agent.worktree_path, Some(&command))?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                &agent.worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&agent.mux_session, width, height);
        }

        let new_id = agent.id;
        app_data.storage.add(agent);
        app_data.storage.save()?;
        Ok(new_id)
    }

    fn kill_root_agent_tree(
        self,
        app_data: &mut AppData,
        root_id: Uuid,
        delete_branch: bool,
    ) -> Result<()> {
        let Some(root) = app_data.storage.get(root_id) else {
            return Ok(());
        };

        let session = root.mux_session.clone();
        let worktree_name = root.branch.clone();
        let repo_root = root.repo_root.clone();

        let pane_pids = self
            .session_manager
            .list_pane_pids(&session)
            .unwrap_or_default();

        // First kill all descendant windows in descending order.
        let descendants = app_data.storage.descendants(root_id);
        let mut indices: Vec<u32> = descendants
            .iter()
            .filter_map(|desc| desc.window_index)
            .collect();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for idx in indices {
            let _ = self.session_manager.kill_window(&session, idx);
        }

        // Kill the session
        let _ = self.session_manager.kill(&session);

        // Ensure any remaining pane processes are terminated before removing the worktree.
        for pid in pane_pids {
            let _ = std::process::Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }

        std::thread::sleep(std::time::Duration::from_millis(100));

        if let Some(repo_path) = repo_root.or_else(|| std::env::current_dir().ok())
            && let Ok(repo) = git::open_repository(&repo_path)
        {
            let worktree_mgr = WorktreeManager::new(&repo);
            let result = if delete_branch {
                worktree_mgr.remove(&worktree_name)
            } else {
                worktree_mgr.remove_worktree_only(&worktree_name)
            };
            if let Err(e) = result {
                warn!("Failed to remove worktree: {e}");
                app_data.set_status(format!("Warning: {e}"));
            }
        }

        app_data.storage.remove_with_descendants(root_id);
        app_data.validate_selection();
        app_data.storage.save()?;
        Ok(())
    }

    /// Spawn a new terminal (standalone shell, not a Claude agent)
    ///
    /// Terminals are spawned as children of the selected agent, in that agent's worktree.
    /// They are excluded from broadcast and can optionally have a startup command.
    ///
    /// # Errors
    ///
    /// Returns an error if terminal creation fails or no agent is selected
    pub fn spawn_terminal(
        self,
        app_data: &mut AppData,
        startup_command: Option<&str>,
    ) -> Result<AppMode> {
        // Must have a selected agent
        let selected = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        // Get the root ancestor to use its mux session
        let selected_id = selected.id;
        let root = app_data
            .storage
            .root_ancestor(selected_id)
            .ok_or_else(|| anyhow::anyhow!("Could not find root agent"))?;

        let root_session = root.mux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();
        let root_id = root.id;

        let title = app_data.spawn.next_terminal_name();
        debug!(title, startup_command, "Creating new terminal");

        // Reserve a window index
        let window_index = app_data.storage.reserve_window_indices(root_id);

        // Create child agent marked as terminal
        let mut terminal = Agent::new_child(
            title.clone(),
            "terminal".to_string(),
            branch,
            worktree_path.clone(),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index,
                repo_root: root.repo_root.clone(),
            },
        );
        terminal.is_terminal = true;
        terminal.workspace_kind = root.workspace_kind;

        // Create window in the root's session (no command - just a shell)
        let actual_index =
            self.session_manager
                .create_window(&root_session, &title, &worktree_path, None)?;

        // Resize the new window to match preview dimensions
        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let window_target = SessionManager::window_target(&root_session, actual_index);
            let _ = self
                .session_manager
                .resize_window(&window_target, width, height);
        }

        // Update window index if it differs
        terminal.window_index = Some(actual_index);

        // If a startup command was provided, send it to the terminal
        if let Some(cmd) = startup_command {
            let window_target = SessionManager::window_target(&root_session, actual_index);
            self.session_manager
                .send_keys_and_submit(&window_target, cmd)?;
        }

        app_data.storage.add(terminal);

        // Expand the parent to show the new terminal
        if let Some(parent) = app_data.storage.get_mut(root_id) {
            parent.collapsed = false;
        }

        app_data.storage.save()?;

        info!(title, "Terminal created successfully");
        app_data.set_status(format!("Created terminal: {title}"));
        Ok(AppMode::normal())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use crate::agent::Storage;
    use crate::agent::WorkspaceKind;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::{AppMode, ConfirmAction, ConfirmingMode};
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    fn init_repo() -> Result<(TempDir, std::path::PathBuf), Box<dyn std::error::Error>> {
        use git2::{Repository, RepositoryInitOptions, Signature};

        let dir = TempDir::new()?;
        let path = dir.path().to_path_buf();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");

        let repo = Repository::init_opts(&path, &init_opts)?;
        repo.set_head("refs/heads/master")?;
        {
            let mut config = repo.config()?;
            config.set_str("user.name", "Test")?;
            config.set_str("user.email", "test@test.com")?;
            config.set_str("commit.gpgsign", "false")?;
        }

        std::fs::write(path.join("README.md"), "# Test\n")?;
        let sig = Signature::now("Test", "test@test.com")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        Ok((dir, path))
    }

    #[test]
    fn test_reconnect_to_worktree_no_conflict_info() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No conflict info set - should error
        let result = handler.reconnect_to_worktree(&mut app.data);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_reconnect_to_worktree_removes_existing_agents() -> Result<(), Box<dyn std::error::Error>>
    {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let worktree = TempDir::new()?;
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();

        let existing = Agent::new(
            "asdf".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            branch.clone(),
            worktree_path.clone(),
        );
        let existing_id = existing.id;
        app.data.storage.add(existing);

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "asdf".to_string(),
            prompt: None,
            branch: branch.clone(),
            worktree_path: worktree_path.clone(),
            repo_root: std::path::PathBuf::from("/tmp"),
            existing_branch: Some("main".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        });

        let next = handler.reconnect_to_worktree(&mut app.data)?;
        assert_eq!(next, AppMode::normal());

        assert!(app.data.storage.get(existing_id).is_none());
        assert_eq!(
            app.data
                .storage
                .iter()
                .filter(|agent| agent.branch == branch && agent.worktree_path == worktree_path)
                .count(),
            1
        );

        let new_session = app
            .data
            .storage
            .iter()
            .find(|agent| agent.branch == branch)
            .ok_or("Expected new agent")?
            .mux_session
            .clone();
        let _ = crate::mux::SessionManager::new().kill(&new_session);

        Ok(())
    }

    #[test]
    fn test_create_agent_outside_git_uses_plain_dir_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        struct RestoreCwd(PathBuf);

        impl Drop for RestoreCwd {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let original_cwd = std::env::current_dir()?;
        let _guard = RestoreCwd(original_cwd);

        let workdir = TempDir::new()?;
        std::env::set_current_dir(workdir.path())?;

        let next = handler.create_agent(&mut app.data, "plain-dir-agent", None)?;
        assert_eq!(next, AppMode::normal());

        let created = app
            .data
            .storage
            .iter()
            .find(|agent| agent.title == "plain-dir-agent")
            .ok_or("Expected agent to be created")?;
        assert_eq!(created.workspace_kind, WorkspaceKind::PlainDir);

        // Stop the session to avoid leaking `sleep` processes.
        let _ = crate::mux::SessionManager::new().kill(&created.mux_session);

        Ok(())
    }

    #[test]
    fn test_reconnect_to_worktree_swarm_removes_existing_agents()
    -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let worktree = TempDir::new()?;
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();

        let root = Agent::new(
            "asdf".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            branch.clone(),
            worktree_path.clone(),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.storage.add(Agent::new_child(
            "child".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            branch.clone(),
            worktree_path.clone(),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
                repo_root: None,
            },
        ));

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "asdf".to_string(),
            prompt: Some("do stuff".to_string()),
            branch: branch.clone(),
            worktree_path: worktree_path.clone(),
            repo_root: std::path::PathBuf::from("/tmp"),
            existing_branch: Some("main".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: Some(2),
        });

        let next = handler.reconnect_to_worktree(&mut app.data)?;
        assert_eq!(next, AppMode::normal());

        assert_eq!(
            app.data
                .storage
                .iter()
                .filter(|agent| agent.branch == branch && agent.worktree_path == worktree_path)
                .count(),
            3
        );

        let new_root_session = app
            .data
            .storage
            .iter()
            .find(|agent| agent.branch == branch && agent.is_root())
            .ok_or("Expected root agent")?
            .mux_session
            .clone();
        let _ = crate::mux::SessionManager::new().kill(&new_root_session);
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Ok(())
    }

    #[test]
    fn test_recreate_worktree_no_conflict_info() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No conflict info set - should error
        let result = handler.recreate_worktree(&mut app.data);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_repo_root_invalid()
    -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let mut root = Agent::new(
            "root".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.repo_root = Some(PathBuf::from("/tmp/tenex-nonexistent-repo-root"));
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.git_op.agent_id = Some(root_id);
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "feature".to_string();
        app.data.review.filter = "m".to_string();

        let next = handler.switch_branch(&mut app.data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        assert!(app.data.review.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_target_branch_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "   ".to_string();
        app.data.review.filter = "m".to_string();

        let next = handler.switch_branch(&mut app.data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        assert!(app.data.review.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_switch_branch_noops_when_already_on_branch() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "main".to_string();

        let next = handler.switch_branch(&mut app.data)?;
        assert_eq!(next, AppMode::normal());
        assert!(
            app.data
                .ui
                .status_message
                .as_ref()
                .is_some_and(|msg| msg.contains("Already on branch: main"))
        );
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        Ok(())
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_root_agent_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "feature".to_string();

        let next = handler.switch_branch(&mut app.data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        Ok(())
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_branch_missing_in_repo()
    -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let (_repo_dir, repo_path) = init_repo()?;
        let root = Agent::new(
            "root".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            "master".to_string(),
            repo_path,
        );
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.git_op.agent_id = Some(root_id);
        app.data.git_op.branch_name = "master".to_string();
        app.data.git_op.target_branch = "branch-does-not-exist".to_string();

        let next = handler.switch_branch(&mut app.data)?;
        let AppMode::ErrorModal(modal) = next else {
            return Err("Expected error modal".into());
        };
        assert!(modal.message.contains("Branch not found"));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        Ok(())
    }

    #[test]
    fn test_ensure_worktree_for_branch_reuses_existing_worktree()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_repo_dir, repo_root) = init_repo()?;
        let worktree_dir = TempDir::new()?;

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            branch_prefix: "tenex-test/".to_string(),
            ..Config::default()
        };

        let repo = git::open_repository(&repo_root)?;
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature")?;

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = config.worktree_path_for_repo_root(&repo_root, "feature");
        worktree_mgr.create(&worktree_path, "feature")?;

        let reused =
            Actions::ensure_worktree_for_branch(&config, &repo_root, &worktree_mgr, "feature")?;
        assert!(reused.exists());
        Ok(())
    }

    #[test]
    fn test_handle_confirm_kill() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp/nonexistent"),
        ));

        // Enter confirming mode for kill
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into(),
        );

        // Confirm should kill and exit mode
        handler.handle_action(&mut app, crate::config::Action::Confirm)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_kill_agent_root() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Kill should work (session doesn't exist, but should not error)
        handler.kill_agent(&mut app.data)?;
        assert_eq!(app.data.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_kill_agent_child() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent (expanded to show children)
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        );
        app.data.storage.add(child);

        // Select the child (it's the second visible agent)
        app.select_next();

        // Kill child should remove just the child
        handler.kill_agent(&mut app.data)?;
        assert_eq!(app.data.storage.len(), 1);
        assert!(app.data.storage.get(root_id).is_some());
        Ok(())
    }

    #[test]
    fn test_kill_agent_with_descendants() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add children
        for i in 0..3 {
            app.data.storage.add(Agent::new_child(
                format!("child{i}"),
                "claude".to_string(),
                "muster/root".to_string(),
                PathBuf::from("/tmp"),
                ChildConfig {
                    parent_id: root_id,
                    mux_session: root_session.clone(),
                    window_index: i + 2,
                    repo_root: None,
                },
            ));
        }

        // Kill root should remove all
        handler.kill_agent(&mut app.data)?;
        assert_eq!(app.data.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_spawn_terminal_creates_child_of_root() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Create a root agent with a child
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp/worktree"),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp/worktree"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        );
        let child_id = child.id;
        app.data.storage.add(child);

        // Select the child (second visible agent)
        app.select_next();
        assert_eq!(app.selected_agent().map(|a| a.id), Some(child_id));

        // Spawn terminal - should fail because mux session doesn't exist
        let result = handler.spawn_terminal(&mut app.data, None);

        // Should fail because mux session doesn't exist
        assert!(result.is_err());
        Ok(())
    }
}
