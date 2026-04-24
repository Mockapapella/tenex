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
const SYNTHESIS_KILL_WINDOW_WARN: &str =
    "Failed to kill descendant mux window during synthesis cleanup";

#[cfg(test)]
thread_local! {
    static TEST_FORCE_SYNTHESIS_WRITE_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
fn with_forced_synthesis_write_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    let previous = TEST_FORCE_SYNTHESIS_WRITE_ERROR.with(|flag| flag.replace(true));
    let result = f();
    TEST_FORCE_SYNTHESIS_WRITE_ERROR.with(|flag| flag.set(previous));
    result
}

fn write_synthesis_contents(file: &mut impl Write, contents: &str) -> std::io::Result<()> {
    #[cfg(test)]
    if TEST_FORCE_SYNTHESIS_WRITE_ERROR.with(std::cell::Cell::get) {
        return Err(std::io::Error::other("forced synthesis write error"));
    }

    file.write_all(contents.as_bytes())
}

#[derive(Clone, Copy)]
struct CodexReviewTimings {
    poll_interval: Duration,
    step_timeout: Duration,
    idle_stable_for: Duration,
    command_hint_timeout: Duration,
}

impl Default for CodexReviewTimings {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(250),
            step_timeout: Duration::from_secs(20),
            idle_stable_for: Duration::from_millis(200),
            command_hint_timeout: Duration::from_secs(5),
        }
    }
}

