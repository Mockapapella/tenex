//! Swarm operations: spawn children, spawn review agents, synthesize

use crate::agent::{Agent, AgentRuntime, ChildConfig, Storage, WorkspaceKind};
use crate::git::{self, WorktreeManager};
use crate::mux::SessionManager;
use crate::prompts;
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::{AppData, WorktreeConflictInfo};
use crate::state::{AppMode, ConfirmAction, ConfirmingMode, ErrorModalMode};

/// Configuration for spawning child agents
pub struct SpawnConfig {
    pub root_session: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub workspace_kind: WorkspaceKind,
    pub runtime: AgentRuntime,
    pub parent_agent_id: uuid::Uuid,
}

struct NewRootSpawnConfig {
    config: SpawnConfig,
    cleaned_stale_worktree: bool,
}

#[derive(Clone, Copy)]
struct ReviewChildAgentConfig<'a> {
    root_session: &'a str,
    worktree_path: &'a Path,
    branch: &'a str,
    workspace_kind: WorkspaceKind,
    runtime: AgentRuntime,
    parent_id: uuid::Uuid,
    program: &'a str,
    review_prompt: &'a str,
    reviewer_number: usize,
    reserved_window_index: u32,
}

const CODEX_PANE_SETTLE_TIMEOUT: &str =
    "Timed out waiting for Codex pane to settle; leaving agent for manual review";
const CODEX_REVIEW_PRESET_TIMEOUT: &str =
    "Timed out waiting for Codex /review preset prompt; leaving agent for manual review";
const CODEX_REVIEW_BASE_BRANCH_TIMEOUT: &str =
    "Timed out waiting for Codex /review base branch prompt; leaving agent for manual review";
const CODEX_REVIEW_START_TIMEOUT: &str =
    "Timed out waiting for Codex /review to start; leaving agent for manual review";
const CODEX_REVIEW_BASE_BRANCH_MISMATCH_STATUS: &str =
    "Codex review may be running against a different base than requested";
const SYNTHESIS_KILL_WINDOW_WARN: &str =
    "Failed to kill descendant mux window during synthesis cleanup";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexReviewStart {
    NotObserved,
    MatchedBaseBranch,
    BaseBranchMismatch,
}

fn codex_review_base_branch_hint(base_branch: &str) -> String {
    format!("changes against '{base_branch}'")
}

fn write_synthesis_contents(file: &mut dyn Write, contents: &str) -> std::io::Result<()> {
    file.write_all(contents.as_bytes())
}

impl Actions {
    fn root_ancestor_for_known_agent<'a>(storage: &'a Storage, agent: &'a Agent) -> &'a Agent {
        storage.root_ancestor(agent.id).unwrap_or(agent)
    }

    fn resolve_swarm_repo_path(app_data: &AppData, current_dir: PathBuf) -> PathBuf {
        app_data
            .spawn
            .root_repo_path
            .clone()
            .or_else(|| app_data.selected_project_root())
            .or_else(|| app_data.cwd_project_root.clone())
            .unwrap_or(current_dir)
    }

    fn build_synthesis_read_command(synthesis_id: uuid::Uuid, prompt: Option<&str>) -> String {
        let mut read_command = format!(
            "Read .tenex/{synthesis_id}.md - it contains the collected descendant work. Use it to guide your next steps."
        );

        let prompt = prompt.unwrap_or("").trim();
        if prompt.is_empty() {
            return read_command;
        }

        read_command.push_str("\n\nAdditional instructions:\n");
        read_command.push_str(prompt);
        read_command
    }

    fn write_synthesis_file(
        worktree_path: &Path,
        synthesis_id: uuid::Uuid,
        synthesis_content: &str,
    ) -> Result<PathBuf> {
        let tenex_dir = worktree_path.join(".tenex");
        fs::create_dir_all(&tenex_dir)
            .context(format!("Failed to create {}", tenex_dir.display()))?;

        let synthesis_file = tenex_dir.join(format!("{synthesis_id}.md"));
        let mut file = fs::File::create(&synthesis_file)
            .context(format!("Failed to create {}", synthesis_file.display()))?;
        write_synthesis_contents(&mut file, synthesis_content)
            .context(format!("Failed to write to {}", synthesis_file.display()))?;

        Ok(synthesis_file)
    }

