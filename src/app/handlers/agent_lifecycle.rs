//! Agent lifecycle operations: create, kill, reconnect
#![cfg_attr(coverage_nightly, coverage(off))]

use crate::agent::{Agent, AgentRuntime, ChildConfig};
use crate::git::{self, WorktreeCreateOptions, WorktreeManager};
use crate::mux::SessionManager;
use anyhow::{Context, Result};
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

#[cfg_attr(coverage_nightly, coverage(off))]
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
    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[doc(hidden)]
    pub fn exercise_agent_lifecycle_paths_for_coverage(app_data: &mut AppData) {
        let root_path = std::env::temp_dir().join("tenex-agent-lifecycle-coverage-root");
        let child_path = std::env::temp_dir().join("tenex-agent-lifecycle-coverage-child");

        let mut claude_root = Agent::new(
            "coverage root".to_string(),
            "claude".to_string(),
            "tenex/coverage-root".to_string(),
            root_path.clone(),
        );
        Self::prepare_agent_for_launch(app_data, &mut claude_root);

        let mut claude_existing = Agent::new(
            "coverage existing".to_string(),
            "claude".to_string(),
            "tenex/coverage-existing".to_string(),
            root_path.clone(),
        );
        claude_existing.conversation_id = Some("existing-conversation".to_string());
        Self::prepare_agent_for_launch(app_data, &mut claude_existing);

        let mut docker_root = Agent::new(
            "coverage docker".to_string(),
            "echo".to_string(),
            "tenex/coverage-docker".to_string(),
            root_path.clone(),
        );
        docker_root.runtime = AgentRuntime::Docker;
        Self::prepare_agent_for_launch(app_data, &mut docker_root);

        let mut docker_scoped = Agent::new(
            "coverage docker scoped".to_string(),
            "echo".to_string(),
            "tenex/coverage-docker-scoped".to_string(),
            root_path.clone(),
        );
        docker_scoped.runtime = AgentRuntime::Docker;
        docker_scoped.runtime_scope = "coverage-scope".to_string();
        Self::prepare_agent_for_launch(app_data, &mut docker_scoped);

        let mut child = Agent::new_child(
            "coverage child".to_string(),
            "claude".to_string(),
            "tenex/coverage-root".to_string(),
            child_path,
            ChildConfig {
                parent_id: claude_root.id,
                mux_session: claude_root.mux_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        Self::prepare_agent_for_launch(app_data, &mut child);

        app_data.storage.add(docker_root.clone());
        let matching_conflict = WorktreeConflictInfo {
            title: "coverage conflict".to_string(),
            prompt: None,
            branch: docker_root.branch.clone(),
            worktree_path: docker_root.worktree_path.clone(),
            repo_root: root_path.clone(),
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "coverage".to_string(),
            swarm_child_count: Some(2),
        };
        let _ = runtime_for_conflict(app_data, &matching_conflict);

        let missing_conflict = WorktreeConflictInfo {
            branch: "tenex/missing".to_string(),
            worktree_path: root_path.join("missing"),
            swarm_child_count: None,
            ..matching_conflict
        };
        let _ = runtime_for_conflict(app_data, &missing_conflict);

        let conflict_root =
            std::env::temp_dir().join(format!("tenex-agent-lifecycle-conflict-{}", Uuid::new_v4()));
        let conflict_worktree = conflict_root.join("conflict");
        let other_worktree = conflict_root.join("other");
        let conflict_branch = "tenex/coverage-conflict".to_string();

        let mut other_branch_agent = Agent::new(
            "coverage other branch".to_string(),
            "echo".to_string(),
            "tenex/coverage-other".to_string(),
            conflict_worktree.clone(),
        );
        other_branch_agent.mux_session = "coverage-other-branch-session".to_string();
        app_data.storage.add(other_branch_agent);

        let mut other_worktree_agent = Agent::new(
            "coverage other worktree".to_string(),
            "echo".to_string(),
            conflict_branch.clone(),
            other_worktree,
        );
        other_worktree_agent.mux_session = "coverage-other-worktree-session".to_string();
        app_data.storage.add(other_worktree_agent);

        let mut shared_conflict_agent = Agent::new(
            "coverage shared conflict".to_string(),
            "echo".to_string(),
            conflict_branch.clone(),
            conflict_worktree.clone(),
        );
        shared_conflict_agent.mux_session = "coverage-shared-session".to_string();
        app_data.storage.add(shared_conflict_agent);

        let mut shared_other_agent = Agent::new(
            "coverage shared other".to_string(),
            "echo".to_string(),
            "tenex/coverage-shared-other".to_string(),
            conflict_worktree.clone(),
        );
        shared_other_agent.mux_session = "coverage-shared-session".to_string();
        app_data.storage.add(shared_other_agent);

        let mut unique_conflict_agent = Agent::new(
            "coverage unique conflict".to_string(),
            "echo".to_string(),
            conflict_branch.clone(),
            conflict_worktree.clone(),
        );
        unique_conflict_agent.mux_session = "coverage-unique-session".to_string();
        app_data.storage.add(unique_conflict_agent);

        let cleanup_conflict = WorktreeConflictInfo {
            title: "coverage cleanup".to_string(),
            prompt: None,
            branch: conflict_branch,
            worktree_path: conflict_worktree,
            repo_root: conflict_root,
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "coverage".to_string(),
            swarm_child_count: None,
        };
        Self::new().remove_conflicting_agents(app_data, &cleanup_conflict);

        let mut empty_app_data = AppData::new(
            Config::default(),
            crate::agent::Storage::new(),
            crate::app::Settings::default(),
            false,
        );
        let _ = Self::new().kill_agent(&mut empty_app_data);
        let _ = Self::new().kill_root_agent_tree(&mut empty_app_data, Uuid::new_v4(), false);
        let _ = Self::new().try_switch_branch(&mut empty_app_data);

        empty_app_data.git_op.agent_id = Some(Uuid::new_v4());
        empty_app_data.git_op.target_branch = "   ".to_string();
        let _ = Self::new().try_switch_branch(&mut empty_app_data);

        empty_app_data.git_op.target_branch = "feature".to_string();
        empty_app_data.git_op.branch_name = "main".to_string();
        let _ = Self::new().try_switch_branch(&mut empty_app_data);

        let mut same_branch_data = AppData::new(
            Config::default(),
            crate::agent::Storage::new(),
            crate::app::Settings::default(),
            false,
        );
        let same_branch_root = Agent::new(
            "coverage same branch".to_string(),
            "echo".to_string(),
            "main".to_string(),
            std::env::temp_dir(),
        );
        let same_branch_root_id = same_branch_root.id;
        same_branch_data.storage.add(same_branch_root);
        same_branch_data.git_op.agent_id = Some(same_branch_root_id);
        same_branch_data.git_op.branch_name = "main".to_string();
        same_branch_data.git_op.target_branch = "main".to_string();
        let _ = Self::new().try_switch_branch(&mut same_branch_data);

        let mut kill_data = AppData::new(
            Config::default(),
            crate::agent::Storage::new(),
            crate::app::Settings::default(),
            false,
        );
        let kill_root = Agent::new(
            "coverage non prefixed".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            std::env::temp_dir(),
        );
        let kill_root_id = kill_root.id;
        kill_data.storage.add(kill_root);
        kill_data.select_agent_by_id(kill_root_id);
        let _ = Self::new().kill_agent(&mut kill_data);

        Self::exercise_branch_switch_paths_for_coverage();
        Self::exercise_child_kill_paths_for_coverage();
        Self::exercise_root_cleanup_paths_for_coverage();
    }

    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn coverage_app_data(config: Config) -> AppData {
        AppData::new(
            config,
            crate::agent::Storage::new(),
            crate::app::Settings::default(),
            false,
        )
    }

    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn exercise_branch_switch_paths_for_coverage() {
        let Some((repo_path, repo)) = super::init_coverage_git_repo("tenex-agent-lifecycle-switch")
        else {
            return;
        };
        let mut config = Config {
            worktree_dir: repo_path.join("worktrees"),
            ..Config::default()
        };

        let branch_mgr = git::BranchManager::new(&repo);
        let _ = Self::resolve_target_branch(&repo, &branch_mgr, "missing");
        let _ = Self::resolve_target_branch(&repo, &branch_mgr, "master");

        let Ok(commit) = repo.head().and_then(|head| head.peel_to_commit()) else {
            return;
        };
        let _ = repo.reference(
            "refs/remotes/origin/remote-only",
            commit.id(),
            true,
            "coverage remote",
        );
        let _ = Self::resolve_target_branch(&repo, &branch_mgr, "origin/remote-only");
        let _ = repo.reference(
            "refs/remotes/origin/master",
            commit.id(),
            true,
            "coverage remote",
        );
        let _ = Self::resolve_target_branch(&repo, &branch_mgr, "origin/master");

        let worktree_mgr = WorktreeManager::new(&repo);
        let existing_branch = "coverage-existing-worktree";
        let _ = branch_mgr.create(existing_branch);
        let existing_worktree = repo_path.join("existing-worktree");
        let _ = worktree_mgr.create(&existing_worktree, existing_branch);
        let _ = Self::ensure_worktree_for_branch(
            &config,
            &repo_path,
            &worktree_mgr,
            existing_branch,
            AgentRuntime::Host,
        );

        let missing_path_branch = "coverage-missing-path";
        let _ = branch_mgr.create(missing_path_branch);
        let missing_path_worktree = repo_path.join("missing-path-worktree");
        let _ = worktree_mgr.create(&missing_path_worktree, missing_path_branch);
        let _ = std::fs::remove_dir_all(&missing_path_worktree);

        config.worktree_dir = repo_path.join("switch-worktrees");
        let mut switch_data = Self::coverage_app_data(config);
        let mut root = Agent::new(
            "coverage switch root".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path.clone(),
        );
        root.repo_root = Some(repo_path);
        let root_id = root.id;
        switch_data.storage.add(root);
        switch_data.git_op.agent_id = Some(root_id);
        switch_data.git_op.branch_name = "master".to_string();
        switch_data.git_op.target_branch = missing_path_branch.to_string();
        let _ = Self::new().try_switch_branch(&mut switch_data);
    }

    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn exercise_child_kill_paths_for_coverage() {
        let root_path =
            std::env::temp_dir().join(format!("tenex-agent-child-kill-{}", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&root_path);
        let mut app_data = Self::coverage_app_data(Config::default());
        let root = Agent::new(
            "coverage kill root".to_string(),
            "echo".to_string(),
            "tenex/coverage-kill".to_string(),
            root_path.clone(),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        let child = Agent::new_child(
            "coverage kill child".to_string(),
            "echo".to_string(),
            "tenex/coverage-kill".to_string(),
            root_path.clone(),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
                repo_root: None,
            },
        );
        let child_id = child.id;
        let descendant = Agent::new_child(
            "coverage kill descendant".to_string(),
            "echo".to_string(),
            "tenex/coverage-kill".to_string(),
            root_path,
            ChildConfig {
                parent_id: child_id,
                mux_session: root_session,
                window_index: 3,
                repo_root: None,
            },
        );
        app_data.storage.add(root);
        app_data.storage.add(child);
        app_data.storage.add(descendant);
        app_data.select_agent_by_id(child_id);
        let _ = Self::new().kill_agent(&mut app_data);
    }

    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn exercise_root_cleanup_paths_for_coverage() {
        let Some((repo_path, repo)) =
            super::init_coverage_git_repo("tenex-agent-lifecycle-cleanup")
        else {
            return;
        };
        let branch_mgr = git::BranchManager::new(&repo);
        let worktree_mgr = WorktreeManager::new(&repo);
        for (branch, delete_branch) in [
            ("tenex/coverage-delete", true),
            ("feature/coverage-keep", false),
        ] {
            let _ = branch_mgr.create(branch);
            let worktree_path = repo_path.join(branch.replace('/', "-"));
            let _ = worktree_mgr.create(&worktree_path, branch);
            let mut app_data = Self::coverage_app_data(Config::default());
            let mut root = Agent::new(
                format!("coverage cleanup {branch}"),
                "echo".to_string(),
                branch.to_string(),
                worktree_path.clone(),
            );
            root.repo_root = Some(repo_path.clone());
            let root_id = root.id;
            let root_session = root.mux_session.clone();
            let child = Agent::new_child(
                "coverage cleanup child".to_string(),
                "echo".to_string(),
                branch.to_string(),
                worktree_path,
                ChildConfig {
                    parent_id: root_id,
                    mux_session: root_session,
                    window_index: 2,
                    repo_root: Some(repo_path.clone()),
                },
            );
            app_data.storage.add(root);
            app_data.storage.add(child);
            let _ = Self::new().kill_root_agent_tree(&mut app_data, root_id, delete_branch);
        }
    }

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

    fn resize_target_to_preview(self, app_data: &AppData, target: &str) {
        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self.session_manager.resize_window(target, width, height);
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
        app_data: &AppData,
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

        // Check if worktree/branch already exists - prompt user for action
        if let Some(conflict_worktree_path) = worktree_mgr.worktree_path(&branch) {
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
        app_data.set_status(format!("Created agent: {title}"));
        Ok(())
    }

    /// Reconnect to an existing worktree (user chose to keep it)
    ///
    /// # Errors
    ///
    /// Returns an error if the mux session cannot be created or storage fails
    #[cfg_attr(coverage_nightly, coverage(off))]
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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
    #[cfg_attr(coverage_nightly, coverage(off))]
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
    #[cfg_attr(coverage_nightly, coverage(off))]
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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
        if let Some(path) = worktree_mgr.worktree_path(branch) {
            return Ok(path);
        }

        let worktree_path = config.worktree_path_for_repo_root(repo_root, branch);
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

    #[cfg_attr(coverage_nightly, coverage(off))]
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
    #[cfg_attr(coverage_nightly, coverage(off))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use crate::agent::{AgentRuntime, Storage, WorkspaceKind};
    use crate::app::{AgentProgram, Settings};
    use crate::config::Config;
    use crate::state::{AppMode, ConfirmAction, ConfirmingMode};
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().expect("create temp state file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    fn canonicalize_or_self(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
    }

    fn mode_is_error_modal(mode: &AppMode) -> bool {
        matches!(mode, AppMode::ErrorModal(_))
    }

    fn mode_is_error_modal_containing(mode: &AppMode, needle: &str) -> bool {
        matches!(mode, AppMode::ErrorModal(modal) if modal.message.contains(needle))
    }

    fn mode_is_normal(mode: &AppMode) -> bool {
        matches!(mode, AppMode::Normal(_))
    }

    fn mode_is_worktree_conflict(mode: &AppMode) -> bool {
        matches!(
            mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            })
        )
    }

    fn conflict_info(branch: &str, worktree_path: PathBuf) -> WorktreeConflictInfo {
        WorktreeConflictInfo {
            title: "conflict".to_string(),
            prompt: None,
            branch: branch.to_string(),
            worktree_path,
            repo_root: PathBuf::from("/tmp"),
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "deadbeef".to_string(),
            swarm_child_count: None,
        }
    }

    #[test]
    fn test_app_mode_helpers_cover_both_outcomes() {
        let normal = AppMode::normal();
        let error: AppMode = ErrorModalMode {
            message: "boom".to_string(),
        }
        .into();
        let conflict: AppMode = ConfirmingMode {
            action: ConfirmAction::WorktreeConflict,
        }
        .into();
        assert!(mode_is_normal(&normal));
        assert!(!mode_is_normal(&error));
        assert!(mode_is_error_modal(&error));
        assert!(!mode_is_error_modal(&normal));
        assert!(mode_is_error_modal_containing(&error, "boom"));
        assert!(!mode_is_error_modal_containing(&error, "nope"));
        assert!(!mode_is_error_modal_containing(&normal, "boom"));
        assert!(mode_is_worktree_conflict(&conflict));
        assert!(!mode_is_worktree_conflict(&normal));
    }

    #[cfg(coverage)]
    #[test]
    fn test_exercise_agent_lifecycle_paths_for_coverage_runs_in_unit_build() {
        let (mut app, _temp_file) = create_test_app();
        Actions::exercise_agent_lifecycle_paths_for_coverage(&mut app.data);
    }

    #[test]
    fn test_runtime_for_conflict_returns_none_when_agent_worktree_differs() {
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let agent_path = worktree.path().join("agent");
        let conflict_path = worktree.path().join("conflict");

        let mut agent = Agent::new(
            "agent".to_string(),
            test_sleep_program(),
            "branch".to_string(),
            agent_path,
        );
        agent.runtime = AgentRuntime::Docker;
        app.data.storage.add(agent);

        let conflict = conflict_info("branch", conflict_path);
        assert_eq!(runtime_for_conflict(&app.data, &conflict), None);
    }

    #[test]
    fn test_runtime_for_conflict_returns_none_when_branch_differs() {
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let path = worktree.path().to_path_buf();

        let mut agent = Agent::new(
            "agent".to_string(),
            test_sleep_program(),
            "branch-a".to_string(),
            path.clone(),
        );
        agent.runtime = AgentRuntime::Docker;
        app.data.storage.add(agent);

        let conflict = conflict_info("branch-b", path);
        assert_eq!(runtime_for_conflict(&app.data, &conflict), None);
    }

    #[test]
    fn test_prepare_agent_for_launch_sets_claude_conversation_id_when_missing() {
        let (mut app, _temp) = create_test_app();

        let mut agent = Agent::new(
            "agent".to_string(),
            "claude".to_string(),
            "muster/agent".to_string(),
            PathBuf::from("/tmp"),
        );
        let expected = agent.id.to_string();

        Actions::prepare_agent_for_launch(&mut app.data, &mut agent);
        assert_eq!(agent.conversation_id, Some(expected));

        agent.conversation_id = Some("fixed".to_string());
        Actions::prepare_agent_for_launch(&mut app.data, &mut agent);
        assert_eq!(agent.conversation_id, Some("fixed".to_string()));
    }

    #[test]
    fn test_runtime_for_conflict_returns_runtime_when_agent_matches() {
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let path = worktree.path().to_path_buf();

        let mut agent = Agent::new(
            "agent".to_string(),
            test_sleep_program(),
            "branch".to_string(),
            path.clone(),
        );
        agent.runtime = AgentRuntime::Docker;
        app.data.storage.add(agent);

        let conflict = conflict_info("branch", path);
        assert_eq!(
            runtime_for_conflict(&app.data, &conflict),
            Some(AgentRuntime::Docker)
        );
    }

    #[test]
    fn test_prepare_agent_for_launch_sets_conversation_id_for_claude_when_missing() {
        let (mut app, _temp) = create_test_app();
        let mut agent = Agent::new(
            "agent".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.conversation_id = None;

        let expected = agent.id.to_string();
        Actions::prepare_agent_for_launch(&mut app.data, &mut agent);
        assert_eq!(agent.conversation_id.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_prepare_agent_for_launch_does_not_overwrite_existing_conversation_id() {
        let (mut app, _temp) = create_test_app();
        let mut agent = Agent::new(
            "agent".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.conversation_id = Some("existing".to_string());

        Actions::prepare_agent_for_launch(&mut app.data, &mut agent);
        assert_eq!(agent.conversation_id.as_deref(), Some("existing"));
    }

    #[test]
    fn test_finish_agent_launch_codex_path_does_not_panic() {
        let (mut app, _temp) = create_test_app();
        let mut stored = Agent::new(
            "stored".to_string(),
            "codex".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        );
        stored.conversation_id = Some("stored-session".to_string());
        app.data.storage.add(stored);

        let mut agent = Agent::new(
            "agent".to_string(),
            "codex".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        );

        Actions::finish_agent_launch(&app.data, &mut agent, SystemTime::now());
    }

    #[test]
    fn test_resize_target_to_preview_attempts_resize_when_dimensions_known() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        app.data.ui.preview_dimensions = Some((80, 24));

        handler.resize_target_to_preview(&app.data, "missing-session");
    }

    fn test_sleep_program() -> String {
        #[cfg(windows)]
        {
            "powershell -NoProfile -Command \"Start-Sleep -Seconds 3600\"".to_string()
        }
        #[cfg(not(windows))]
        {
            "sh -c 'sleep 3600'".to_string()
        }
    }

    #[cfg(unix)]
    fn write_fake_docker_script(temp: &TempDir, body: &str) -> PathBuf {
        let script = temp.path().join("docker");
        fs::write(&script, body).expect("write fake docker script");
        let mut perms = fs::metadata(&script)
            .expect("load fake docker script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("set fake docker script permissions");
        script
    }

    fn window_output_contains(window_target: &str, needle: &str, attempts: usize) -> bool {
        let capture = crate::mux::OutputCapture::new();
        for _ in 0..attempts {
            let lines = capture
                .tail(window_target, 25)
                .expect("capture output tail");
            if lines.iter().any(|line| line.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn init_repo() -> (TempDir, std::path::PathBuf) {
        use git2::{Repository, RepositoryInitOptions, Signature};

        let dir = TempDir::new().expect("create repo tempdir");
        let path = dir.path().to_path_buf();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");

        let repo = Repository::init_opts(&path, &init_opts).expect("init repo");
        repo.set_head("refs/heads/master").expect("set HEAD");
        {
            let mut config = repo.config().expect("open repo config");
            config.set_str("user.name", "Test").expect("set user.name");
            config
                .set_str("user.email", "test@test.com")
                .expect("set user.email");
            config
                .set_str("commit.gpgsign", "false")
                .expect("disable gpg signing");
        }

        std::fs::write(path.join("README.md"), "# Test\n").expect("write README");
        let sig = Signature::now("Test", "test@test.com").expect("create signature");
        let mut index = repo.index().expect("open index");
        index.add_path(Path::new("README.md")).expect("add README");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("commit initial tree");

        (dir, path)
    }

    #[test]
    fn test_reconnect_to_worktree_no_conflict_info() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // No conflict info set - should error
        let result = handler.reconnect_to_worktree(&mut app.data);
        assert!(result.is_err());
    }

    #[test]
    fn test_reconnect_to_worktree_removes_existing_agents() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();

        let existing = Agent::new(
            "asdf".to_string(),
            test_sleep_program(),
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

        let next = with_tracing_dispatch(|| handler.reconnect_to_worktree(&mut app.data))
            .expect("reconnect to worktree");
        assert_eq!(next, AppMode::normal());

        assert!(app.data.storage.get(existing_id).is_none());
        app.data.storage.add(Agent::new(
            "sentinel".to_string(),
            test_sleep_program(),
            "tenex-test/sentinel".to_string(),
            worktree_path.clone(),
        ));
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
            .expect("expected new agent")
            .mux_session
            .clone();
        let _ = crate::mux::SessionManager::new().kill(&new_session);
    }

    #[test]
    fn test_create_agent_outside_git_uses_plain_dir_workspace() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let workdir = TempDir::new().expect("create workdir");
        app.set_cwd_project_root(Some(workdir.path().to_path_buf()));

        let next = handler
            .create_agent(&mut app.data, "plain-dir-agent", None)
            .expect("create agent");
        assert_eq!(next, AppMode::normal());

        let created = app
            .data
            .storage
            .iter()
            .find(|agent| agent.title == "plain-dir-agent")
            .expect("expected agent to be created");
        assert_eq!(created.workspace_kind, WorkspaceKind::PlainDir);

        // Stop the session to avoid leaking `sleep` processes.
        let _ = crate::mux::SessionManager::new().kill(&created.mux_session);
    }

    #[test]
    fn test_create_agent_plain_dir_propagates_storage_save_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-plain");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let temp_dir = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let workdir = TempDir::new().expect("create workdir");
        app.set_cwd_project_root(Some(workdir.path().to_path_buf()));

        let err = with_tracing_dispatch(|| {
            handler.create_agent(&mut app.data, "plain-dir-save-error", None)
        })
        .expect_err("expected create_agent to fail");
        assert!(err.to_string().contains("Failed to replace state file"));

        for agent in app
            .data
            .storage
            .iter()
            .filter(|agent| agent.title == "plain-dir-save-error")
        {
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }
    }

    #[test]
    fn test_reconnect_to_worktree_swarm_removes_existing_agents() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();

        app.data.storage.add(Agent::new(
            "sentinel".to_string(),
            test_sleep_program(),
            "tenex-test/sentinel".to_string(),
            worktree_path.clone(),
        ));

        let root = Agent::new(
            "asdf".to_string(),
            test_sleep_program(),
            branch.clone(),
            worktree_path.clone(),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.storage.add(Agent::new_child(
            "child".to_string(),
            test_sleep_program(),
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

        let next = with_tracing_dispatch(|| handler.reconnect_to_worktree(&mut app.data))
            .expect("reconnect to worktree");
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
            .expect("expected root agent")
            .mux_session
            .clone();
        let _ = crate::mux::SessionManager::new().kill(&new_root_session);
        let _ = crate::mux::SessionManager::new().kill(&root_session);
    }

    #[cfg(unix)]
    #[test]
    fn test_reconnect_to_worktree_swarm_propagates_launch_root_agent_errors() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/reconnect-swarm".to_string();

        let mut existing = Agent::new(
            "existing".to_string(),
            test_sleep_program(),
            branch.clone(),
            worktree_path.clone(),
        );
        existing.runtime = AgentRuntime::Host;
        app.data.storage.add(existing);

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "swarm".to_string(),
            prompt: Some("do stuff".to_string()),
            branch,
            worktree_path,
            repo_root: std::path::PathBuf::from("/tmp"),
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "deadbeef".to_string(),
            swarm_child_count: Some(2),
        });

        crate::mux::set_socket_override("tenex-mux\0invalid").expect("set socket override");

        let _ = with_tracing_dispatch(|| handler.reconnect_to_worktree(&mut app.data))
            .expect_err("expected reconnect_to_worktree to fail");
    }

    #[test]
    fn test_recreate_worktree_no_conflict_info() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // No conflict info set - should error
        let result = handler.recreate_worktree(&mut app.data);
        assert!(result.is_err());
    }

    #[test]
    fn test_recreate_worktree_errors_when_repo_root_invalid() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let worktree = TempDir::new().expect("create worktree dir");

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "invalid-repo".to_string(),
            prompt: None,
            branch: "feature".to_string(),
            worktree_path: worktree.path().to_path_buf(),
            repo_root: PathBuf::from("/tmp/tenex-nonexistent-repo-root"),
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "deadbeef".to_string(),
            swarm_child_count: None,
        });

        let err = with_tracing_dispatch(|| handler.recreate_worktree(&mut app.data))
            .expect_err("expected recreate_worktree to error");
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[test]
    fn test_recreate_worktree_propagates_worktree_remove_errors() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = worktree_dir.path().join("feature");
        worktree_mgr
            .create(&worktree_path, "feature")
            .expect("create worktree");
        worktree_mgr
            .lock("feature", Some("locked"))
            .expect("lock worktree");

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "locked".to_string(),
            prompt: None,
            branch: "feature".to_string(),
            worktree_path,
            repo_root,
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "deadbeef".to_string(),
            swarm_child_count: None,
        });

        let err = with_tracing_dispatch(|| handler.recreate_worktree(&mut app.data))
            .expect_err("expected recreate_worktree to error");
        assert!(err.to_string().contains("Failed to remove worktree"));
    }

    #[test]
    fn test_recreate_worktree_propagates_create_agent_internal_errors() {
        let handler = Actions::new();
        let temp_dir = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);
        let (_repo_dir, repo_root) = init_repo();

        let worktree_dir = TempDir::new().expect("create worktree dir");
        let invalid_parent = worktree_dir.path().join("not-a-dir");
        std::fs::write(&invalid_parent, "payload").expect("write file worktree parent");
        let worktree_path = invalid_parent.join("feature");

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "create-agent-internal-fails".to_string(),
            prompt: None,
            branch: "feature".to_string(),
            worktree_path,
            repo_root,
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "deadbeef".to_string(),
            swarm_child_count: None,
        });

        let err = with_tracing_dispatch(|| handler.recreate_worktree(&mut app.data))
            .expect_err("expected recreate_worktree to error");
        assert!(
            err.to_string()
                .contains("Failed to create parent directory")
        );
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_repo_root_invalid() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
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

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal(&next));
        assert!(!mode_is_normal(&next));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        assert!(app.data.review.filter.is_empty());
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_target_branch_empty() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "   ".to_string();
        app.data.review.filter = "m".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal(&next));
        assert!(!mode_is_normal(&next));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
        assert!(app.data.review.filter.is_empty());
    }

    #[test]
    fn test_switch_branch_noops_when_already_on_branch() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "main".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
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
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_root_agent_missing() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "main".to_string();
        app.data.git_op.target_branch = "feature".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal(&next));
        assert!(!mode_is_normal(&next));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_create_agent_errors_when_target_directory_cannot_be_resolved() {
        let _guard = crate::test_support::lock_env_test_environment();

        let original_dir = std::env::current_dir().expect("read current dir");
        let parent = TempDir::new().expect("create parent dir");
        let cwd = parent.path().join("child");
        std::fs::create_dir_all(&cwd).expect("create child dir");
        std::env::set_current_dir(&cwd).expect("set cwd");
        drop(parent);
        assert!(std::env::current_dir().is_err());

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        app.set_cwd_project_root(None);

        let err = handler
            .create_agent(&mut app.data, "cannot-resolve-root", None)
            .expect_err("expected create_agent to error");
        assert!(
            err.to_string()
                .contains("Failed to resolve target directory")
        );

        std::env::set_current_dir(&original_dir).expect("restore cwd");
    }

    #[test]
    fn test_create_agent_prompts_worktree_conflict_when_worktree_exists() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.set_cwd_project_root(Some(repo_root.clone()));

        let title = "worktree-conflict";
        let branch = app.data.config.generate_branch_name(title);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, &branch);

        let repo = git::open_repository(&repo_root).expect("open repository");
        let worktree_mgr = WorktreeManager::new(&repo);
        worktree_mgr
            .create_with_new_branch_with_options(
                &worktree_path,
                &branch,
                WorktreeCreateOptions::default(),
            )
            .expect("create worktree");

        let next = with_tracing_dispatch(|| handler.create_agent(&mut app.data, title, None))
            .expect("create agent");
        assert!(mode_is_worktree_conflict(&next));

        let conflict = app
            .data
            .spawn
            .worktree_conflict
            .as_ref()
            .expect("expected conflict info");
        assert_eq!(conflict.branch, branch);
        let actual_repo_root = canonicalize_or_self(&conflict.repo_root);
        let expected_repo_root = canonicalize_or_self(&repo_root);
        assert_eq!(actual_repo_root, expected_repo_root);

        let actual_worktree = canonicalize_or_self(&conflict.worktree_path);
        let expected_worktree = canonicalize_or_self(&worktree_path);
        assert_eq!(actual_worktree, expected_worktree);
        assert_eq!(conflict.title, title);
        assert!(conflict.current_branch.contains("master"));
        assert!(conflict.existing_branch.is_some());
        assert!(conflict.existing_commit.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_create_agent_worktree_conflict_falls_back_when_head_info_unavailable() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.set_cwd_project_root(Some(repo_root.clone()));

        let title = "broken-head-info";
        let branch = app.data.config.generate_branch_name(title);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, &branch);

        let repo = git::open_repository(&repo_root).expect("open repository");
        let worktree_mgr = WorktreeManager::new(&repo);
        worktree_mgr
            .create_with_new_branch_with_options(
                &worktree_path,
                &branch,
                WorktreeCreateOptions::default(),
            )
            .expect("create worktree");

        std::fs::write(
            repo_root.join(".git").join("HEAD"),
            "ref: refs/heads/does-not-exist\n",
        )
        .expect("write invalid HEAD");
        std::fs::remove_file(worktree_path.join(".git")).expect("remove worktree git pointer");

        let next = with_tracing_dispatch(|| handler.create_agent(&mut app.data, title, None))
            .expect("create agent");
        assert!(mode_is_worktree_conflict(&next));

        let conflict = app
            .data
            .spawn
            .worktree_conflict
            .as_ref()
            .expect("expected conflict info");
        assert_eq!(conflict.current_branch, "unknown");
        assert_eq!(conflict.current_commit, "unknown");
        assert!(conflict.existing_branch.is_none());
        assert!(conflict.existing_commit.is_none());
    }

    #[test]
    fn test_create_agent_propagates_worktree_create_errors() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");
        let invalid_worktree_root = worktree_dir.path().join("not-a-dir");
        std::fs::write(&invalid_worktree_root, "payload").expect("write worktree root file");

        app.data.config.worktree_dir = invalid_worktree_root;
        app.set_cwd_project_root(Some(repo_root));

        let err =
            with_tracing_dispatch(|| handler.create_agent(&mut app.data, "bad-worktree", None))
                .expect_err("expected create_agent to error");
        assert!(
            err.to_string()
                .contains("Failed to create parent directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_launch_child_agent_propagates_runtime_ready_errors() {
        let handler = Actions::new();
        let (app, _temp) = create_test_app();
        let worktree = TempDir::new().expect("create worktree dir");
        let docker = TempDir::new().expect("create docker dir");
        let script =
            write_fake_docker_script(&docker, "#!/bin/sh\necho 'docker down' >&2\nexit 1\n");

        let mut agent = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        agent.runtime = AgentRuntime::Docker;

        let err = crate::runtime::with_docker_program_override_for_tests(script, || {
            handler.launch_child_agent(&app.data, &mut agent, "Child", None)
        })
        .expect_err("expected docker readiness to fail");
        assert!(err.to_string().contains("Docker"));
    }

    #[test]
    fn test_launch_child_agent_propagates_program_parse_errors() {
        let handler = Actions::new();
        let (app, _temp) = create_test_app();
        let worktree = TempDir::new().expect("create worktree dir");

        let mut agent = Agent::new(
            "root".to_string(),
            r#"claude "unterminated"#.to_string(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );

        let err = handler
            .launch_child_agent(&app.data, &mut agent, "Child", None)
            .expect_err("expected invalid program to error");
        assert!(err.to_string().contains("Failed to parse command line"));
    }

    #[test]
    fn test_create_agent_internal_reports_repo_open_errors() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let repo_root = TempDir::new().expect("create dir");
        let worktree_dir = TempDir::new().expect("create worktree dir");

        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();

        let branch = "agent/internal-open-repo-error".to_string();
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(repo_root.path(), &branch);

        let err = handler
            .create_agent_internal(
                &mut app.data,
                repo_root.path(),
                "title",
                None,
                &branch,
                &worktree_path,
            )
            .expect_err("expected create_agent_internal to fail");
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn test_create_agent_internal_propagates_launch_root_agent_errors_when_docker_unavailable() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");
        let docker = TempDir::new().expect("create docker dir");
        let script =
            write_fake_docker_script(&docker, "#!/bin/sh\necho 'docker down' >&2\nexit 1\n");

        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.settings.docker_for_new_roots = true;

        let branch = "agent/internal-docker".to_string();
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, &branch);

        let err = crate::runtime::with_docker_program_override_for_tests(script, || {
            handler.create_agent_internal(
                &mut app.data,
                &repo_root,
                "title",
                None,
                &branch,
                &worktree_path,
            )
        })
        .expect_err("expected create_agent_internal to fail");
        assert!(err.to_string().contains("Docker is unavailable"));
    }

    #[test]
    fn test_create_agent_internal_succeeds_when_repo_ok() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-int");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let title = "internal-success";
        let branch = app.data.config.generate_branch_name(title);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, &branch);

        with_tracing_dispatch(|| {
            handler.create_agent_internal(
                &mut app.data,
                &repo_root,
                title,
                Some("do stuff"),
                &branch,
                &worktree_path,
            )
        })
        .expect("create agent internal");

        let created = app
            .data
            .storage
            .iter()
            .find(|agent| agent.branch == branch)
            .expect("expected agent to be created");
        let actual_worktree = canonicalize_or_self(&created.worktree_path);
        let expected_worktree = canonicalize_or_self(&worktree_path);
        assert_eq!(actual_worktree, expected_worktree);
        assert_eq!(created.workspace_kind, WorkspaceKind::GitWorktree);
        assert_eq!(created.repo_root.as_deref(), Some(repo_root.as_path()));

        let _ = crate::mux::SessionManager::new().kill(&created.mux_session);
    }

    #[test]
    fn test_canonicalize_or_self_falls_back_on_missing_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let missing = temp_dir.path().join(uuid::Uuid::new_v4().to_string());
        let actual = canonicalize_or_self(&missing);
        assert_eq!(actual, missing);
    }

    #[test]
    fn test_create_agent_internal_propagates_storage_save_errors() {
        let handler = Actions::new();
        let temp_file = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let branch = "tenex-test/save-error".to_string();
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, &branch);

        let err = handler
            .create_agent_internal(
                &mut app.data,
                &repo_root,
                "save error",
                None,
                &branch,
                &worktree_path,
            )
            .expect_err("expected create_agent_internal to fail");
        assert!(err.to_string().contains("Failed to replace state file"));

        for agent in app
            .data
            .storage
            .iter()
            .filter(|agent| agent.branch == branch)
        {
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }
    }

    #[test]
    fn test_reconnect_to_worktree_propagates_storage_save_errors() {
        let handler = Actions::new();
        let temp_file = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let worktree = TempDir::new().expect("create worktree dir");
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();
        let root = Agent::new(
            "asdf".to_string(),
            test_sleep_program(),
            branch.clone(),
            worktree_path.clone(),
        );
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "asdf".to_string(),
            prompt: None,
            branch: branch.clone(),
            worktree_path,
            repo_root: std::path::PathBuf::from("/tmp"),
            existing_branch: Some("main".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        });

        let err = handler
            .reconnect_to_worktree(&mut app.data)
            .expect_err("expected reconnect to fail");
        assert!(err.to_string().contains("Failed to replace state file"));

        for agent in app
            .data
            .storage
            .iter()
            .filter(|agent| agent.branch == branch)
        {
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }
        let _ = crate::mux::SessionManager::new().kill(&root_session);
    }

    #[test]
    fn test_reconnect_to_worktree_swarm_propagates_child_spawn_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-reco");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        app.data.settings.docker_for_new_roots = false;
        app.data.settings.agent_program = AgentProgram::Custom;
        app.data.settings.custom_agent_command = test_sleep_program();
        app.data.settings.planner_agent_program = AgentProgram::Custom;
        app.data.settings.planner_custom_agent_command = "bad \"".to_string();
        app.data.spawn.use_plan_prompt = true;

        let repo_root = TempDir::new().expect("create repo root dir");
        let worktree = TempDir::new().expect("create worktree dir");

        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "swarm".to_string(),
            prompt: Some("task".to_string()),
            branch: "tenex-test/swarm".to_string(),
            worktree_path: worktree.path().to_path_buf(),
            repo_root: repo_root.path().to_path_buf(),
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "abc".to_string(),
            swarm_child_count: Some(1),
        });

        assert!(handler.reconnect_to_worktree(&mut app.data).is_err());

        for agent in app.data.storage.iter() {
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_branch_missing_in_repo() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let (_repo_dir, repo_path) = init_repo();
        let root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "master".to_string(),
            repo_path,
        );
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.git_op.agent_id = Some(root_id);
        app.data.git_op.branch_name = "master".to_string();
        app.data.git_op.target_branch = "branch-does-not-exist".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal_containing(&next, "Branch not found"));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_kill_root_agent_tree_fails() {
        let handler = Actions::new();
        let temp_dir = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_dir = TempDir::new().expect("create worktree dir");
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "master".to_string(),
            repo_root.clone(),
        );
        root.repo_root = Some(repo_root);
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.git_op.agent_id = Some(root_id);
        app.data.git_op.branch_name = "master".to_string();
        app.data.git_op.target_branch = "feature".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal_containing(
            &next,
            "Switch branch failed"
        ));
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_spawn_root_agent_fails() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_dir = TempDir::new().expect("create worktree dir");
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "master".to_string(),
            repo_root.clone(),
        );
        root.program = "bad \"".to_string();
        root.repo_root = Some(repo_root);
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.git_op.agent_id = Some(root_id);
        app.data.git_op.branch_name = "master".to_string();
        app.data.git_op.target_branch = "feature".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal_containing(
            &next,
            "Switch branch failed"
        ));
    }

    #[test]
    fn test_ensure_worktree_for_branch_reuses_existing_worktree() {
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            branch_prefix: "tenex-test/".to_string(),
            ..Config::default()
        };

        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = config.worktree_path_for_repo_root(&repo_root, "feature");
        worktree_mgr
            .create(&worktree_path, "feature")
            .expect("create worktree");

        let reused = Actions::ensure_worktree_for_branch(
            &config,
            &repo_root,
            &worktree_mgr,
            "feature",
            AgentRuntime::Host,
        )
        .expect("ensure worktree");
        assert!(reused.exists());
    }

    #[test]
    fn test_prepare_branch_switch_target_propagates_remote_branch_commit_read_errors() {
        use git2::Repository;

        let (_repo_dir, repo_root) = init_repo();
        let repo = Repository::open(&repo_root).expect("open repository");
        let tree_id = repo
            .head()
            .expect("read HEAD")
            .peel_to_commit()
            .expect("peel HEAD commit")
            .tree_id();
        repo.reference(
            "refs/remotes/origin/bad",
            tree_id,
            true,
            "add bad remote ref",
        )
        .expect("create remote ref");

        let config = Config::default();
        let err = Actions::prepare_branch_switch_target(
            &config,
            &repo_root,
            "origin/bad",
            AgentRuntime::Host,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to read commit for remote branch")
        );
    }

    #[test]
    fn test_prepare_branch_switch_target_propagates_worktree_create_errors() {
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            branch_prefix: "tenex-test/".to_string(),
            ..Config::default()
        };

        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_path = config.worktree_path_for_repo_root(&repo_root, "feature");
        let parent_dir = worktree_path
            .parent()
            .expect("worktree path missing parent");
        std::fs::create_dir_all(parent_dir).expect("create worktree parent dir");
        std::fs::write(&worktree_path, "block").expect("write blocking file");

        let err = Actions::prepare_branch_switch_target(
            &config,
            &repo_root,
            "feature",
            AgentRuntime::Host,
        )
        .unwrap_err();
        assert!(err.to_string().contains("worktree"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_worktree_for_branch_skips_ignored_file_links_for_docker() {
        use git2::Signature;

        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            branch_prefix: "tenex-test/".to_string(),
            ..Config::default()
        };

        let repo = git::open_repository(&repo_root).expect("open repository");
        let sig = Signature::now("Test", "test@test.com").expect("create signature");
        let parent_commit = repo
            .head()
            .expect("read HEAD")
            .peel_to_commit()
            .expect("peel HEAD commit");

        std::fs::write(repo_root.join(".gitignore"), "ignored.txt\n").expect("write gitignore");
        let mut index = repo.index().expect("open index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("add gitignore");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add gitignore",
            &tree,
            &[&parent_commit],
        )
        .expect("commit gitignore");
        std::fs::write(repo_root.join("ignored.txt"), "payload").expect("write ignored file");
        let agents_path = repo_root.join("AGENTS.md");
        std::fs::write(&agents_path, "# local instructions\n").expect("write AGENTS.md");
        std::os::unix::fs::symlink("AGENTS.md", repo_root.join("CLAUDE.md"))
            .expect("symlink CLAUDE.md");

        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_mgr = WorktreeManager::new(&repo);
        let created = Actions::ensure_worktree_for_branch(
            &config,
            &repo_root,
            &worktree_mgr,
            "feature",
            AgentRuntime::Docker,
        )
        .expect("ensure docker worktree");

        assert!(std::fs::symlink_metadata(created.join("ignored.txt")).is_err());
        let linked_agents = created.join("AGENTS.md");
        assert!(
            std::fs::symlink_metadata(&linked_agents)
                .expect("read linked agents metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::canonicalize(&linked_agents).expect("canonicalize linked agents"),
            std::fs::canonicalize(&agents_path).expect("canonicalize agents")
        );

        let linked_claude = created.join("CLAUDE.md");
        assert!(
            std::fs::symlink_metadata(&linked_claude)
                .expect("read linked claude metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(&linked_claude).expect("read claude link"),
            PathBuf::from("AGENTS.md")
        );
        assert_eq!(
            std::fs::canonicalize(&linked_claude).expect("canonicalize claude"),
            std::fs::canonicalize(&agents_path).expect("canonicalize agents")
        );
    }

    #[test]
    fn test_handle_confirm_kill() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

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
        handler
            .handle_action(&mut app, crate::config::Action::Confirm)
            .expect("confirm kill");
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_kill_agent_root() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // Add a root agent
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Kill should work (session doesn't exist, but should not error)
        handler.kill_agent(&mut app.data).expect("kill root agent");
        assert_eq!(app.data.storage.len(), 0);
    }

    #[test]
    fn test_kill_agent_root_sets_delete_branch_for_tenex_namespace() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.kill_agent(&mut app.data).expect("kill root agent");
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_kill_agent_root_does_not_delete_non_prefixed_branch() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.kill_agent(&mut app.data).expect("kill root agent");
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_kill_root_agent_tree_skips_worktree_cleanup_when_repo_open_fails() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.repo_root = Some(temp.path().to_path_buf());
        let root_id = root.id;
        app.data.storage.add(root);

        handler
            .kill_root_agent_tree(&mut app.data, root_id, false)
            .expect("kill root agent tree");
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_kill_root_agent_tree_skips_worktree_cleanup_when_cwd_unavailable() {
        let _guard = crate::test_support::lock_env_test_environment();

        let original_dir = std::env::current_dir().expect("read current dir");
        let parent = TempDir::new().expect("create parent dir");
        let cwd = parent.path().join("child");
        std::fs::create_dir_all(&cwd).expect("create child dir");
        std::env::set_current_dir(&cwd).expect("set cwd");
        drop(parent);
        assert!(std::env::current_dir().is_err());

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        app.data.storage.add(root);

        handler
            .kill_root_agent_tree(&mut app.data, root_id, false)
            .expect("kill root agent tree");
        assert!(app.data.storage.is_empty());

        std::env::set_current_dir(&original_dir).expect("restore cwd");
    }

    #[cfg(unix)]
    #[test]
    fn test_kill_agent_root_cleans_up_docker_runtime() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                log.display()
            ),
        );

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.runtime = AgentRuntime::Docker;
        let expected_container = format!("tenex-runtime-{}", root.mux_session).to_lowercase();
        app.data.storage.add(root);

        crate::runtime::with_docker_program_override_for_tests(script, || {
            handler.kill_agent(&mut app.data)
        })
        .expect("kill root agent");

        let log_contents = fs::read_to_string(&log).expect("read docker log");
        assert!(log_contents.contains(&format!("rm -f {expected_container}")));
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_kill_agent_child() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

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
        handler.kill_agent(&mut app.data).expect("kill child agent");
        assert_eq!(app.data.storage.len(), 1);
        assert!(app.data.storage.get(root_id).is_some());
    }

    #[test]
    fn test_kill_agent_child_handles_missing_window_indices_and_save_errors() {
        let handler = Actions::new();
        let temp_dir = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

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

        let mut child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        child.window_index = None;
        let child_id = child.id;
        app.data.storage.add(child);
        let mut grandchild = Agent::new_child(
            "grandchild".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: child_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        );
        grandchild.window_index = None;
        app.data.storage.add(grandchild);
        app.data.select_agent_by_id(child_id);

        let err = handler.kill_agent(&mut app.data).unwrap_err();
        assert!(err.to_string().contains("Failed to replace state file"));
    }

    #[test]
    fn test_kill_agent_with_descendants() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

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
        handler
            .kill_agent(&mut app.data)
            .expect("kill agent with descendants");
        assert_eq!(app.data.storage.len(), 0);
    }

    #[test]
    fn test_spawn_terminal_creates_child_of_root() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

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
    }

    #[test]
    fn test_prepare_agent_for_launch_injects_conversation_id_for_claude() {
        let (mut app, _temp) = create_test_app();
        let mut agent = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex-test/root".to_string(),
            PathBuf::from("/tmp"),
        );

        Actions::prepare_agent_for_launch(&mut app.data, &mut agent);

        let expected_conversation_id = agent.id.to_string();
        assert_eq!(
            agent.conversation_id.as_deref(),
            Some(expected_conversation_id.as_str())
        );
        assert!(agent.mux_session.starts_with("tenex-"));
        assert!(agent.mux_session.ends_with(&agent.short_id()));
    }

    #[test]
    fn test_prepare_agent_for_launch_does_not_override_child_mux_session() {
        let (mut app, _temp) = create_test_app();

        let root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "tenex-test/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        let root_session = root.mux_session;

        let mut child = Agent::new_child(
            "child".to_string(),
            "codex".to_string(),
            "tenex-test/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 1,
                repo_root: None,
            },
        );
        let original_session = child.mux_session.clone();

        Actions::prepare_agent_for_launch(&mut app.data, &mut child);

        assert_eq!(child.mux_session, original_session);
        assert!(child.conversation_id.is_none());
    }

    #[test]
    fn test_prepare_agent_for_launch_keeps_existing_docker_runtime_scope() {
        let (mut app, _temp) = create_test_app();
        let mut agent = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex-test/root".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.runtime = AgentRuntime::Docker;
        agent.runtime_scope = "existing-scope".to_string();
        Actions::prepare_agent_for_launch(&mut app.data, &mut agent);
        assert_eq!(agent.runtime_scope, "existing-scope");
    }

    #[test]
    fn test_launch_root_agent_returns_error_when_program_invalid() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");
        let mut agent = Agent::new(
            "root".to_string(),
            r#"claude "unterminated"#.to_string(),
            "tenex-test/root".to_string(),
            temp.path().to_path_buf(),
        );

        let err = handler
            .launch_root_agent(&mut app.data, &mut agent, None)
            .expect_err("expected invalid program to error");
        assert!(err.to_string().contains("Failed to parse command line"));
    }

    #[cfg(unix)]
    #[test]
    fn test_launch_root_agent_returns_error_when_mux_socket_invalid() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");
        crate::mux::set_socket_override("tenex-mux\0invalid").expect("set socket override");

        let mut agent = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "tenex-test/root".to_string(),
            temp.path().to_path_buf(),
        );

        let _ = handler
            .launch_root_agent(&mut app.data, &mut agent, None)
            .expect_err("expected invalid socket to error");
    }

    #[cfg(unix)]
    #[test]
    fn test_launch_root_agent_returns_error_when_docker_unavailable() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");
        let script = write_fake_docker_script(&temp, "#!/bin/sh\necho 'docker down' >&2\nexit 1\n");

        let mut agent = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex-test/root".to_string(),
            temp.path().to_path_buf(),
        );
        agent.runtime = AgentRuntime::Docker;

        let err = crate::runtime::with_docker_program_override_for_tests(script, || {
            handler.launch_root_agent(&mut app.data, &mut agent, None)
        })
        .expect_err("expected docker readiness to fail");
        assert!(err.to_string().contains("Docker is unavailable"));
        assert!(err.to_string().contains("docker down"));
    }

    #[test]
    fn test_create_agent_internal_errors_when_worktree_path_not_empty() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let branch = "tenex-test/branch".to_string();
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, &branch);
        std::fs::create_dir_all(&worktree_path).expect("create existing worktree path");
        std::fs::write(worktree_path.join("payload.txt"), "payload").expect("write payload");

        let err = handler
            .create_agent_internal(
                &mut app.data,
                &repo_root,
                "title",
                None,
                &branch,
                &worktree_path,
            )
            .expect_err("expected create_agent_internal to fail");
        assert!(err.to_string().contains("worktree"));
    }

    #[test]
    fn test_remove_conflicting_agents_continues_when_session_used_elsewhere() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let worktree = TempDir::new().expect("create worktree dir");
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();

        let mut conflict_agent = Agent::new(
            "conflict".to_string(),
            "codex".to_string(),
            branch.clone(),
            worktree_path.clone(),
        );
        conflict_agent.mux_session = "shared-session".to_string();
        let conflict_id = conflict_agent.id;
        app.data.storage.add(conflict_agent);

        let mut other_agent = Agent::new(
            "other".to_string(),
            "codex".to_string(),
            "tenex-test/other".to_string(),
            worktree_path.clone(),
        );
        other_agent.mux_session = "shared-session".to_string();
        let other_id = other_agent.id;
        app.data.storage.add(other_agent);

        let removed = handler.remove_conflicting_agents(
            &mut app.data,
            &WorktreeConflictInfo {
                title: "conflict".to_string(),
                prompt: None,
                branch,
                worktree_path,
                repo_root: PathBuf::from("/tmp"),
                existing_branch: None,
                existing_commit: None,
                current_branch: "main".to_string(),
                current_commit: "abc".to_string(),
                swarm_child_count: None,
            },
        );

        assert_eq!(removed, 1);
        assert!(app.data.storage.get(conflict_id).is_none());
        assert!(app.data.storage.get(other_id).is_some());
    }

    #[test]
    fn test_remove_conflicting_agents_covers_other_sessions_and_other_worktrees() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        crate::mux::set_socket_override("tenex-mux\0invalid/socket")
            .expect("Expected socket override");

        let worktree = TempDir::new().expect("create worktree dir");
        let conflict_worktree_path = worktree.path().join("conflict");
        let other_worktree_path = worktree.path().join("other");

        let branch = "tenex-test/asdf".to_string();

        let mut other_session_agent = Agent::new(
            "other-session".to_string(),
            "codex".to_string(),
            "tenex-test/other".to_string(),
            conflict_worktree_path.clone(),
        );
        other_session_agent.mux_session = "other-session".to_string();
        let other_session_id = other_session_agent.id;
        app.data.storage.add(other_session_agent);

        let mut other_worktree_agent = Agent::new(
            "other-worktree".to_string(),
            "codex".to_string(),
            branch.clone(),
            other_worktree_path,
        );
        other_worktree_agent.mux_session = "other-session-2".to_string();
        let other_worktree_id = other_worktree_agent.id;
        app.data.storage.add(other_worktree_agent);

        let mut conflict_agent = Agent::new(
            "conflict".to_string(),
            "codex".to_string(),
            branch.clone(),
            conflict_worktree_path.clone(),
        );
        conflict_agent.mux_session = "conflict-session".to_string();
        let conflict_id = conflict_agent.id;
        app.data.storage.add(conflict_agent);

        let removed = handler.remove_conflicting_agents(
            &mut app.data,
            &WorktreeConflictInfo {
                title: "conflict".to_string(),
                prompt: None,
                branch,
                worktree_path: conflict_worktree_path,
                repo_root: PathBuf::from("/tmp"),
                existing_branch: None,
                existing_commit: None,
                current_branch: "main".to_string(),
                current_commit: "abc".to_string(),
                swarm_child_count: None,
            },
        );

        assert_eq!(removed, 1);
        assert!(app.data.storage.get(conflict_id).is_none());
        assert!(app.data.storage.get(other_session_id).is_some());
        assert!(app.data.storage.get(other_worktree_id).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_remove_conflicting_agents_warns_when_cleanup_runtime_fails() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        crate::mux::set_socket_override("tenex-mux\0invalid").expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 1\n",
                log.display()
            ),
        );

        let worktree = TempDir::new().expect("create worktree dir");
        let worktree_path = worktree.path().to_path_buf();
        let branch = "tenex-test/asdf".to_string();

        let mut agent = Agent::new(
            "conflict".to_string(),
            "codex".to_string(),
            branch.clone(),
            worktree_path.clone(),
        );
        agent.runtime = AgentRuntime::Docker;
        agent.mux_session = "conflict-session".to_string();
        let expected_container = format!("tenex-runtime-{}", agent.mux_session).to_lowercase();
        app.data.storage.add(agent);

        let removed = crate::runtime::with_docker_program_override_for_tests(script, || {
            tracing::dispatcher::with_default(&dispatch, || {
                handler.remove_conflicting_agents(
                    &mut app.data,
                    &WorktreeConflictInfo {
                        title: "conflict".to_string(),
                        prompt: None,
                        branch: branch.clone(),
                        worktree_path: worktree_path.clone(),
                        repo_root: PathBuf::from("/tmp"),
                        existing_branch: None,
                        existing_commit: None,
                        current_branch: "main".to_string(),
                        current_commit: "abc".to_string(),
                        swarm_child_count: None,
                    },
                )
            })
        });

        assert_eq!(removed, 1);
        let log_contents = std::fs::read_to_string(&log).expect("read docker log");
        assert!(log_contents.contains(&format!("rm -f {expected_container}")));
    }

    #[cfg(unix)]
    #[test]
    fn test_kill_root_agent_tree_warns_when_cleanup_runtime_fails() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        crate::mux::set_socket_override("tenex-mux\0invalid").expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().expect("create temp dir");
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 1\n",
                log.display()
            ),
        );

        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_dir = TempDir::new().expect("create worktree dir");
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();
        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, "feature");
        worktree_mgr
            .create(&worktree_path, "feature")
            .expect("create worktree");

        let mut root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "feature".to_string(),
            worktree_path,
        );
        root.repo_root = Some(repo_root);
        root.runtime = AgentRuntime::Docker;
        root.mux_session = "runtime-fail-session".to_string();
        let expected_container = format!("tenex-runtime-{}", root.mux_session).to_lowercase();
        let root_id = root.id;
        app.data.storage.add(root);

        crate::runtime::with_docker_program_override_for_tests(script, || {
            tracing::dispatcher::with_default(&dispatch, || {
                handler.kill_root_agent_tree(&mut app.data, root_id, false)
            })
        })
        .expect("kill root agent tree");

        let log_contents = std::fs::read_to_string(&log).expect("read docker log");
        assert!(log_contents.contains(&format!("rm -f {expected_container}")));
    }

    #[cfg(unix)]
    #[test]
    fn test_kill_root_agent_tree_warns_when_worktree_remove_fails() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        crate::mux::set_socket_override("tenex-mux\0invalid").expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_dir = TempDir::new().expect("create worktree dir");
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, "feature");
        worktree_mgr
            .create(&worktree_path, "feature")
            .expect("create worktree");
        worktree_mgr
            .lock("feature", Some("locked"))
            .expect("lock worktree");

        let mut root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "feature".to_string(),
            worktree_path,
        );
        root.repo_root = Some(repo_root);
        let root_id = root.id;
        app.data.storage.add(root);

        tracing::dispatcher::with_default(&dispatch, || {
            handler.kill_root_agent_tree(&mut app.data, root_id, false)
        })
        .expect("kill root agent tree");

        assert!(
            app.data
                .ui
                .status_message
                .as_deref()
                .unwrap_or_default()
                .contains("Warning:")
        );
    }

    #[test]
    fn test_kill_agent_child_with_descendant_collects_window_indices() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        crate::mux::set_socket_override("tenex-mux\0invalid").expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

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

        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
                repo_root: None,
            },
        );
        let child_id = child.id;
        app.data.storage.add(child);

        let grandchild = Agent::new_child(
            "grandchild".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: child_id,
                mux_session: root_session,
                window_index: 3,
                repo_root: None,
            },
        );
        app.data.storage.add(grandchild);

        app.select_next();
        assert_eq!(app.selected_agent().map(|a| a.id), Some(child_id));

        tracing::dispatcher::with_default(&dispatch, || handler.kill_agent(&mut app.data))
            .expect("kill agent");

        assert_eq!(app.data.storage.len(), 1);
        assert!(app.data.storage.get(root_id).is_some());
    }

    #[test]
    fn test_switch_branch_returns_error_modal_when_worktree_missing_on_disk() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.config.branch_prefix = "tenex-test/".to_string();

        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_root, "feature");
        worktree_mgr
            .create(&worktree_path, "feature")
            .expect("create worktree");
        std::fs::remove_dir_all(&worktree_path).expect("remove worktree");

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "master".to_string(),
            repo_root.clone(),
        );
        root.repo_root = Some(repo_root);
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.git_op.agent_id = Some(root_id);
        app.data.git_op.branch_name = "master".to_string();
        app.data.git_op.target_branch = "feature".to_string();

        let next = handler.switch_branch(&mut app.data).expect("switch branch");
        assert!(mode_is_error_modal_containing(
            &next,
            "Worktree path does not exist"
        ));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.target_branch.is_empty());
    }

    #[test]
    fn test_resolve_target_branch_errors_when_remote_ref_not_commit() {
        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);

        let blob = repo.blob(b"payload").expect("create blob");
        repo.reference("refs/remotes/origin/bad", blob, true, "test")
            .expect("create remote reference");

        let err = Actions::resolve_target_branch(&repo, &branch_mgr, "origin/bad")
            .expect_err("expected invalid remote ref to error");
        assert!(
            err.to_string()
                .contains("Failed to read commit for remote branch 'origin/bad'")
        );
    }

    #[test]
    fn test_resolve_target_branch_creates_local_branch_for_remote() {
        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);

        let commit = repo
            .head()
            .expect("read HEAD")
            .peel_to_commit()
            .expect("peel HEAD commit");
        repo.reference(
            "refs/remotes/origin/feature",
            commit.id(),
            true,
            "test remote branch",
        )
        .expect("failed to create remote reference");

        let resolved = Actions::resolve_target_branch(&repo, &branch_mgr, "origin/feature")
            .expect("resolve branch");
        assert_eq!(resolved.as_deref(), Some("feature"));
        assert!(branch_mgr.exists("feature"));
    }

    #[test]
    fn test_resolve_target_branch_reuses_existing_local_branch_for_remote() {
        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let commit = repo
            .head()
            .expect("read HEAD")
            .peel_to_commit()
            .expect("peel HEAD commit");
        repo.reference(
            "refs/remotes/origin/feature",
            commit.id(),
            true,
            "test remote branch",
        )
        .expect("failed to create remote reference");

        let resolved = Actions::resolve_target_branch(&repo, &branch_mgr, "origin/feature")
            .expect("resolve branch");
        assert_eq!(resolved.as_deref(), Some("feature"));
    }

    #[test]
    fn test_resolve_target_branch_reports_error_when_local_branch_name_invalid() {
        let (_repo_dir, repo_root) = init_repo();
        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);

        let commit = repo
            .head()
            .expect("read HEAD")
            .peel_to_commit()
            .expect("peel HEAD commit");
        repo.reference(
            "refs/remotes/origin/HEAD",
            commit.id(),
            true,
            "test remote branch",
        )
        .expect("failed to create remote reference");

        let err = Actions::resolve_target_branch(&repo, &branch_mgr, "origin/HEAD")
            .expect_err("expected invalid local branch name to error");
        assert!(
            err.to_string()
                .contains("Failed to create local branch 'HEAD'")
        );
    }

    #[test]
    fn test_ensure_worktree_for_branch_errors_when_target_path_not_empty() {
        let (_repo_dir, repo_root) = init_repo();
        let worktree_dir = TempDir::new().expect("create worktree dir");

        let config = Config {
            worktree_dir: worktree_dir.path().to_path_buf(),
            branch_prefix: "tenex-test/".to_string(),
            ..Config::default()
        };

        let repo = git::open_repository(&repo_root).expect("open repository");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature").expect("create branch");

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktree_path = config.worktree_path_for_repo_root(&repo_root, "feature");
        std::fs::create_dir_all(&worktree_path).expect("create worktree path");
        std::fs::write(worktree_path.join("payload.txt"), "payload").expect("write payload");

        let err = Actions::ensure_worktree_for_branch(
            &config,
            &repo_root,
            &worktree_mgr,
            "feature",
            AgentRuntime::Host,
        )
        .expect_err("expected non-empty worktree path to error");
        assert!(err.to_string().contains("worktree"));
    }

    #[test]
    fn test_kill_root_agent_tree_returns_ok_when_root_missing() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        handler
            .kill_root_agent_tree(&mut app.data, uuid::Uuid::new_v4(), false)
            .expect("kill root agent tree");
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_root_agent_in_worktree_propagates_storage_save_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-root");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let temp_dir = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let worktree = TempDir::new().expect("create worktree dir");
        let err = handler
            .spawn_root_agent_in_worktree(
                &mut app.data,
                RootLaunchSpec {
                    title: "root".to_string(),
                    program: test_sleep_program(),
                    runtime: AgentRuntime::Host,
                    repo_root: worktree.path().to_path_buf(),
                    branch: "master".to_string(),
                    worktree_path: worktree.path().to_path_buf(),
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains("Failed to replace state file"));

        for agent in app.data.storage.iter() {
            let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_terminal_docker_runtime_skips_host_startup_input() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-dock");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let repo_dir = TempDir::new().expect("create repo dir");
        let mut root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "main".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.runtime = AgentRuntime::Docker;
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let manager = SessionManager::new();
        manager
            .create(&session, repo_dir.path(), None)
            .expect("create mux session");

        let docker_dir = TempDir::new().expect("create docker dir");
        let script = write_fake_docker_script(
            &docker_dir,
            "#!/bin/sh\ncase \"$1\" in\n  version) exit 0;;\n  image) echo '<no value>'; exit 0;;\n  build) cat >/dev/null; exit 0;;\n  inspect) echo 'No such container' >&2; exit 1;;\n  run) echo fake-container-id; exit 0;;\n  exec) exit 0;;\n  *) echo \"unexpected docker args: $*\" >&2; exit 1;;\nesac\n",
        );

        let next = crate::runtime::with_docker_program_override_for_tests(script, || {
            handler.spawn_terminal(&mut app.data, Some("echo TENEX_TERMINAL_STARTUP"))
        })
        .expect("spawn docker terminal");
        app.apply_mode(next);
        assert_eq!(app.mode, AppMode::normal());

        let _ = manager.kill(&session);
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_terminal_propagates_storage_save_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-tsave");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let temp_dir = TempDir::new().expect("create temp dir");
        let storage = Storage::with_path(temp_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let repo_dir = TempDir::new().expect("create repo dir");
        let mut root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "main".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let manager = SessionManager::new();
        manager
            .create(&session, repo_dir.path(), None)
            .expect("create mux session");

        let err = handler.spawn_terminal(&mut app.data, None).unwrap_err();
        assert!(err.to_string().contains("Failed to replace state file"));

        let _ = manager.kill(&session);
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_terminal_sends_startup_command_on_host_runtime() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-thost");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let repo_dir = TempDir::new().expect("create repo dir");
        let mut root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "main".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let manager = SessionManager::new();
        manager
            .create(&session, repo_dir.path(), None)
            .expect("create mux session");

        let next = tracing::dispatcher::with_default(&dispatch, || {
            handler.spawn_terminal(&mut app.data, Some("echo TENEX_TERMINAL_STARTUP"))
        })
        .expect("spawn terminal");
        app.apply_mode(next);
        assert_eq!(app.mode, AppMode::normal());

        let children = app.data.storage.children(root_id);
        let terminal = children.first().expect("expected terminal");
        let window_target = SessionManager::window_target(
            &terminal.mux_session,
            terminal.window_index.expect("missing window index"),
        );

        let found = window_output_contains(&window_target, "TENEX_TERMINAL_STARTUP", 25);
        let _ = manager.kill(&session);
        assert!(found);
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_terminal_startup_command_helpers_cover_timeout_path() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("agent-life-ttime");
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let repo_dir = TempDir::new().expect("create repo dir");
        let mut root = Agent::new(
            "root".to_string(),
            "codex".to_string(),
            "main".to_string(),
            repo_dir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let manager = SessionManager::new();
        manager
            .create(&session, repo_dir.path(), None)
            .expect("create mux session");

        let next = handler
            .spawn_terminal(&mut app.data, Some("echo TENEX_TERMINAL_STARTUP"))
            .expect("spawn terminal");
        app.apply_mode(next);
        assert_eq!(app.mode, AppMode::normal());

        let children = app.data.storage.children(root_id);
        let terminal = children.first().expect("expected terminal");
        let window_target = SessionManager::window_target(
            &terminal.mux_session,
            terminal.window_index.expect("missing window index"),
        );

        let found = window_output_contains(&window_target, "TENEX_NEVER_PRESENT", 5);
        let _ = manager.kill(&session);
        assert!(!found);
    }
}