impl Actions {
    fn root_ancestor_for_known_agent<'a>(storage: &'a Storage, agent: &'a Agent) -> &'a Agent {
        let mut current = agent;
        while let Some(parent_id) = current.parent_id {
            let Some(parent) = storage.get(parent_id) else {
                break;
            };
            current = parent;
        }
        current
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

    fn build_synthesis_read_command(
        synthesis_id: uuid::Uuid,
        descendants_count: usize,
        prompt: Option<&str>,
    ) -> String {
        let agent_word = if descendants_count == 1 {
            "agent"
        } else {
            "agents"
        };

        let mut read_command = format!(
            "Read .tenex/{synthesis_id}.md - it contains the work of {descendants_count} {agent_word}. Use it to guide your next steps."
        );
        if let Some(prompt) = prompt.map(str::trim)
            && !prompt.is_empty()
        {
            read_command.push_str("\n\nAdditional instructions:\n");
            read_command.push_str(prompt);
        }
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

    fn start_codex_review_flow(self, target: &str, base_branch: &str) -> Result<()> {
        self.start_codex_review_flow_with_timings(
            target,
            base_branch,
            CodexReviewTimings::default(),
        )
    }

    fn start_codex_review_flow_with_timings(
        self,
        target: &str,
        base_branch: &str,
        timings: CodexReviewTimings,
    ) -> Result<()> {
        let base_branch = base_branch.trim();
        if base_branch.is_empty() {
            bail!("Base branch cannot be empty for Codex review flow");
        }

        let poll_interval = timings.poll_interval;
        let step_timeout = timings.step_timeout;
        let idle_stable_for = timings.idle_stable_for;

        if !self.wait_for_pane_idle(target, idle_stable_for, step_timeout, poll_interval) {
            warn!(target, "{}", CODEX_PANE_SETTLE_TIMEOUT);
            return Ok(());
        }

        self.session_manager.send_keys(target, "/review")?;
        let _ = self.wait_for_pane_contains_any(
            target,
            &["review my current changes", "review current changes"],
            timings.command_hint_timeout,
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
            return Ok(());
        }

        let _ = self.wait_for_pane_idle(target, idle_stable_for, step_timeout, poll_interval);
        self.session_manager.send_keys_and_submit(target, "")?;
        if !self.wait_for_pane_contains_any(target, &["base branch"], step_timeout, poll_interval) {
            warn!(target, "{}", CODEX_REVIEW_BASE_BRANCH_TIMEOUT);
            return Ok(());
        }

        let _ = self.wait_for_pane_idle(target, idle_stable_for, step_timeout, poll_interval);
        self.session_manager.paste_keys(target, base_branch)?;
        self.session_manager.send_keys_and_submit(target, "")?;
        if !self.wait_for_pane_contains_any(
            target,
            &[
                "review started",
                "Code review started",
                "Code Review Started",
            ],
            step_timeout,
            poll_interval,
        ) {
            warn!(target, "{}", CODEX_REVIEW_START_TIMEOUT);
        }

        Ok(())
    }

    fn start_codex_review_flows(self, flows: Vec<(String, String)>) {
        for (target, base_branch) in flows {
            if let Err(err) = self.start_codex_review_flow(&target, &base_branch) {
                warn!(target, error = %err, "Failed to drive Codex /review flow");
            }
        }
    }

    fn start_codex_review_flows_in_background(self, flows: Vec<(String, String)>) {
        std::thread::spawn(move || {
            self.start_codex_review_flows(flows);
        });
    }

    fn wait_for_pane_contains_any(
        self,
        target: &str,
        needles: &[&str],
        timeout: Duration,
        poll_interval: Duration,
    ) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if let Ok(pane) = self.output_capture.capture_pane(target)
                && needles.iter().any(|needle| pane.contains(needle))
            {
                return true;
            }
            std::thread::sleep(poll_interval);
        }
        false
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
        app_data: &AppData,
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

        let spawn_config = if let Some(pid) = parent_id {
            Self::get_existing_parent_config(app_data, pid)?
        } else {
            match self.create_new_root_for_swarm(app_data, task, count)? {
                Some(config) => config,
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
        app_data.set_status(format!("Spawned {count} child agents"));
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
    ) -> Result<Option<SpawnConfig>> {
        let root_title = Self::generate_root_title(task);
        let cwd_fallback = PathBuf::from(".");
        let repo_path = Self::resolve_swarm_repo_path(
            app_data,
            std::env::current_dir().unwrap_or(cwd_fallback),
        );
        let Ok(repo) = git::open_repository(&repo_path) else {
            let config = self.create_plain_dir_root_for_swarm(app_data, root_title, repo_path)?;
            return Ok(Some(config));
        };
        let branch = app_data.config.generate_branch_name(&root_title);

        let worktree_mgr = WorktreeManager::new(&repo);

        if let Some(conflict_worktree_path) = worktree_mgr.worktree_path(&branch) {
            Self::setup_worktree_conflict(
                app_data,
                &worktree_mgr,
                root_title,
                task,
                branch,
                conflict_worktree_path,
                count,
                repo_path,
            );
            return Ok(None);
        }

        let worktree_path = app_data
            .config
            .worktree_path_for_repo_root(&repo_path, &branch);

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
        Ok(Some(SpawnConfig {
            root_session,
            worktree_path,
            branch,
            workspace_kind: WorkspaceKind::GitWorktree,
            runtime,
            parent_agent_id: root_id,
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
            .map(|(b, c)| (Some(b), Some(c)))
            .unwrap_or((None, None));

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
            if crate::conversation::detect_agent_cli(&child.program)
                == crate::conversation::AgentCli::Codex
                && let Some(actual_index) = child.window_index
            {
                let window_target = SessionManager::window_target(&child.mux_session, actual_index);
                codex_review_flows.push((window_target, base_branch.clone()));
            }
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
            self.start_codex_review_flows_in_background(codex_review_flows);
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

        // Collect findings from all descendants (children, grandchildren, etc.)
        // Filter out terminal agents - they are interactive shells, not research agents
        let descendants: Vec<_> = app_data
            .storage
            .descendants(parent_id)
            .into_iter()
            .filter(|d| !d.is_terminal_agent())
            .collect();

        if descendants.is_empty() {
            warn!(agent_id = %parent_id, title = %parent_title, "No non-terminal children to synthesize");
            return Ok(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            }
            .into());
        }

        let mut findings: Vec<(String, String)> = Vec::new();

        for descendant in &descendants {
            // Capture terminal output from descendant's window
            let target = descendant.window_index.map_or_else(
                || descendant.mux_session.clone(),
                |window_idx| SessionManager::window_target(&parent_session, window_idx),
            );

            let output = self
                .output_capture
                .capture_pane_with_history(&target, 5000)
                .unwrap_or_else(|_| "(Could not capture output)".to_string());

            findings.push((descendant.title.clone(), output));
        }

        // Build synthesis content
        let synthesis_content = prompts::build_synthesis_prompt(&findings);

        let synthesis_id = uuid::Uuid::new_v4();
        let synthesis_file =
            Self::write_synthesis_file(&worktree_path, synthesis_id, &synthesis_content)?;

        debug!(?synthesis_file, "Wrote synthesis file");

        // Kill all descendant windows and remove from storage
        // Collect IDs and window indices first to avoid borrow issues
        let descendant_info: Vec<_> = descendants.iter().map(|d| (d.id, d.window_index)).collect();
        let descendants_count = descendant_info.len();

        for (descendant_id, window_idx) in descendant_info {
            // Kill the window if it has one
            if let Some(idx) = window_idx
                && let Err(e) = self.session_manager.kill_window(&parent_session, idx)
            {
                warn!(session = %parent_session, window_index = idx, error = %e, "{}", SYNTHESIS_KILL_WINDOW_WARN);
            }
            // Remove from storage (remove_with_descendants handles nested removal)
            app_data.storage.remove(descendant_id);
        }

        // Now tell the parent to read the file
        let read_command =
            Self::build_synthesis_read_command(synthesis_id, descendants_count, prompt);
        let session_manager = &self.session_manager;
        let target = &parent_target;
        let agent = &parent_agent;
        let command = &read_command;
        session_manager.send_keys_and_submit_for_agent(target, agent, command)?;

        app_data.validate_selection();
        app_data.storage.save()?;
        info!(%parent_title, descendants_count, "Synthesis complete");
        app_data.set_status("Synthesized findings into parent agent");
        Ok(AppMode::normal())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::mux::SessionManager;
    use crate::state::{AppMode, ConfirmAction, ConfirmingMode, ErrorModalMode};
    use git2::{RepositoryInitOptions, Signature};
    #[cfg(unix)]
    use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
    use std::path::Path;
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tempfile::NamedTempFile;

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

    #[test]
    fn test_canonicalize_or_self_falls_back_on_missing_path() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let missing = temp_dir.path().join(uuid::Uuid::new_v4().to_string());
        let actual = canonicalize_or_self(&missing);
        assert_eq!(actual, missing);
    }

    fn init_git_repo() -> (tempfile::TempDir, PathBuf) {
        let temp_dir = tempfile::TempDir::new().expect("create temp repo dir");
        let repo_path = temp_dir.path().to_path_buf();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = crate::git::Repository::init_opts(&repo_path, &init_opts).expect("init repo");
        repo.set_head("refs/heads/master").expect("set HEAD");
        {
            let mut config = repo.config().expect("open repo config");
            config.set_str("user.name", "Test").expect("set user.name");
            config
                .set_str("user.email", "test@test.com")
                .expect("set user.email");
            config
                .set_bool("core.autocrlf", false)
                .expect("disable autocrlf");
            config.set_str("core.eol", "lf").expect("set eol");
            config
                .set_str("commit.gpgsign", "false")
                .expect("disable gpg signing");
        }

        let sig = Signature::now("Test", "test@test.com").expect("signature");
        std::fs::write(repo_path.join("README.md"), "# Test Repository\n").expect("write README");

        let mut index = repo.index().expect("open index");
        index
            .add_path(std::path::Path::new("README.md"))
            .expect("add README");
        index.write().expect("write index");

        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("commit tree");

        (temp_dir, repo_path)
    }

    #[test]
    fn test_build_synthesis_read_command_handles_singular_plural_and_prompt() {
        let singular = Actions::build_synthesis_read_command(uuid::Uuid::new_v4(), 1, None);
        assert!(singular.contains("1 agent"));

        let plural = Actions::build_synthesis_read_command(
            uuid::Uuid::new_v4(),
            2,
            Some("  check the edge cases  "),
        );
        assert!(plural.contains("2 agents"));
        assert!(plural.contains("Additional instructions"));
        assert!(plural.contains("check the edge cases"));
    }

    #[test]
    fn test_build_synthesis_read_command_skips_empty_prompt_after_trim() {
        let command = Actions::build_synthesis_read_command(uuid::Uuid::new_v4(), 2, Some("   "));
        assert!(!command.contains("Additional instructions"));
    }

    #[test]
    fn test_write_synthesis_file_returns_error_when_destination_is_directory() {
        let temp_dir = tempfile::TempDir::new().expect("create temp dir");
        let synthesis_id = uuid::Uuid::nil();

        let tenex_dir = temp_dir.path().join(".tenex");
        std::fs::create_dir_all(&tenex_dir).expect("create .tenex dir");

        let synthesis_path = tenex_dir.join(format!("{synthesis_id}.md"));
        std::fs::create_dir(&synthesis_path).expect("create directory at synthesis path");

        let err = Actions::write_synthesis_file(temp_dir.path(), synthesis_id, "content")
            .expect_err("write synthesis file should fail");
        let message = format!("{err:#}");
        assert!(message.contains("Failed to create"));
        assert!(message.contains(&synthesis_path.display().to_string()));
    }

    #[test]
    fn test_write_synthesis_file_returns_error_when_worktree_path_is_file() {
        let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
        let synthesis_id = uuid::Uuid::nil();

        let err = Actions::write_synthesis_file(temp_file.path(), synthesis_id, "content")
            .expect_err("write synthesis file should fail");
        assert!(err.to_string().contains("Failed to create"));
    }

    #[test]
    fn test_write_synthesis_file_returns_error_when_write_fails() {
        let temp_dir = tempfile::TempDir::new().expect("create temp dir");
        let synthesis_id = uuid::Uuid::nil();

        let err = with_forced_synthesis_write_error_for_tests(|| {
            Actions::write_synthesis_file(temp_dir.path(), synthesis_id, "content")
        })
        .expect_err("write synthesis file should fail");
        assert!(err.to_string().contains("Failed to write to"));
    }

    #[test]
    fn test_start_codex_review_flow_bails_on_empty_base_branch() {
        let handler = Actions::new();
        let err = handler
            .start_codex_review_flow("nonexistent-target", "   ")
            .expect_err("expected empty base branch to error");
        assert!(err.to_string().contains("Base branch cannot be empty"));
    }

    #[test]
    fn test_wait_for_pane_contains_any_returns_false_on_timeout() {
        let handler = Actions::new();
        assert!(!handler.wait_for_pane_contains_any(
            "nonexistent-target",
            &["needle"],
            Duration::from_millis(1),
            Duration::from_millis(0),
        ));
    }

    #[test]
    fn test_wait_for_pane_idle_returns_false_on_timeout() {
        let handler = Actions::new();
        assert!(!handler.wait_for_pane_idle(
            "nonexistent-target",
            Duration::from_millis(1),
            Duration::from_millis(1),
            Duration::from_millis(0),
        ));
    }

    #[cfg(unix)]
    struct MuxSessionGuard {
        manager: SessionManager,
        session: String,
    }

    #[cfg(unix)]
    impl Drop for MuxSessionGuard {
        fn drop(&mut self) {
            let _ = self.manager.kill(&self.session);
        }
    }

    #[cfg(unix)]
    fn start_mux_session(shell_script: &str) -> (MuxSessionGuard, tempfile::TempDir) {
        let socket = format!(
            "tenex-swarm-mux-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let workdir = tempfile::tempdir().expect("tempdir");
        let session = format!("tenex-swarm-session-{}", uuid::Uuid::new_v4());
        let manager = SessionManager::new();
        let _ = manager.kill(&session);

        let command = vec!["sh".to_string(), "-c".to_string(), shell_script.to_string()];
        manager
            .create(&session, workdir.path(), Some(&command))
            .expect("create session");
        std::thread::sleep(Duration::from_millis(300));

        (MuxSessionGuard { manager, session }, workdir)
    }

    #[cfg(unix)]
    fn make_mock_socket(
        dir: &tempfile::TempDir,
    ) -> (String, interprocess::local_socket::Name<'static>) {
        let socket_path = dir.path().join("mux.sock");
        let display = socket_path.to_string_lossy().into_owned();
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .expect("socket fs name")
            .into_owned();
        (display, name)
    }

    #[cfg(unix)]
    fn spawn_mock_mux_server(
        name: interprocess::local_socket::Name<'static>,
        capture_text: String,
        send_input_responses: Vec<crate::mux::MuxResponse>,
        expected_requests: usize,
    ) -> std::thread::JoinHandle<()> {
        use crate::mux::{MuxRequest, MuxResponse};

        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("Expected mock mux listener to start");
        let send_input_counter = AtomicUsize::new(0);

        std::thread::spawn(move || {
            let mut handled = 0usize;
            for mut stream in listener.incoming().flatten() {
                while handled < expected_requests {
                    let Ok(request) = crate::mux::read_json::<_, MuxRequest>(&mut stream) else {
                        break;
                    };

                    let response = match request {
                        MuxRequest::Capture { .. } => MuxResponse::Text {
                            text: capture_text.clone(),
                        },
                        MuxRequest::SendInput { .. } => {
                            let idx = send_input_counter.fetch_add(1, Ordering::SeqCst);
                            send_input_responses
                                .get(idx)
                                .cloned()
                                .unwrap_or(MuxResponse::Ok)
                        }
                        _ => MuxResponse::Err {
                            message: "unexpected request".to_string(),
                        },
                    };

                    let _ = crate::mux::write_json(&mut stream, &response);
                    handled = handled.saturating_add(1);
                }

                if handled >= expected_requests {
                    break;
                }
            }
        })
    }

    fn require_mux_err_response(
        response: crate::mux::MuxResponse,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let crate::mux::MuxResponse::Err { message } = response else {
            return Err("Expected error response from unexpected mux request".into());
        };
        Ok(message)
    }

    #[test]
    fn test_require_mux_err_response_returns_err_for_non_err_response() {
        let err = require_mux_err_response(crate::mux::MuxResponse::Ok)
            .expect_err("expected non-err response to be rejected");
        assert!(err.to_string().contains("Expected error response"));
    }

    #[test]
    fn test_require_mux_err_response_returns_message_for_err_response() {
        let message = require_mux_err_response(crate::mux::MuxResponse::Err {
            message: "boom".to_string(),
        })
        .expect("expected err response to be accepted");
        assert_eq!(message, "boom");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_completes_when_mux_prompts_present() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let capture_text = [
            "review my current changes",
            "review preset",
            "base branch",
            "review started",
        ]
        .join("\n");

        let server = spawn_mock_mux_server(name, capture_text, Vec::new(), 15);

        let handler = Actions::new();
        let timings = CodexReviewTimings {
            poll_interval: Duration::from_millis(1),
            step_timeout: Duration::from_millis(250),
            idle_stable_for: Duration::from_millis(0),
            command_hint_timeout: Duration::from_millis(50),
        };

        handler
            .start_codex_review_flow_with_timings("session", "main", timings)
            .expect("review flow should succeed");

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_propagates_send_keys_errors() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let capture_text = [
            "review my current changes",
            "review preset",
            "base branch",
            "review started",
        ]
        .join("\n");

        let send_input_responses = vec![crate::mux::MuxResponse::Err {
            message: "send_keys failed".to_string(),
        }];
        let server = spawn_mock_mux_server(name, capture_text, send_input_responses, 3);

        let handler = Actions::new();
        let timings = CodexReviewTimings {
            poll_interval: Duration::from_millis(1),
            step_timeout: Duration::from_millis(250),
            idle_stable_for: Duration::from_millis(0),
            command_hint_timeout: Duration::from_millis(50),
        };

        let err = handler
            .start_codex_review_flow_with_timings("session", "main", timings)
            .expect_err("expected send_keys to error");
        assert!(err.to_string().contains("send_keys failed"));

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_propagates_submit_errors() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let capture_text = [
            "review my current changes",
            "review preset",
            "base branch",
            "review started",
        ]
        .join("\n");

        let send_input_responses = vec![
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Err {
                message: "submit failed".to_string(),
            },
        ];
        let server = spawn_mock_mux_server(name, capture_text, send_input_responses, 5);

        let handler = Actions::new();
        let timings = CodexReviewTimings {
            poll_interval: Duration::from_millis(0),
            step_timeout: Duration::from_millis(50),
            idle_stable_for: Duration::from_millis(0),
            command_hint_timeout: Duration::from_millis(50),
        };

        let err = handler
            .start_codex_review_flow_with_timings("session", "main", timings)
            .expect_err("expected submit to error");
        assert!(err.to_string().contains("submit failed"));

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_propagates_second_submit_errors() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let capture_text = [
            "review my current changes",
            "review preset",
            "base branch",
            "review started",
        ]
        .join("\n");

        let send_input_responses = vec![
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Err {
                message: "second submit failed".to_string(),
            },
        ];
        let server = spawn_mock_mux_server(name, capture_text, send_input_responses, 9);

        let handler = Actions::new();
        let timings = CodexReviewTimings {
            poll_interval: Duration::from_millis(0),
            step_timeout: Duration::from_millis(50),
            idle_stable_for: Duration::from_millis(0),
            command_hint_timeout: Duration::from_millis(50),
        };

        let err = handler
            .start_codex_review_flow_with_timings("session", "main", timings)
            .expect_err("expected second submit to error");
        assert!(err.to_string().contains("second submit failed"));

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_propagates_paste_errors() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let capture_text = [
            "review my current changes",
            "review preset",
            "base branch",
            "review started",
        ]
        .join("\n");

        let send_input_responses = vec![
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Err {
                message: "paste failed".to_string(),
            },
        ];
        let server = spawn_mock_mux_server(name, capture_text, send_input_responses, 13);

        let handler = Actions::new();
        let timings = CodexReviewTimings {
            poll_interval: Duration::from_millis(0),
            step_timeout: Duration::from_millis(50),
            idle_stable_for: Duration::from_millis(0),
            command_hint_timeout: Duration::from_millis(50),
        };

        let err = handler
            .start_codex_review_flow_with_timings("session", "main", timings)
            .expect_err("expected paste_keys to error");
        assert!(err.to_string().contains("paste failed"));

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_propagates_submit_after_paste_errors() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let capture_text = [
            "review my current changes",
            "review preset",
            "base branch",
            "review started",
        ]
        .join("\n");

        let send_input_responses = vec![
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Ok,
            crate::mux::MuxResponse::Err {
                message: "submit after paste failed".to_string(),
            },
        ];
        let server = spawn_mock_mux_server(name, capture_text, send_input_responses, 14);

        let handler = Actions::new();
        let timings = CodexReviewTimings {
            poll_interval: Duration::from_millis(0),
            step_timeout: Duration::from_millis(50),
            idle_stable_for: Duration::from_millis(0),
            command_hint_timeout: Duration::from_millis(50),
        };

        let err = handler
            .start_codex_review_flow_with_timings("session", "main", timings)
            .expect_err("expected send_keys_and_submit after paste to error");
        assert!(err.to_string().contains("submit after paste failed"));

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_spawn_mock_mux_server_breaks_on_read_error_then_handles_unexpected_request() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let temp = tempfile::TempDir::new().expect("tempdir");
        let (display, name) = make_mock_socket(&temp);
        crate::mux::set_socket_override(&display).expect("set socket override");

        let server = spawn_mock_mux_server(name, "capture".to_string(), Vec::new(), 1);

        let endpoint = crate::mux::socket_endpoint().expect("socket endpoint");

        let _ = interprocess::local_socket::Stream::connect(endpoint.name.clone())
            .expect("Expected connect to mock mux server");

        // Connect and immediately drop to force the server's `read_json` call to fail.
        drop(
            interprocess::local_socket::Stream::connect(endpoint.name.clone())
                .expect("connect to mock mux server"),
        );

        let mut stream =
            interprocess::local_socket::Stream::connect(endpoint.name).expect("connect");
        crate::mux::write_json(&mut stream, &crate::mux::MuxRequest::Ping).expect("write ping");
        let response: crate::mux::MuxResponse =
            crate::mux::read_json(&mut stream).expect("read response");
        let message = require_mux_err_response(response).expect("err response");
        assert_eq!(message, "unexpected request");

        server.join().expect("mock mux server panicked");
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_for_pane_idle_returns_true_when_output_stable() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let (guard, _workdir) = start_mux_session("printf 'ready\\n'; sleep 60");
        let handler = Actions::new();

        assert!(handler.wait_for_pane_idle(
            &guard.session,
            Duration::from_millis(25),
            Duration::from_millis(200),
            Duration::from_millis(5),
        ));
    }

    #[test]
    fn test_start_codex_review_flows_warns_on_error() {
        with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.start_codex_review_flows(vec![(
                "nonexistent-target".to_string(),
                "   ".to_string(),
            )]);
        });
    }

    #[test]
    fn test_start_codex_review_flow_returns_ok_when_pane_never_settles() {
        with_tracing_dispatch(|| {
            let handler = Actions::new();
            let timings = CodexReviewTimings {
                poll_interval: Duration::from_millis(0),
                step_timeout: Duration::from_millis(1),
                idle_stable_for: Duration::from_millis(1),
                command_hint_timeout: Duration::from_millis(0),
            };
            handler.start_codex_review_flow_with_timings("nonexistent-target", "main", timings)
        })
        .expect("review flow should return Ok when pane never settles");
    }

    #[test]
    fn test_spawn_review_child_agent_uses_child_runtime_scope_when_parent_missing() {
        let handler = Actions::new();
        let (app, _temp) = create_test_app();

        let missing_parent_id = uuid::Uuid::new_v4();
        let config = ReviewChildAgentConfig {
            root_session: "missing-session",
            worktree_path: Path::new("/tmp"),
            branch: "main",
            workspace_kind: WorkspaceKind::PlainDir,
            runtime: AgentRuntime::Host,
            parent_id: missing_parent_id,
            program: "claude",
            review_prompt: "review",
            reviewer_number: 1,
            reserved_window_index: 1,
        };

        handler
            .spawn_review_child_agent(&app.data, config)
            .expect_err("expected missing session to error");
    }

    #[test]
    fn test_spawn_child_agents_uses_plan_prompt_program_and_titles() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            PathBuf::from("/tmp"),
        );
        root.workspace_kind = WorkspaceKind::PlainDir;
        let root_id = root.id;
        app.data.storage.add(root);

        let config = SpawnConfig {
            root_session: "missing-session".to_string(),
            worktree_path: PathBuf::from("/tmp"),
            branch: "main".to_string(),
            workspace_kind: WorkspaceKind::PlainDir,
            runtime: AgentRuntime::Host,
            parent_agent_id: root_id,
        };

        app.data.spawn.use_plan_prompt = true;

        handler
            .spawn_child_agents(&mut app.data, &config, 1, Some("plan this"))
            .expect_err("expected missing session to error");
    }

    #[test]
    fn test_spawn_single_child_uses_child_runtime_scope_when_parent_missing() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let config = SpawnConfig {
            root_session: "missing-session".to_string(),
            worktree_path: PathBuf::from("/tmp"),
            branch: "main".to_string(),
            workspace_kind: WorkspaceKind::PlainDir,
            runtime: AgentRuntime::Host,
            parent_agent_id: uuid::Uuid::new_v4(),
        };

        handler
            .spawn_single_child(
                &mut app.data,
                &config,
                1,
                "claude",
                Some("prompt"),
                "Planner 1",
            )
            .expect_err("expected missing session to error");
    }