    fn start_codex_review_flow(self, target: &str, base_branch: &str) -> Result<CodexReviewStart> {
        let base_branch = base_branch.trim();
        if base_branch.is_empty() {
            bail!("Base branch cannot be empty for Codex review flow");
        }

        let poll_interval = Duration::from_millis(250);
        let step_timeout = Duration::from_secs(20);
        let idle_stable_for = Duration::from_millis(200);

        if !self.wait_for_pane_idle(target, idle_stable_for, step_timeout, poll_interval) {
            warn!(target, "{}", CODEX_PANE_SETTLE_TIMEOUT);
            return Ok(CodexReviewStart::NotObserved);
        }

        self.session_manager.send_keys(target, "/review")?;
        let _ = self.wait_for_pane_contains_any(
            target,
            &["review my current changes", "review current changes"],
            Duration::from_secs(5),
            poll_interval,
        );

        self.session_manager.send_keys_and_submit(target, "")?;
        if !self.wait_for_pane_contains_any(
            target,
            &["review preset", "review mode"],
            step_timeout,
            poll_interval,
        ) {
            warn!(target, "{}", CODEX_REVIEW_PRESET_TIMEOUT);
            return Ok(CodexReviewStart::NotObserved);
        }

        let _ = self.wait_for_pane_idle(target, idle_stable_for, step_timeout, poll_interval);
        self.session_manager.send_keys_and_submit(target, "")?;
        if !self.wait_for_pane_contains_any(target, &["base branch"], step_timeout, poll_interval) {
            warn!(target, "{}", CODEX_REVIEW_BASE_BRANCH_TIMEOUT);
            return Ok(CodexReviewStart::NotObserved);
        }

        let _ = self.wait_for_pane_idle(target, idle_stable_for, step_timeout, poll_interval);
        self.session_manager.send_keys(target, base_branch)?;
        self.session_manager.send_keys_and_submit(target, "")?;
        let review_start = self.wait_for_pane_contains_any_text(
            target,
            &[
                "review started",
                "Code review started",
                "Code Review Started",
            ],
            step_timeout,
            poll_interval,
        );
        let Some(review_start) = review_start else {
            warn!(target, "{}", CODEX_REVIEW_START_TIMEOUT);
            return Ok(CodexReviewStart::NotObserved);
        };

        let expected_hint = codex_review_base_branch_hint(base_branch);
        if !review_start.contains(&expected_hint) {
            warn!(
                target,
                requested_base_branch = base_branch,
                expected_hint,
                review_confirmation = %review_start,
                "Codex /review may have started against a different base branch"
            );
            return Ok(CodexReviewStart::BaseBranchMismatch);
        }

        Ok(CodexReviewStart::MatchedBaseBranch)
    }

    fn start_codex_review_flows(self, flows: Vec<(String, String)>) -> bool {
        let mut found_mismatch = false;
        for (target, base_branch) in flows {
            match self.start_codex_review_flow(&target, &base_branch) {
                Ok(CodexReviewStart::BaseBranchMismatch) => {
                    found_mismatch = true;
                }
                Ok(CodexReviewStart::NotObserved | CodexReviewStart::MatchedBaseBranch) => {}
                Err(err) => {
                    warn!(target, error = %err, "Failed to drive Codex /review flow");
                }
            }
        }
        found_mismatch
    }

    fn codex_review_flow_for_child(child: &Agent, base_branch: &str) -> Option<(String, String)> {
        if crate::conversation::detect_agent_cli(&child.program)
            != crate::conversation::AgentCli::Codex
        {
            return None;
        }

        let actual_index = child.window_index?;
        let window_target = SessionManager::window_target(&child.mux_session, actual_index);
        Some((window_target, base_branch.to_string()))
    }

    fn wait_for_pane_contains_any(
        self,
        target: &str,
        needles: &[&str],
        timeout: Duration,
        poll_interval: Duration,
    ) -> bool {
        self.wait_for_pane_contains_any_text(target, needles, timeout, poll_interval)
            .is_some()
    }

