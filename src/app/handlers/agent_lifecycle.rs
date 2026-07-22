//! Agent lifecycle operations: create, kill, reconnect

use crate::agent::{Agent, AgentRuntime, ChildConfig};
use crate::git::{self, WorktreeCreateOptions, WorktreeManager};
use crate::mux::SessionManager;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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
    worktree_path: PathBuf,
}

#[derive(Debug)]
struct RootLaunchSpec {
    title: String,
    program: String,
    runtime: AgentRuntime,
    repo_root: PathBuf,
    branch: String,
    worktree_path: PathBuf,
}

fn runtime_for_conflict(
    app_data: &AppData,
    conflict: &WorktreeConflictInfo,
) -> Option<AgentRuntime> {
    for agent in app_data.storage.iter() {
        if agent.branch.as_str() == conflict.branch.as_str()
            && agent.worktree_path == conflict.worktree_path
        {
            return Some(agent.runtime);
        }
    }
    None
}

impl Actions {
    pub(super) fn root_worktree_create_options(runtime: AgentRuntime) -> WorktreeCreateOptions {
        if runtime == AgentRuntime::Docker {
            WorktreeCreateOptions::without_ignored_file_links()
        } else {
            WorktreeCreateOptions::default()
        }
    }

    fn prepare_agent_for_launch(app_data: &mut AppData, agent: &mut Agent) {
        if crate::conversation::detect_agent_cli(&agent.program)
            == crate::conversation::AgentCli::Claude
            && agent.conversation_id.is_none()
        {
            agent.conversation_id = Some(agent.id.to_string());
        }

        if agent.is_root() {
            let session_prefix = app_data.storage.instance_session_prefix();
            agent.mux_session = format!("{session_prefix}{}", agent.short_id());
            if agent.runtime == AgentRuntime::Docker && agent.runtime_scope.is_empty() {
                agent.runtime_scope = format!("root-{}", agent.id.simple());
            }
        }
    }

    fn finish_agent_launch(app_data: &AppData, agent: &mut Agent, started_at: SystemTime) {
        if crate::conversation::detect_agent_cli(&agent.program)
            == crate::conversation::AgentCli::Codex
        {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                &crate::runtime::codex_session_workdir(agent),
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }
    }

    fn resize_target_to_preview(self, app_data: &mut AppData, target: &str) {
        if let Some((width, height)) = app_data.ui.preview_dimensions {
            if width == 0 || height == 0 {
                warn!(
                    target,
                    width, height, "Skipping zero-sized agent preview resize"
                );
                app_data.set_status(format!(
                    "Preview is too small to resize agent: {width}x{height}"
                ));
                return;
            }

            if let Err(err) = self.session_manager.resize_window(target, width, height) {
                warn!(
                    target,
                    width,
                    height,
                    error = %err,
                    "Failed to resize agent preview"
                );
                app_data.set_status(format!("Failed to resize agent preview: {err}"));
            }
        }
    }