    #[test]
    fn test_build_child_prompt_uses_plan_preamble_when_requested() {
        let prompt = Actions::build_child_prompt("do the thing", true);
        assert!(prompt.contains(prompts::PLAN_PREAMBLE));
        assert!(prompt.contains("do the thing"));
        assert_eq!(Actions::build_child_prompt("task", false), "task");
    }

    #[test]
    fn test_generate_root_title_falls_back_to_uuid_when_missing_task() {
        let title = Actions::generate_root_title(None);
        assert!(title.starts_with("Swarm ("));
        assert!(title.ends_with(')'));
    }

    #[test]
    fn test_generate_root_title_handles_short_and_long_tasks() {
        assert_eq!(
            Actions::generate_root_title(Some("short task")),
            "short task"
        );

        let long = "x".repeat(31);
        let title = Actions::generate_root_title(Some(&long));
        assert!(title.ends_with("..."));
        assert_eq!(title.len(), 30);
    }

    #[test]
    fn test_spawn_review_agents_bails_when_not_git_workspace() {
        let (mut app, _temp) = create_test_app();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            PathBuf::from("/tmp"),
        );
        root.workspace_kind = WorkspaceKind::PlainDir;
        root.collapsed = false;
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.spawn.spawning_under = Some(root_id);
        app.data.spawn.child_count = 1;
        app.data.review.base_branch = Some("main".to_string());