    fn wait_for_pane_contains_any_text(
        self,
        target: &str,
        needles: &[&str],
        timeout: Duration,
        poll_interval: Duration,
    ) -> Option<String> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Ok(pane) = self.output_capture.capture_pane(target)
                && needles.iter().any(|needle| pane.contains(needle))
            {
                return Some(pane);
            }
            std::thread::sleep(poll_interval);
        }
        None
    }

    fn wait_for_pane_idle(
        self,
        target: &str,
        stable_for: Duration,
        timeout: Duration,
        poll_interval: Duration,
    ) -> bool {
        let start = Instant::now();
        let mut last_change = Instant::now();
        let mut baseline = String::new();
        while start.elapsed() < timeout {
            if let Ok(pane) = self.output_capture.capture_pane(target) {
                if pane != baseline {
                    baseline = pane;
                    last_change = Instant::now();
                } else if last_change.elapsed() >= stable_for {
                    return true;
                }
            }
            std::thread::sleep(poll_interval);
        }
        false
    }

    fn spawn_review_child_agent(
        self,
        app_data: &mut AppData,
        config: ReviewChildAgentConfig<'_>,
    ) -> Result<Agent> {
        let child_title = format!("Reviewer {}", config.reviewer_number);
        let mut child = Agent::new_child(
            child_title.clone(),
            config.program.to_string(),
            config.branch.to_string(),
            config.worktree_path.to_path_buf(),
            ChildConfig {
                parent_id: config.parent_id,
                mux_session: config.root_session.to_string(),
                window_index: config.reserved_window_index,
                repo_root: app_data
                    .storage
                    .get(config.parent_id)
                    .and_then(|agent| agent.repo_root.clone()),
            },
        );
        child.workspace_kind = config.workspace_kind;
        child.runtime = config.runtime;
        child.runtime_scope = app_data
            .storage
            .root_ancestor(config.parent_id)
            .map_or_else(
                || child.effective_runtime_scope().to_string(),
                |root| root.effective_runtime_scope().to_string(),
            );

        let cli = crate::conversation::detect_agent_cli(config.program);
        let prompt = match cli {
            crate::conversation::AgentCli::Codex => None,
            _ => Some(config.review_prompt),
        };

        let actual_index = self.launch_child_agent(app_data, &mut child, &child_title, prompt)?;
        debug_assert_eq!(config.root_session, child.mux_session);
        child.window_index = Some(actual_index);
        Ok(child)
    }

    /// Spawn child agents under a parent (or create new root with children)
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_children(self, app_data: &mut AppData, task: Option<&str>) -> Result<AppMode> {
        let count = app_data.spawn.child_count;
        let parent_id = app_data.spawn.spawning_under;

        info!(
            count,
            ?parent_id,
            task_len = task.map_or(0, str::len),
            "Spawning child agents"
        );

        let (spawn_config, cleaned_stale_worktree) = if let Some(pid) = parent_id {
            (Self::get_existing_parent_config(app_data, pid)?, false)
        } else {
            match self.create_new_root_for_swarm(app_data, task, count)? {
                Some(new_root) => (new_root.config, new_root.cleaned_stale_worktree),
                None => {
                    return Ok(ConfirmingMode {
                        action: ConfirmAction::WorktreeConflict,
                    }
                    .into());
                }
            }
        };

        self.spawn_child_agents(app_data, &spawn_config, count, task)?;

        // Expand the parent to show children
        app_data
            .storage
            .set_collapsed(spawn_config.parent_agent_id, false);

        app_data.storage.save()?;
        info!(count, parent_id = %spawn_config.parent_agent_id, "Child agents spawned successfully");
        if cleaned_stale_worktree {
            app_data.set_status(format!(
                "Cleaned stale worktree and spawned {count} child agents"
            ));
        } else {
            app_data.set_status(format!("Spawned {count} child agents"));
        }
        Ok(AppMode::normal())
    }

    /// Get spawn configuration from an existing parent agent
    fn get_existing_parent_config(app_data: &AppData, pid: uuid::Uuid) -> Result<SpawnConfig> {
        let parent = app_data
            .storage
            .get(pid)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        if parent.is_terminal_agent() {
            bail!("Cannot spawn children under a terminal");
        }

        let root = Self::root_ancestor_for_known_agent(&app_data.storage, parent);
        Ok(SpawnConfig {
            root_session: root.mux_session.clone(),
            worktree_path: root.worktree_path.clone(),
            branch: root.branch.clone(),
            workspace_kind: root.workspace_kind,
            runtime: root.runtime,
            parent_agent_id: pid,
        })
    }

    /// Create a new root agent for a swarm, returns None if worktree conflict
    fn create_new_root_for_swarm(
        self,
        app_data: &mut AppData,
        task: Option<&str>,
        count: usize,
    ) -> Result<Option<NewRootSpawnConfig>> {
        let root_title = Self::generate_root_title(task);
        let cwd_fallback = PathBuf::from(".");
        let repo_path = Self::resolve_swarm_repo_path(
            app_data,
            std::env::current_dir().unwrap_or(cwd_fallback),
        );
        let Ok(repo) = git::open_repository(&repo_path) else {
            let config = self.create_plain_dir_root_for_swarm(app_data, root_title, repo_path)?;
            return Ok(Some(NewRootSpawnConfig {
                config,
                cleaned_stale_worktree: false,
            }));
        };
        let branch = app_data.config.generate_branch_name(&root_title);

        let worktree_mgr = WorktreeManager::new(&repo);

        let worktree_path = app_data
            .config
            .worktree_path_for_repo_root(&repo_path, &branch);
        let target_preparation = worktree_mgr.prepare_worktree_creation_target(
            &worktree_path,
            &branch,
            &app_data.config.worktree_dir_for_repo_root(&repo_path),
        )?;

        if let Some(conflict_worktree_path) = target_preparation.registered_path() {
            Self::setup_worktree_conflict(
                app_data,
                &worktree_mgr,
                root_title,
                task,
                branch,
                conflict_worktree_path.to_path_buf(),
                count,
                repo_path,
            );
            return Ok(None);
        }

        let runtime = crate::runtime::new_root_runtime(&app_data.settings);
        let worktree_options = Self::root_worktree_create_options(runtime);
        worktree_mgr.create_with_new_branch_with_options(
            &worktree_path,
            &branch,
            worktree_options,
        )?;

        let program = app_data.agent_spawn_command();
        let mut root_agent = Agent::new(root_title, program, branch.clone(), worktree_path.clone());
        root_agent.repo_root = Some(repo_path);
        root_agent.runtime = runtime;

        self.launch_root_agent(app_data, &mut root_agent, None)?;

        let root_session = root_agent.mux_session.clone();
        let root_id = root_agent.id;

        app_data.storage.add(root_agent);
        Ok(Some(NewRootSpawnConfig {
            config: SpawnConfig {
                root_session,
                worktree_path,
                branch,
                workspace_kind: WorkspaceKind::GitWorktree,
                runtime,
                parent_agent_id: root_id,
            },
            cleaned_stale_worktree: target_preparation.cleaned_stale_target(),
        }))
    }

    fn create_plain_dir_root_for_swarm(
        self,
        app_data: &mut AppData,
        root_title: String,
        workdir: PathBuf,
    ) -> Result<SpawnConfig> {
        let program = app_data.agent_spawn_command();
        let branch = app_data.config.generate_branch_name(&root_title);
        let runtime = crate::runtime::new_root_runtime(&app_data.settings);
        let mut root_agent = Agent::new(root_title, program, branch.clone(), workdir.clone());
        root_agent.workspace_kind = WorkspaceKind::PlainDir;
        root_agent.repo_root = Some(workdir.clone());
        root_agent.runtime = runtime;

        self.launch_root_agent(app_data, &mut root_agent, None)?;

        let root_session = root_agent.mux_session.clone();
        let root_id = root_agent.id;

        app_data.storage.add(root_agent);
        Ok(SpawnConfig {
            root_session,
            worktree_path: workdir,
            branch,
            workspace_kind: WorkspaceKind::PlainDir,
            runtime,
            parent_agent_id: root_id,
        })
    }

    /// Generate a title for a new root swarm agent
    fn generate_root_title(task: Option<&str>) -> String {
        match task {
            Some(t) if t.len() > 30 => format!("{}...", &t[..27]),
            Some(t) => t.to_string(),
            None => {
                let short_id = &uuid::Uuid::new_v4().to_string()[..8];
                format!("Swarm ({short_id})")
            }
        }
    }

    /// Setup worktree conflict info for user to resolve
    #[expect(
        clippy::too_many_arguments,
        reason = "Worktree conflict setup needs repo, branch, and UI context"
    )]
    fn setup_worktree_conflict(
        app_data: &mut AppData,
        worktree_mgr: &WorktreeManager<'_>,
        root_title: String,
        task: Option<&str>,
        branch: String,
        worktree_path: PathBuf,
        count: usize,
        repo_root: PathBuf,
    ) {
        debug!(branch, "Worktree already exists for swarm, prompting user");

        let (current_branch, current_commit) = worktree_mgr
            .head_info()
            .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));

        let (existing_branch, existing_commit) = worktree_mgr
            .worktree_head_info(&branch)
            .map_or((None, None), |(b, c)| (Some(b), Some(c)));

        app_data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: root_title,
            prompt: task.map(String::from),
            branch,
            worktree_path,
            repo_root,
            existing_branch,
            existing_commit,
            current_branch,
            current_commit,
            swarm_child_count: Some(count),
        });
    }

    /// Spawn the actual child agents
    fn spawn_child_agents(
        self,
        app_data: &mut AppData,
        config: &SpawnConfig,
        count: usize,
        task: Option<&str>,
    ) -> Result<()> {
        let start_window_index = app_data
            .storage
            .reserve_window_indices(config.parent_agent_id);
        let program = if app_data.spawn.use_plan_prompt {
            app_data.planner_agent_spawn_command()
        } else {
            app_data.agent_spawn_command()
        };
        let child_prompt =
            task.map(|t| Self::build_child_prompt(t, app_data.spawn.use_plan_prompt));
        let child_title_prefix = if app_data.spawn.use_plan_prompt && child_prompt.is_some() {
            "Planner"
        } else {
            "Agent"
        };
        let start_child_number = next_child_number(
            &app_data.storage,
            config.parent_agent_id,
            child_title_prefix,
        );

        for i in 0..count {
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);
            let child_number = start_child_number.saturating_add(i);
            let child_title = format!("{child_title_prefix} {child_number}");
            self.spawn_single_child(
                app_data,
                config,
                window_index,
                &program,
                child_prompt.as_deref(),
                &child_title,
            )?;
        }

        Ok(())
    }

    /// Build the prompt for child agents
    fn build_child_prompt(task: &str, use_plan_prompt: bool) -> String {
        if use_plan_prompt {
            prompts::build_plan_prompt(task)
        } else {
            task.to_string()
        }
    }

    /// Spawn a single child agent
    fn spawn_single_child(
        self,
        app_data: &mut AppData,
        config: &SpawnConfig,
        window_index: u32,
        program: &str,
        child_prompt: Option<&str>,
        child_title: &str,
    ) -> Result<()> {
        let mut child = Agent::new_child(
            child_title.to_string(),
            program.to_string(),
            config.branch.clone(),
            config.worktree_path.clone(),
            ChildConfig {
                parent_id: config.parent_agent_id,
                mux_session: config.root_session.clone(),
                window_index,
                repo_root: app_data
                    .storage
                    .get(config.parent_agent_id)
                    .and_then(|agent| agent.repo_root.clone()),
            },
        );
        child.workspace_kind = config.workspace_kind;
        child.runtime = config.runtime;
        child.runtime_scope = app_data
            .storage
            .root_ancestor(config.parent_agent_id)
            .map_or_else(
                || child.effective_runtime_scope().to_string(),
                |root| root.effective_runtime_scope().to_string(),
            );

        let actual_index =
            self.launch_child_agent(app_data, &mut child, child_title, child_prompt)?;
        child.window_index = Some(actual_index);
        app_data.storage.add(child);

        Ok(())
    }

    /// Spawn child agents for an existing root agent
    ///
    /// This is a helper used by both `spawn_children` and `reconnect_to_worktree`
    pub(crate) fn spawn_children_for_root(
        self,
        app_data: &mut AppData,
        config: &SpawnConfig,
        count: usize,
        task: &str,
    ) -> Result<()> {
        self.spawn_child_agents(app_data, config, count, Some(task))?;

        // Expand the parent to show children
        app_data
            .storage
            .set_collapsed(config.parent_agent_id, false);

        Ok(())
    }

    /// Spawn review agents for the selected agent against a base branch
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_review_agents(self, app_data: &mut AppData) -> Result<()> {
        let count = app_data.spawn.child_count;
        let parent_id = app_data
            .spawn
            .spawning_under
            .ok_or_else(|| anyhow::anyhow!("No agent selected for review"))?;
        let parent = app_data
            .storage
            .get(parent_id)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        if parent.is_terminal_agent() {
            bail!("Cannot spawn review agents under a terminal");
        }
        let base_branch = app_data
            .review
            .base_branch
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No base branch selected for review"))?;

        info!(count, parent_id = %parent_id, base_branch, "Spawning review agents");

        // Get the root agent's session and worktree info
        let root = Self::root_ancestor_for_known_agent(&app_data.storage, parent);

        let root_session = root.mux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();
        let root_workspace_kind = root.workspace_kind;
        let root_runtime = root.runtime;
        if root_workspace_kind != WorkspaceKind::GitWorktree {
            bail!("Review swarm requires a git repository");
        }

        // Build the review prompt
        let review_prompt = prompts::build_review_prompt(&base_branch);

        // Reserve window indices
        let start_window_index = app_data.storage.reserve_window_indices(parent_id);
        let program = app_data.review_agent_spawn_command();
        let mut codex_review_flows: Vec<(String, String)> = Vec::new();
        let start_reviewer_number = next_child_number(&app_data.storage, parent_id, "Reviewer");

        // Create review child agents
        for i in 0..count {
            let offset = u32::try_from(i).map_or(u32::MAX, |value| value);
            let window_index = start_window_index.saturating_add(offset);
            let reviewer_number = start_reviewer_number.saturating_add(i);
            let config = ReviewChildAgentConfig {
                root_session: root_session.as_str(),
                worktree_path: worktree_path.as_path(),
                branch: branch.as_str(),
                workspace_kind: root_workspace_kind,
                runtime: root_runtime,
                parent_id,
                program: program.as_str(),
                review_prompt: review_prompt.as_str(),
                reviewer_number,
                reserved_window_index: window_index,
            };
            let child = self.spawn_review_child_agent(app_data, config)?;
            codex_review_flows.extend(Self::codex_review_flow_for_child(&child, &base_branch));
            app_data.storage.add(child);
        }

        // Expand the parent to show children
        app_data.storage.set_collapsed(parent_id, false);

        app_data.storage.save()?;
        info!(count, parent_id = %parent_id, base_branch, "Review agents spawned successfully");
        app_data.set_status(format!(
            "Spawned {count} review agents against {base_branch}"
        ));

        // Clear review state
        app_data.review.clear();

        if !codex_review_flows.is_empty() {
            let found_mismatch = self.start_codex_review_flows(codex_review_flows);
            if found_mismatch {
                app_data.set_status(format!(
                    "{CODEX_REVIEW_BASE_BRANCH_MISMATCH_STATUS}: {base_branch}"
                ));
            }
        }

        Ok(())
    }

    /// Synthesize children into the parent agent
    ///
    /// Writes synthesis content to `.tenex/<id>.md` and tells the parent to read it.
    ///
    /// # Errors
    ///
    /// Returns an error if synthesis fails
    pub fn synthesize(self, app_data: &mut AppData) -> Result<AppMode> {
        self.synthesize_with_prompt(app_data, None)
    }

    /// Synthesize children into the parent agent with optional extra instructions.
    ///
    /// Writes synthesis content to `.tenex/<id>.md` and tells the parent to read it.
    ///
    /// # Errors
    ///
    /// Returns an error if synthesis fails
    pub fn synthesize_with_prompt(
        self,
        app_data: &mut AppData,
        prompt: Option<&str>,
    ) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected".to_string(),
            }
            .into());
        };

        if agent.is_terminal_agent() {
            return Ok(ErrorModalMode {
                message: "Cannot synthesize into a terminal agent".to_string(),
            }
            .into());
        }

        if !app_data.storage.has_children(agent.id) {
            warn!(agent_id = %agent.id, title = %agent.title, "No children to synthesize");
            return Ok(ErrorModalMode {
                message: "Selected agent has no children to synthesize".to_string(),
            }
            .into());
        }

        let parent_id = agent.id;
        let parent_agent = agent.clone();
        let parent_session = agent.mux_session.clone();
        let parent_title = agent.title.clone();
        let worktree_path = agent.worktree_path.clone();
        // Determine the correct target for the parent
        // If the parent has a window_index, it's a child agent running in a window
        let parent_target = agent.window_index.map_or_else(
            || parent_session.clone(),
            |window_idx| SessionManager::window_target(&parent_session, window_idx),
        );

        info!(%parent_id, %parent_title, "Synthesizing descendants into parent");

        let targets = app_data.synthesis_targets_for(parent_id);

        if targets.capture_agent_ids.is_empty() {
            warn!(agent_id = %parent_id, title = %parent_title, "No non-terminal children to synthesize");
            return Ok(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            }
            .into());
        }

        let findings =
            self.capture_synthesis_findings(app_data, &parent_session, &targets.capture_agent_ids);

        // Build synthesis content
        let synthesis_content = prompts::build_synthesis_prompt(&findings);

        let synthesis_id = uuid::Uuid::new_v4();
        let synthesis_file =
            Self::write_synthesis_file(&worktree_path, synthesis_id, &synthesis_content)?;

        debug!(?synthesis_file, "Wrote synthesis file");

        let root_id = app_data
            .storage
            .root_ancestor(parent_id)
            .map_or(parent_id, |root| root.id);
        let descendants_count = targets.capture_agent_ids.len();
        self.remove_synthesis_targets(
            app_data,
            root_id,
            &parent_session,
            &targets.teardown_root_ids,
            &targets.teardown_agent_ids,
        );

        // Now tell the parent to read the file
        let read_command = Self::build_synthesis_read_command(synthesis_id, prompt);
        let session_manager = &self.session_manager;
        let target = &parent_target;
        let agent = &parent_agent;
        let command = &read_command;
        session_manager.send_keys_and_submit_for_agent(target, agent, command)?;

        app_data.validate_selection();
        app_data.storage.save()?;
        app_data.clear_synthesis_marks();
        info!(%parent_title, descendants_count, "Synthesis complete");
        app_data.set_status("Synthesized findings into parent agent");
        Ok(AppMode::normal())
    }

    fn capture_synthesis_findings(
        self,
        app_data: &AppData,
        parent_session: &str,
        capture_agent_ids: &[uuid::Uuid],
    ) -> Vec<(String, String)> {
        let mut findings = Vec::new();

        for agent_id in capture_agent_ids {
            let Some(descendant) = app_data.storage.get(*agent_id) else {
                continue;
            };
            let target = descendant.window_index.map_or_else(
                || descendant.mux_session.clone(),
                |window_idx| SessionManager::window_target(parent_session, window_idx),
            );

            let output = self
                .output_capture
                .capture_pane_with_history(&target, 5000)
                .unwrap_or_else(|_| "(Could not capture output)".to_string());

            findings.push((descendant.title.clone(), output));
        }

        findings
    }

    fn remove_synthesis_targets(
        self,
        app_data: &mut AppData,
        root_id: uuid::Uuid,
        parent_session: &str,
        teardown_root_ids: &[uuid::Uuid],
        teardown_agent_ids: &[uuid::Uuid],
    ) {
        let mut deleted_indices: Vec<u32> = teardown_agent_ids
            .iter()
            .filter_map(|agent_id| app_data.storage.get(*agent_id)?.window_index)
            .collect();
        deleted_indices.sort_unstable_by(|a, b| b.cmp(a));
        deleted_indices.dedup();

        for idx in &deleted_indices {
            if let Err(e) = self.session_manager.kill_window(parent_session, *idx) {
                warn!(session = %parent_session, window_index = *idx, error = %e, "{}", SYNTHESIS_KILL_WINDOW_WARN);
            }
        }

        super::window::adjust_window_indices_after_deletions(
            app_data,
            root_id,
            teardown_root_ids,
            &deleted_indices,
        );

        for descendant_id in teardown_root_ids {
            app_data.storage.remove_with_descendants(*descendant_id);
        }
    }
}

fn next_child_number(
    storage: &crate::agent::Storage,
    parent_id: uuid::Uuid,
    prefix: &str,
) -> usize {
    storage
        .children(parent_id)
        .into_iter()
        .filter_map(|agent| parse_agent_title_number(&agent.title, prefix))
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn parse_agent_title_number(title: &str, prefix: &str) -> Option<usize> {
    let rest = title.strip_prefix(prefix)?.trim_start();
    rest.split_whitespace().next()?.parse().ok()
}