    pub(crate) fn launch_root_agent(
        self,
        app_data: &mut AppData,
        agent: &mut Agent,
        prompt: Option<&str>,
    ) -> Result<()> {
        Self::prepare_agent_for_launch(app_data, agent);
        crate::runtime::ensure_runtime_ready(agent, &app_data.settings)?;
        let command = crate::runtime::build_agent_command(
            agent,
            crate::runtime::AgentLaunch::Spawn { prompt },
            &app_data.settings,
        );
        let command = command?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&agent.mux_session, &agent.worktree_path, Some(&command))?;
        Self::finish_agent_launch(app_data, agent, started_at);
        self.resize_target_to_preview(app_data, &agent.mux_session);
        Ok(())
    }

    pub(crate) fn launch_child_agent(
        self,
        app_data: &mut AppData,
        agent: &mut Agent,
        title: &str,
        prompt: Option<&str>,
    ) -> Result<u32> {
        crate::runtime::ensure_runtime_ready(agent, &app_data.settings)?;
        let command = crate::runtime::build_agent_command(
            agent,
            crate::runtime::AgentLaunch::Spawn { prompt },
            &app_data.settings,
        );
        let command = command?;
        let started_at = SystemTime::now();
        let actual_index = self.session_manager.create_window(
            &agent.mux_session,
            title,
            &agent.worktree_path,
            Some(&command),
        );
        let actual_index = actual_index?;
        Self::finish_agent_launch(app_data, agent, started_at);
        let target = SessionManager::window_target(&agent.mux_session, actual_index);
        self.resize_target_to_preview(app_data, &target);
        Ok(actual_index)
    }

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

        let current_dir = std::env::current_dir().ok();
        let repo_path = app_data
            .selected_project_root()
            .or_else(|| app_data.cwd_project_root.clone())
            .or(current_dir)
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

        let target_preparation = worktree_mgr.prepare_worktree_creation_target(
            &worktree_path,
            &branch,
            &app_data.config.worktree_dir_for_repo_root(&repo_path),
        )?;

        if let Some(conflict_worktree_path) = target_preparation.registered_path() {
            debug!(branch, "Worktree already exists, prompting user");

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
                worktree_path: conflict_worktree_path.to_path_buf(),
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

        let cleaned_stale_target = target_preparation.cleaned_stale_target();
        self.create_agent_internal(app_data, &repo_path, title, prompt, &branch, &worktree_path)?;
        if cleaned_stale_target {
            app_data.set_status(format!("Cleaned stale worktree and created agent: {title}"));
        }
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

        let mut agent = Agent::new(title.to_string(), program, branch, workdir.to_path_buf());
        agent.workspace_kind = crate::agent::WorkspaceKind::PlainDir;
        agent.repo_root = Some(workdir.to_path_buf());
        agent.runtime = crate::runtime::new_root_runtime(&app_data.settings);
        self.launch_root_agent(app_data, &mut agent, prompt)?;

        let agent_id = agent.id;
        app_data.storage.add(agent);
        app_data.storage.save()?;
        app_data.select_agent_by_id(agent_id);

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
        let runtime = crate::runtime::new_root_runtime(&app_data.settings);
        let target_preparation = worktree_mgr.prepare_worktree_creation_target(
            worktree_path,
            branch,
            &app_data.config.worktree_dir_for_repo_root(repo_path),
        )?;
        if let Some(registered_path) = target_preparation.registered_path() {
            bail!(
                "Cannot create worktree for branch '{branch}' because a registered worktree already exists at {}",
                registered_path.display()
            );
        }

        let created = worktree_mgr.create_with_new_branch_with_options(
            worktree_path,
            branch,
            Self::root_worktree_create_options(runtime),
        );
        created?;

        let program = app_data.agent_spawn_command();
        let mut agent = Agent::new(
            title.to_string(),
            program,
            branch.to_string(),
            worktree_path.to_path_buf(),
        );
        agent.repo_root = Some(repo_path.to_path_buf());
        agent.runtime = runtime;
        self.launch_root_agent(app_data, &mut agent, prompt)?;

        let agent_id = agent.id;
        app_data.storage.add(agent);
        app_data.storage.save()?;
        app_data.select_agent_by_id(agent_id);

        info!(title, %branch, "Agent created successfully");
        if target_preparation.cleaned_stale_target() {
            app_data.set_status(format!("Cleaned stale worktree and created agent: {title}"));
        } else {
            app_data.set_status(format!("Created agent: {title}"));
        }
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
        let runtime = runtime_for_conflict(app_data, &conflict)
            .unwrap_or_else(|| crate::runtime::new_root_runtime(&app_data.settings));

        self.remove_conflicting_agents(app_data, &conflict);

        if let Some(child_count) = conflict.swarm_child_count {
            self.reconnect_swarm_to_worktree(app_data, &conflict, &program, runtime, child_count)?;
        } else {
            self.reconnect_single_to_worktree(app_data, &conflict, &program, runtime)?;
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
        let mut runtime_agents_by_session: HashMap<String, Agent> = HashMap::new();

        for agent in app_data.storage.iter() {
            if agent.branch != conflict.branch {
                continue;
            }

            if agent.worktree_path == conflict.worktree_path {
                ids_to_remove.push(agent.id);
                runtime_agents_by_session
                    .entry(agent.mux_session.clone())
                    .or_insert_with(|| agent.clone());
            }
        }

        let removed = ids_to_remove.len();
        let ids_set: HashSet<Uuid> = ids_to_remove.iter().copied().collect();

        for (session, runtime_agent) in runtime_agents_by_session {
            let session_used_elsewhere = app_data.storage.iter().any(|agent| {
                if agent.mux_session != session {
                    return false;
                }

                !ids_set.contains(&agent.id)
            });

            if !session_used_elsewhere {
                let _ = self.session_manager.kill(&session);
                if let Err(err) = crate::runtime::cleanup_runtime(&runtime_agent) {
                    warn!(
                        session = %session,
                        error = %err,
                        "Failed to clean up runtime for conflicting agent"
                    );
                }
            }
        }

        for id in ids_to_remove {
            app_data.storage.remove(id);
        }
        app_data.validate_selection();
        removed
    }

    fn reconnect_swarm_to_worktree(
        self,
        app_data: &mut AppData,
        conflict: &WorktreeConflictInfo,
        program: &str,
        runtime: AgentRuntime,
        child_count: usize,
    ) -> Result<()> {
        let mut root_agent = Agent::new(
            conflict.title.clone(),
            program.to_string(),
            conflict.branch.clone(),
            conflict.worktree_path.clone(),
        );
        root_agent.repo_root = Some(conflict.repo_root.clone());
        root_agent.runtime = runtime;

        self.launch_root_agent(app_data, &mut root_agent, None)?;

        let root_session = root_agent.mux_session.clone();
        let root_id = root_agent.id;

        app_data.storage.add(root_agent);

        let task = conflict.prompt.as_deref().unwrap_or("");
        let spawn_config = SpawnConfig {
            root_session,
            worktree_path: conflict.worktree_path.clone(),
            branch: conflict.branch.clone(),
            workspace_kind: crate::agent::WorkspaceKind::GitWorktree,
            runtime,
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
        runtime: AgentRuntime,
    ) -> Result<()> {
        let mut agent = Agent::new(
            conflict.title.clone(),
            program.to_string(),
            conflict.branch.clone(),
            conflict.worktree_path.clone(),
        );
        agent.repo_root = Some(conflict.repo_root.clone());
        agent.runtime = runtime;
        self.launch_root_agent(app_data, &mut agent, conflict.prompt.as_deref())?;

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
            let created = self.create_agent_internal(
                app_data,
                &conflict.repo_root,
                &conflict.title,
                conflict.prompt.as_deref(),
                &conflict.branch,
                &conflict.worktree_path,
            );
            created?;
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

            info!(
                %title,
                %agent_id,
                is_root,
                %session,
                "Killing agent"
            );

            if is_root {
                let delete_branch = worktree_name.starts_with(&app_data.config.branch_prefix)
                    || worktree_name.starts_with("tenex/");
                self.kill_root_agent_tree(app_data, agent_id, delete_branch)?;
                app_data.set_status("Agent killed");
                return Ok(());
            }

            // Child agent: kill just this window and its descendants
            // Get the root's session for killing windows
            let root = app_data.storage.root_ancestor(agent_id).unwrap_or(agent);
            let root_session = root.mux_session.clone();
            let root_id = root.id;

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
            super::window::adjust_window_indices_after_deletion(
                app_data,
                root_id,
                agent_id,
                &deleted_indices,
            );

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
        let runtime = root.runtime;
        let repo_root = root
            .repo_root
            .clone()
            .unwrap_or_else(|| root.worktree_path.clone());

        let Some(target) =
            Self::prepare_branch_switch_target(&app_data.config, &repo_root, &target_raw, runtime)?
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
            RootLaunchSpec {
                title,
                program,
                runtime,
                repo_root,
                branch: target.branch.clone(),
                worktree_path: target.worktree_path,
            },
        );
        let new_id = new_id?;

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
        runtime: AgentRuntime,
    ) -> Result<Option<BranchSwitchTarget>> {
        let repo = git::open_repository(repo_root)?;
        let worktree_mgr = WorktreeManager::new(&repo);
        let branch_mgr = git::BranchManager::new(&repo);

        let Some(branch) = Self::resolve_target_branch(&repo, &branch_mgr, target_raw)? else {
            return Ok(None);
        };

        let worktree_path =
            Self::ensure_worktree_for_branch(config, repo_root, &worktree_mgr, &branch, runtime)?;
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
        runtime: AgentRuntime,
    ) -> Result<std::path::PathBuf> {
        let worktree_path = config.worktree_path_for_repo_root(repo_root, branch);
        let target_preparation = worktree_mgr.prepare_worktree_creation_target(
            &worktree_path,
            branch,
            &config.worktree_dir_for_repo_root(repo_root),
        )?;
        if let Some(path) = target_preparation.registered_path() {
            return Ok(path.to_path_buf());
        }

        let created = worktree_mgr.create_with_options(
            &worktree_path,
            branch,
            Self::root_worktree_create_options(runtime),
        );
        created?;
        Ok(worktree_path)
    }

    fn spawn_root_agent_in_worktree(
        self,
        app_data: &mut AppData,
        spec: RootLaunchSpec,
    ) -> Result<Uuid> {
        let mut agent = Agent::new(spec.title, spec.program, spec.branch, spec.worktree_path);
        agent.repo_root = Some(spec.repo_root);
        agent.runtime = spec.runtime;
        self.launch_root_agent(app_data, &mut agent, None)?;

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
        let runtime_agent = root.clone();

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

        if let Err(err) = crate::runtime::cleanup_runtime(&runtime_agent) {
            warn!(session = %session, error = %err, "Failed to clean up agent runtime");
        }

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
            .unwrap_or(selected);

        let root_session = root.mux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();
        let root_id = root.id;
        let repo_root = root.repo_root.clone();
        let workspace_kind = root.workspace_kind;
        let runtime = root.runtime;
        let runtime_scope = root.effective_runtime_scope().to_string();

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
                repo_root,
            },
        );
        terminal.is_terminal = true;
        terminal.workspace_kind = workspace_kind;
        terminal.runtime = runtime;
        terminal.runtime_scope = runtime_scope;

        crate::runtime::ensure_runtime_ready(&terminal, &app_data.settings)?;
        let terminal_command =
            crate::runtime::build_terminal_command(&terminal, startup_command, &app_data.settings);
        let actual_index = self.session_manager.create_window(
            &root_session,
            &title,
            &worktree_path,
            terminal_command.as_deref(),
        );
        let actual_index = actual_index?;
        let window_target = SessionManager::window_target(&root_session, actual_index);
        self.resize_target_to_preview(app_data, &window_target);

        // Update window index if it differs
        terminal.window_index = Some(actual_index);

        // Host terminals still use the default shell and receive startup commands after launch.
        if terminal_command.is_none()
            && let Some(cmd) = startup_command
        {
            // Best-effort convenience: if input fails to send, still keep the terminal.
            let _ = self
                .session_manager
                .send_keys_and_submit(&window_target, cmd);
        }

        app_data.storage.add(terminal);

        // Expand the parent to show the new terminal.
        app_data.storage.set_collapsed(root_id, false);

        app_data.storage.save()?;

        info!(title, "Terminal created successfully");
        app_data.set_status(format!("Created terminal: {title}"));
        Ok(AppMode::normal())
    }
}