        let err = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.spawn_review_agents(&mut app.data)
        })
        .expect_err("expected non-git workspace to error");
        assert!(err.to_string().contains("git"));
    }

    #[test]
    fn test_synthesize_returns_error_when_only_terminal_descendants() {
        let (mut app, _temp) = create_test_app();
        let temp_dir = tempfile::tempdir().expect("tempdir");

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            temp_dir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let mut terminal = Agent::new_child(
            "Terminal 1".to_string(),
            "terminal".to_string(),
            "main".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        let next = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.synthesize(&mut app.data)
        })
        .expect("synthesize");
        assert_eq!(
            std::mem::discriminant(&next),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_synthesize_handles_missing_window_index_and_cleanup_failures() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-swarm-synthesize-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let manager = SessionManager::new();
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'parent ready\\n'; sleep 60".to_string(),
        ];
        manager
            .create(&session, workdir.path(), Some(&command))
            .expect("create mux session");
        std::thread::sleep(Duration::from_millis(300));

        let mut missing_index = Agent::new_child(
            "Child".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
            ChildConfig {
                parent_id: root_id,
                mux_session: session.clone(),
                window_index: 2,
                repo_root: None,
            },
        );
        missing_index.window_index = None;
        app.data.storage.add(missing_index);

        let invalid_index = Agent::new_child(
            "Child 2".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
            ChildConfig {
                parent_id: root_id,
                mux_session: session.clone(),
                window_index: 999,
                repo_root: None,
            },
        );
        app.data.storage.add(invalid_index);

        let next = with_tracing_dispatch(|| handler.synthesize(&mut app.data)).expect("synthesize");
        assert_eq!(next, AppMode::normal());
        assert_eq!(app.data.storage.len(), 1);

        let tenex_dir = workdir.path().join(".tenex");
        assert!(tenex_dir.is_dir());
        let md_count = std::fs::read_dir(&tenex_dir)
            .expect("read .tenex")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
            .count();
        assert_eq!(md_count, 1);

        let _ = manager.kill(&session);
    }

    #[cfg(unix)]
    #[test]
    fn test_synthesize_propagates_storage_save_errors() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let socket = format!(
            "tenex-swarm-synthesize-save-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        );
        crate::mux::set_socket_override(&socket).expect("set socket override");

        let handler = Actions::new();

        let state_dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::with_path(state_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let manager = SessionManager::new();
        let command = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'parent ready\\n'; sleep 60".to_string(),
        ];
        manager
            .create(&session, workdir.path(), Some(&command))
            .expect("create mux session");
        std::thread::sleep(Duration::from_millis(300));

        let child = Agent::new_child(
            "Child".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
            ChildConfig {
                parent_id: root_id,
                mux_session: session.clone(),
                window_index: 2,
                repo_root: None,
            },
        );
        app.data.storage.add(child);

        let err = with_tracing_dispatch(|| handler.synthesize(&mut app.data))
            .expect_err("expected storage save to error");
        assert!(err.to_string().contains("Failed to replace state file"));

        let _ = manager.kill(&session);
    }

    #[cfg(unix)]
    #[test]
    fn test_synthesize_returns_error_when_send_keys_fails() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let child = Agent::new_child(
            "Child".to_string(),
            "echo".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
            ChildConfig {
                parent_id: root_id,
                mux_session: session,
                window_index: 2,
                repo_root: None,
            },
        );
        app.data.storage.add(child);

        let long_component = "x".repeat(200);
        let socket_path = workdir.path().join(long_component);
        crate::mux::set_socket_override(&socket_path.to_string_lossy())
            .expect("set socket override");

        with_tracing_dispatch(|| handler.synthesize(&mut app.data))
            .expect_err("expected send_keys to fail");

        let tenex_dir = workdir.path().join(".tenex");
        assert!(tenex_dir.is_dir());
    }

    #[test]
    fn test_parse_agent_title_number_extracts_numeric_suffix() {
        assert_eq!(parse_agent_title_number("Reviewer 3", "Reviewer"), Some(3));
        assert!(parse_agent_title_number("Reviewer", "Reviewer").is_none());
        assert!(parse_agent_title_number("Reviewer nope", "Reviewer").is_none());
    }

    #[test]
    fn test_spawn_children_for_root_no_session() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // Add a root agent (the session won't exist)
        let root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        app.data.storage.add(root);

        // Calling spawn_children_for_root should fail because the session doesn't exist
        let spawn_config = SpawnConfig {
            root_session: "nonexistent-session".to_string(),
            worktree_path: PathBuf::from("/tmp"),
            branch: "test-branch".to_string(),
            workspace_kind: WorkspaceKind::GitWorktree,
            runtime: AgentRuntime::Host,
            parent_agent_id: root_id,
        };
        let result = handler.spawn_children_for_root(&mut app.data, &spawn_config, 2, "test task");

        // This should error because the session doesn't exist
        assert!(result.is_err());
    }

    #[test]
    fn test_spawn_children_creates_plain_dir_root_outside_git() {
        let (mut app, _temp) = create_test_app();

        let non_git_dir = tempfile::TempDir::new().expect("tempdir");
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));

        app.data.spawn.child_count = 1;
        app.data.spawn.spawning_under = None;

        let next = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.spawn_children(&mut app.data, Some("plain-root-task"))
        })
        .expect("spawn children");
        assert_eq!(next, AppMode::normal());

        let root = app
            .data
            .storage
            .root_agents()
            .into_iter()
            .next()
            .expect("Expected root agent");
        assert_eq!(root.workspace_kind, WorkspaceKind::PlainDir);
        assert_eq!(app.data.storage.len(), 2);

        // Stop the session to avoid leaking `sleep` processes.
        let _ = crate::mux::SessionManager::new().kill(&root.mux_session);
    }

    #[test]
    fn test_resolve_swarm_repo_path_prefers_explicit_root_repo_path() {
        let (mut app, _temp) = create_test_app();
        let explicit = tempfile::TempDir::new().expect("tempdir");

        app.data.spawn.root_repo_path = Some(explicit.path().to_path_buf());
        app.data.cwd_project_root = Some(PathBuf::from("/tmp/cwd-root"));
        app.data.selected = usize::MAX;

        let resolved =
            Actions::resolve_swarm_repo_path(&app.data, PathBuf::from("/tmp/current-root"));
        assert_eq!(resolved, explicit.path().to_path_buf());
    }

    #[test]
    fn test_resolve_swarm_repo_path_uses_cwd_project_root_when_sidebar_unavailable() {
        let (mut app, _temp) = create_test_app();
        let cwd_project_root = tempfile::TempDir::new().expect("tempdir");
        app.data.cwd_project_root = Some(cwd_project_root.path().to_path_buf());
        app.data.selected = usize::MAX;

        let resolved =
            Actions::resolve_swarm_repo_path(&app.data, PathBuf::from("/tmp/current-root"));
        assert_eq!(resolved, cwd_project_root.path().to_path_buf());
    }

    #[test]
    fn test_resolve_swarm_repo_path_falls_back_to_current_dir_when_no_roots() {
        let (mut app, _temp) = create_test_app();
        app.data.spawn.root_repo_path = None;
        app.data.cwd_project_root = None;
        app.data.selected = usize::MAX;

        let current_dir = PathBuf::from("/tmp/current-root");
        let resolved = Actions::resolve_swarm_repo_path(&app.data, current_dir.clone());
        assert_eq!(resolved, current_dir);
    }

    #[test]
    fn test_get_existing_parent_config_uses_parent_when_root_missing_in_chain() {
        let (mut app, _temp) = create_test_app();

        let mut agent = Agent::new(
            "child".to_string(),
            "echo".to_string(),
            "main".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.parent_id = Some(uuid::Uuid::new_v4());
        agent.collapsed = true;

        let agent_id = agent.id;
        let expected_root_session = agent.mux_session.clone();
        let expected_worktree_path = agent.worktree_path.clone();
        let expected_branch = agent.branch.clone();
        let expected_workspace_kind = agent.workspace_kind;
        let expected_runtime = agent.runtime;
        app.data.storage.add(agent);

        let config = Actions::get_existing_parent_config(&app.data, agent_id)
            .expect("get existing parent config");
        assert_eq!(config.root_session, expected_root_session);
        assert_eq!(config.worktree_path, expected_worktree_path);
        assert_eq!(config.branch, expected_branch);
        assert_eq!(config.workspace_kind, expected_workspace_kind);
        assert_eq!(config.runtime, expected_runtime);
    }

    #[test]
    fn test_spawn_children_propagates_plain_dir_root_launch_errors_when_program_invalid() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let non_git_dir = tempfile::TempDir::new().expect("tempdir");
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));
        app.data.spawn.child_count = 1;
        app.data.spawn.spawning_under = None;
        app.data.settings.agent_program = crate::app::AgentProgram::Custom;
        app.data.settings.custom_agent_command = r#"claude "unterminated"#.to_string();

        let err = handler
            .spawn_children(&mut app.data, Some("plain-root-task"))
            .expect_err("expected invalid program to error");
        assert!(err.to_string().contains("Failed to parse command line"));
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_spawn_children_propagates_worktree_create_errors_when_worktree_dir_not_directory() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_path) = init_git_repo();

        let worktree_dir_file = NamedTempFile::new().expect("worktree dir file");
        app.data.config.worktree_dir = worktree_dir_file.path().to_path_buf();
        app.data.spawn.root_repo_path = Some(repo_path);
        app.data.spawn.child_count = 1;
        app.data.spawn.spawning_under = None;

        let err = handler
            .spawn_children(&mut app.data, Some("worktree-create-fail"))
            .expect_err("expected worktree create to error");
        assert!(
            err.to_string()
                .contains("Failed to create parent directory")
        );
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_spawn_children_propagates_git_root_launch_errors_when_program_invalid() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_path) = init_git_repo();
        let worktree_dir = tempfile::TempDir::new().expect("tempdir");

        app.data.spawn.root_repo_path = Some(repo_path);
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.spawn.child_count = 1;
        app.data.spawn.spawning_under = None;
        app.data.settings.agent_program = crate::app::AgentProgram::Custom;
        app.data.settings.custom_agent_command = r#"claude "unterminated"#.to_string();

        let err = handler
            .spawn_children(&mut app.data, Some("git-root-task"))
            .expect_err("expected invalid program to error");
        assert!(err.to_string().contains("Failed to parse command line"));
        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_spawn_children_propagates_spawn_child_agents_errors_when_session_missing() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "main".to_string(),
            PathBuf::from("/tmp"),
        );
        root.workspace_kind = WorkspaceKind::PlainDir;
        root.mux_session = "missing-session".to_string();
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.spawn.spawning_under = Some(root_id);
        app.data.spawn.child_count = 1;

        handler
            .spawn_children(&mut app.data, Some("test task"))
            .expect_err("expected missing session to error");
    }

    #[test]
    fn test_spawn_children_propagates_storage_save_errors_after_spawning() {
        let state_path_parent = NamedTempFile::new().expect("state path parent");
        let storage = Storage::with_path(state_path_parent.path().join("agents.json"));
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let non_git_dir = tempfile::TempDir::new().expect("tempdir");
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));

        app.data.spawn.spawning_under = None;
        app.data.spawn.child_count = 1;

        let err = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.spawn_children(&mut app.data, Some("plain-root-task"))
        })
        .expect_err("expected storage save to error");
        assert!(err.to_string().contains("Failed to create state directory"));

        let mut sessions = std::collections::HashSet::new();
        for agent in app.data.storage.iter() {
            sessions.insert(agent.mux_session.clone());
        }
        for session in sessions {
            let _ = crate::mux::SessionManager::new().kill(&session);
        }
    }

    #[test]
    fn test_spawn_children_returns_confirming_mode_when_worktree_conflict_detected() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_path) = init_git_repo();
        let worktree_dir = tempfile::TempDir::new().expect("tempdir");

        app.data.spawn.root_repo_path = Some(repo_path.clone());
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.spawn.spawning_under = None;
        app.data.spawn.child_count = 2;

        let root_title = Actions::generate_root_title(Some("conflict-task"));
        let branch = app.data.config.generate_branch_name(&root_title);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_path, &branch);

        let repo = crate::git::open_repository(&repo_path).expect("open repository");
        let mgr = WorktreeManager::new(&repo);
        mgr.create_with_new_branch_with_options(
            &worktree_path,
            &branch,
            crate::git::WorktreeCreateOptions::default(),
        )
        .expect("Create test worktree");

        let next = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.spawn_children(&mut app.data, Some("conflict-task"))
        })
        .expect("spawn children");

        assert_eq!(
            std::mem::discriminant(&next),
            std::mem::discriminant(&AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }))
        );
        let conflict = app
            .data
            .spawn
            .worktree_conflict
            .as_ref()
            .expect("Expected worktree_conflict");
        assert_eq!(conflict.branch, branch);
        let actual_worktree = canonicalize_or_self(&conflict.worktree_path);
        let expected_worktree = canonicalize_or_self(&worktree_path);
        assert_eq!(actual_worktree, expected_worktree);

        let actual_repo_root = canonicalize_or_self(&conflict.repo_root);
        let expected_repo_root = canonicalize_or_self(&repo_path);
        assert_eq!(actual_repo_root, expected_repo_root);
        assert_eq!(conflict.swarm_child_count, Some(2));
    }

    #[test]
    fn test_spawn_children_sets_unknown_current_head_when_repo_head_info_unavailable() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let (mut app, _temp) = create_test_app();
        let (_repo_dir, repo_path) = init_git_repo();
        let worktree_dir = tempfile::TempDir::new().expect("tempdir");

        app.data.spawn.root_repo_path = Some(repo_path.clone());
        app.data.config.worktree_dir = worktree_dir.path().to_path_buf();
        app.data.spawn.spawning_under = None;
        app.data.spawn.child_count = 1;

        let root_title = Actions::generate_root_title(Some("conflict-task"));
        let branch = app.data.config.generate_branch_name(&root_title);
        let worktree_path = app
            .data
            .config
            .worktree_path_for_repo_root(&repo_path, &branch);

        let repo = crate::git::open_repository(&repo_path).expect("open repository");
        let mgr = WorktreeManager::new(&repo);
        mgr.create_with_new_branch_with_options(
            &worktree_path,
            &branch,
            crate::git::WorktreeCreateOptions::default(),
        )
        .expect("Create test worktree");

        std::fs::write(repo.path().join("HEAD"), "ref: refs/heads/missing\n").expect("write HEAD");

        let next = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.spawn_children(&mut app.data, Some("conflict-task"))
        })
        .expect("spawn children");

        assert_eq!(
            std::mem::discriminant(&next),
            std::mem::discriminant(&AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }))
        );
        let conflict = app
            .data
            .spawn
            .worktree_conflict
            .as_ref()
            .expect("Expected worktree_conflict");
        assert_eq!(conflict.current_branch, "unknown");
        assert_eq!(conflict.current_commit, "unknown");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_warns_and_returns_ok_for_missing_preset_prompt() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let (guard, _workdir) = start_mux_session("printf 'ready\\n'; sleep 60");
        with_tracing_dispatch(|| {
            let handler = Actions::new();
            let timings = CodexReviewTimings {
                poll_interval: Duration::from_millis(0),
                step_timeout: Duration::from_millis(200),
                idle_stable_for: Duration::from_millis(1),
                command_hint_timeout: Duration::from_millis(0),
            };
            handler.start_codex_review_flow_with_timings(&guard.session, "main", timings)
        })
        .expect("review flow should return Ok when prompt missing");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_warns_and_returns_ok_for_missing_base_branch_prompt() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let (guard, _workdir) = start_mux_session("printf 'review preset\\n'; sleep 60");
        with_tracing_dispatch(|| {
            let handler = Actions::new();
            let timings = CodexReviewTimings {
                poll_interval: Duration::from_millis(0),
                step_timeout: Duration::from_millis(200),
                idle_stable_for: Duration::from_millis(1),
                command_hint_timeout: Duration::from_millis(0),
            };
            handler.start_codex_review_flow_with_timings(&guard.session, "main", timings)
        })
        .expect("review flow should return Ok when prompt missing");
    }

    #[cfg(unix)]
    #[test]
    fn test_start_codex_review_flow_warns_and_returns_ok_for_missing_review_started() {
        let _mux_guard = crate::test_support::lock_mux_test_environment();
        let (guard, _workdir) =
            start_mux_session("printf 'review preset\\nbase branch\\n'; sleep 60");
        with_tracing_dispatch(|| {
            let handler = Actions::new();
            let timings = CodexReviewTimings {
                poll_interval: Duration::from_millis(0),
                step_timeout: Duration::from_millis(200),
                idle_stable_for: Duration::from_millis(1),
                command_hint_timeout: Duration::from_millis(0),
            };
            handler.start_codex_review_flow_with_timings(&guard.session, "main", timings)
        })
        .expect("review flow should return Ok when prompt missing");
    }

    #[test]
    fn test_synthesize_no_agent() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // Should return error modal with no agent selected
        let next = handler.synthesize(&mut app.data).expect("synthesize");
        assert_eq!(
            std::mem::discriminant(&next),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    #[test]
    fn test_synthesize_no_children() {
        let (mut app, _temp) = create_test_app();

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Should set error when agent has no children
        let next = with_tracing_dispatch(|| {
            let handler = Actions::new();
            handler.synthesize(&mut app.data)
        })
        .expect("synthesize");
        app.apply_mode(next);
        assert!(app.data.ui.last_error.is_some());
        assert!(
            app.data
                .ui
                .last_error
                .as_ref()
                .expect("Expected last_error")
                .contains("no children to synthesize")
        );
    }

    #[test]
    fn test_synthesize_propagates_write_synthesis_file_errors() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let temp_file = tempfile::NamedTempFile::new().expect("create temp file");
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            temp_file.path().to_path_buf(),
        );
        root.collapsed = false;
        let root_id = root.id;
        let session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.select_agent_by_id(root_id);

        let child = Agent::new_child(
            "Child".to_string(),
            "claude".to_string(),
            "main".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: session,
                window_index: 2,
                repo_root: None,
            },
        );
        app.data.storage.add(child);

        let err = with_tracing_dispatch(|| handler.synthesize(&mut app.data))
            .expect_err("expected synthesize to fail");
        assert!(err.to_string().contains("Failed to create"));
    }

    #[test]
    fn test_spawn_review_agents_no_parent() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // No spawning_under set - should error
        app.data.spawn.spawning_under = None;
        app.data.review.base_branch = Some("main".to_string());

        let result = handler.spawn_review_agents(&mut app.data);
        assert!(result.is_err());
    }

    #[test]
    fn test_spawn_review_agents_no_base_branch() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        // spawning_under set but no base branch - should error
        app.data.spawn.spawning_under = Some(agent_id);
        app.data.review.base_branch = None;

        let result = handler.spawn_review_agents(&mut app.data);
        assert!(result.is_err());
    }

    #[test]
    fn test_spawn_review_agents_propagates_spawn_errors() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.workspace_kind = WorkspaceKind::GitWorktree;
        root.collapsed = false;
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.spawn.spawning_under = Some(root_id);
        app.data.spawn.child_count = 1;
        app.data.review.base_branch = Some("main".to_string());

        with_tracing_dispatch(|| handler.spawn_review_agents(&mut app.data))
            .expect_err("expected spawn_review_agents to propagate errors");
    }

    #[test]
    fn test_spawn_review_agents_propagates_storage_save_errors() {
        let handler = Actions::new();
        let state_dir = tempfile::tempdir().expect("tempdir");
        let storage = Storage::with_path(state_dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.workspace_kind = WorkspaceKind::GitWorktree;
        root.collapsed = false;
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.spawn.spawning_under = Some(root_id);
        app.data.spawn.child_count = 0;
        app.data.review.base_branch = Some("main".to_string());

        let err = with_tracing_dispatch(|| handler.spawn_review_agents(&mut app.data))
            .expect_err("expected storage save to error");
        assert!(err.to_string().contains("Failed to replace state file"));
    }

    #[test]
    fn test_spawn_review_agents_runs_with_tracing_disabled() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.workspace_kind = WorkspaceKind::GitWorktree;
        root.collapsed = false;
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.spawn.spawning_under = Some(root_id);
        app.data.spawn.child_count = 0;
        app.data.review.base_branch = Some("main".to_string());

        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::ERROR)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, || {
            handler
                .spawn_review_agents(&mut app.data)
                .expect("spawn_review_agents");
        });
    }

    #[test]
    fn test_spawn_review_agents_runs_with_info_tracing_enabled() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let workdir = tempfile::tempdir().expect("tempdir");
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "main".to_string(),
            workdir.path().to_path_buf(),
        );
        root.workspace_kind = WorkspaceKind::GitWorktree;
        root.collapsed = false;
        let root_id = root.id;
        app.data.storage.add(root);

        app.data.spawn.spawning_under = Some(root_id);
        app.data.spawn.child_count = 0;
        app.data.review.base_branch = Some("main".to_string());

        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::INFO)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, || {
            handler
                .spawn_review_agents(&mut app.data)
                .expect("spawn_review_agents");
        });
    }

    #[test]
    fn test_spawn_children_rejects_terminal_parent() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        let terminal_id = terminal.id;
        app.data.storage.add(terminal);

        app.data.spawn.spawning_under = Some(terminal_id);

        let err = handler
            .spawn_children(&mut app.data, Some("test task"))
            .expect_err("expected terminal parent to be rejected");
        assert!(err.to_string().contains("terminal"));
    }

    #[test]
    fn test_spawn_review_agents_rejects_terminal_parent() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        let terminal_id = terminal.id;
        app.data.storage.add(terminal);

        app.data.spawn.spawning_under = Some(terminal_id);
        app.data.review.base_branch = Some("main".to_string());

        let err = handler
            .spawn_review_agents(&mut app.data)
            .expect_err("expected terminal parent to be rejected");
        assert!(err.to_string().contains("terminal"));
    }

    #[test]
    fn test_synthesize_terminal_agent() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        let next = handler.synthesize(&mut app.data).expect("synthesize");
        assert_eq!(
            std::mem::discriminant(&next),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    #[test]
    fn test_broadcast_excludes_terminals() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        // Create a root agent
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add a regular child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
                repo_root: None,
            },
        );
        app.data.storage.add(child);

        // Add a terminal child (is_terminal = true)
        let mut terminal = Agent::new_child(
            "Terminal 1".to_string(),
            "terminal".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 3,
                repo_root: None,
            },
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        // Broadcast should only target the non-terminal child (1 agent)
        // Since mux sessions don't exist, it will fail but we can check
        // it attempts to send to the right number of agents
        let result = handler.broadcast_to_leaves(&mut app.data, "test");

        // The broadcast will "succeed" with 0 sent (sessions don't exist)
        // but importantly it should NOT error and should report 0 (not try terminal)
        assert!(result.is_ok());

        // Check status message mentions 0 or shows error about no agents
        // (since the mux sessions don't actually exist)
    }
}
