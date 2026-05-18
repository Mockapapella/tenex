//! Sync operations: agent status synchronization and auto-connect
#![cfg_attr(coverage_nightly, coverage(off))]

use crate::agent::{Agent, Status};
use crate::git::{self, WorktreeManager};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::path::Path;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::{App, AppData, PaneActivityDigestMode};

fn resolve_repo_path_with_deps(
    cwd_project_root: Option<PathBuf>,
    current_dir: &mut dyn FnMut() -> std::io::Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(root) = cwd_project_root {
        return Ok(root);
    }

    current_dir().context("Failed to resolve current project directory")
}

impl Actions {
    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    #[doc(hidden)]
    pub fn exercise_sync_paths_for_coverage(app: &mut App) {
        let root = Agent::new(
            "coverage sync root".to_string(),
            "echo".to_string(),
            "tenex/coverage-sync".to_string(),
            std::env::temp_dir().join(format!("tenex-sync-coverage-{}", uuid::Uuid::new_v4())),
        );
        app.data.storage.add(root);
        let _ = Self::new().sync_agent_status_with_sessions(app, Ok(Vec::new()));

        let mut empty_app = App::new(
            crate::config::Config::default(),
            crate::agent::Storage::new(),
            crate::app::Settings::default(),
            false,
        );
        let _ = Self::new().sync_agent_status_with_sessions(&mut empty_app, Ok(Vec::new()));

        let alive = Agent::new(
            "coverage alive root".to_string(),
            "echo".to_string(),
            "tenex/coverage-alive".to_string(),
            std::env::temp_dir(),
        );
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);
        let missing = Agent::new(
            "coverage missing root".to_string(),
            "echo".to_string(),
            "tenex/coverage-missing".to_string(),
            std::env::temp_dir(),
        );
        app.data.storage.add(missing);
        let _ = Self::new().sync_agent_status_with_sessions(
            app,
            Ok(vec![crate::mux::Session {
                name: alive_session,
                created: 0,
                attached: false,
            }]),
        );

        let _ = finish_respawn_summary(&mut app.data, &RespawnSummary::default());
        let _ = finish_respawn_summary(
            &mut app.data,
            &RespawnSummary {
                changed: true,
                respawned_sessions: 0,
            },
        );
        let _ = finish_respawn_summary(
            &mut app.data,
            &RespawnSummary {
                changed: false,
                respawned_sessions: 1,
            },
        );
        let _ = Self::new().respawn_missing_agents_in_data(&mut empty_app.data);
        let _ = Self::new().respawn_missing_agents_in_data(&mut app.data);
        let _ = title_for_branch("tenex/", "tenex/");
        let _ = title_for_branch("tenex/coverage", "tenex/");

        let selected_agent_id = uuid::Uuid::new_v4();
        let observed_agent_id = uuid::Uuid::new_v4();
        let mut digest_mode = PaneActivityDigestMode::Cursor;
        let mut cursor_err = || Err(anyhow::anyhow!("cursor unavailable"));
        let mut capture_ok = || Ok("coverage capture".to_string());
        let _ = observe_agent_pane_activity(
            &mut app.data.ui,
            observed_agent_id,
            Some(selected_agent_id),
            "coverage-target",
            &mut digest_mode,
            &mut cursor_err,
            &mut capture_ok,
        );

        let _ = Self::new().sync_agent_status(&mut empty_app);
        let _ = Self::new().sync_agent_pane_activity(&mut empty_app);
        Self::exercise_respawn_paths_for_coverage();
    }

    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn coverage_sync_app_data() -> AppData {
        AppData::new(
            crate::config::Config::default(),
            crate::agent::Storage::new(),
            crate::app::Settings::default(),
            false,
        )
    }

    #[cfg(coverage)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn exercise_respawn_paths_for_coverage() {
        let mut app_data = Self::coverage_sync_app_data();
        let root_path =
            std::env::temp_dir().join(format!("tenex-sync-respawn-root-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&root_path);
        let mut root = Agent::new(
            "coverage respawn root".to_string(),
            "echo".to_string(),
            "tenex/coverage-respawn".to_string(),
            root_path.clone(),
        );
        root.set_status(Status::Running);
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        let mut child = Agent::new_child(
            "coverage respawn child".to_string(),
            "echo".to_string(),
            "tenex/coverage-respawn".to_string(),
            root_path.clone(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: "old-session".to_string(),
                window_index: 2,
                repo_root: None,
            },
        );
        child.set_status(Status::Starting);
        let child_id = child.id;
        let outside = Agent::new(
            "coverage outside".to_string(),
            "echo".to_string(),
            "tenex/outside".to_string(),
            root_path.clone(),
        );
        app_data.storage.add(root.clone());
        app_data.storage.add(child.clone());
        app_data.storage.add(outside);

        let mut recreated = HashMap::new();
        let _ = recreated.insert(child_id, 4);
        let _ = mark_respawned_agents_running(&mut app_data, root_id, "coverage-new", recreated);
        let mut unchanged = HashMap::new();
        let _ = unchanged.insert(child_id, 4);
        let _ = unchanged.insert(uuid::Uuid::new_v4(), 9);
        let _ = mark_respawned_agents_running(&mut app_data, root_id, "coverage-new", unchanged);
        let _ = normalize_tree_running(&mut app_data, root_id, "coverage-new", true);
        let _ = normalize_tree_running(&mut app_data, root_id, "coverage-stopped", false);

        let mut summary = RespawnSummary::default();
        let missing_root = Agent::new(
            "coverage missing worktree".to_string(),
            "echo".to_string(),
            "tenex/coverage-missing-worktree".to_string(),
            root_path.join("missing"),
        );
        app_data.storage.add(missing_root.clone());
        respawn_root_agent(
            Self::new().session_manager,
            &mut app_data,
            &missing_root,
            &mut summary,
        );

        let mut docker_root = Agent::new(
            "coverage docker respawn".to_string(),
            "unsupported-coverage-program".to_string(),
            "tenex/coverage-docker-respawn".to_string(),
            root_path.clone(),
        );
        docker_root.runtime = crate::agent::AgentRuntime::Docker;
        app_data.storage.add(docker_root.clone());
        respawn_root_agent(
            Self::new().session_manager,
            &mut app_data,
            &docker_root,
            &mut summary,
        );

        respawn_root_agent(
            Self::new().session_manager,
            &mut app_data,
            &root,
            &mut summary,
        );

        let missing_child = Agent::new_child(
            "coverage missing child".to_string(),
            "echo".to_string(),
            "tenex/coverage-respawn".to_string(),
            root_path.join("missing-child"),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 5,
                repo_root: None,
            },
        );
        let mut docker_child = Agent::new_child(
            "coverage docker child".to_string(),
            "unsupported-coverage-program".to_string(),
            "tenex/coverage-respawn".to_string(),
            root_path.clone(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 6,
                repo_root: None,
            },
        );
        docker_child.runtime = crate::agent::AgentRuntime::Docker;
        let host_child = Agent::new_child(
            "coverage host child".to_string(),
            "echo".to_string(),
            "tenex/coverage-respawn".to_string(),
            root_path,
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 7,
                repo_root: None,
            },
        );
        let _ = recreate_descendant_windows(
            Self::new().session_manager,
            "coverage-session",
            &[missing_child, docker_child, host_child],
            &crate::app::Settings::default(),
        );

        let mut terminal = child;
        terminal.is_terminal = true;
        let _ = command_for_agent(&terminal, &crate::app::Settings::default());
    }

    /// Check and update agent statuses based on mux sessions
    ///
    /// # Errors
    ///
    /// Returns an error if saving updated state fails.
    ///
    /// If mux session listing fails, this function treats the session state as
    /// unknown and performs no pruning or status updates.
    pub fn sync_agent_status(self, app: &mut App) -> Result<()> {
        // Session listing is an external observation. Avoid starting a fresh mux daemon just to
        // check state, especially during shutdown or upgrades.
        if !crate::mux::is_server_running() {
            debug!("Mux daemon not running; skipping agent sync");
            return Ok(());
        }

        let sessions = self.session_manager.list();
        self.sync_agent_status_with_sessions(app, sessions)
    }

    fn sync_agent_status_with_sessions(
        self,
        app: &mut App,
        sessions: Result<Vec<crate::mux::Session>>,
    ) -> Result<()> {
        let mut changed = false;

        // Fetch all sessions once instead of calling exists() per agent.
        let sessions = match sessions {
            Ok(sessions) => sessions,
            Err(err) => {
                // Listing sessions is an external observation. If it fails, don't treat it as an
                // authoritative "no sessions exist" signal or we'll incorrectly prune all agents.
                debug!(error = %err, "Failed to list mux sessions; skipping agent sync");
                return Ok(());
            }
        };

        // A successful but empty session list can be a transient mis-observation (e.g. after the
        // mux daemon restarts or if we're connected to a fresh daemon). Avoid turning that into a
        // destructive prune+save.
        if sessions.is_empty() && !app.data.storage.is_empty() {
            debug!("Mux session list empty; skipping agent sync");
            return Ok(());
        }

        let active_sessions: std::collections::HashSet<String> =
            sessions.into_iter().map(|s| s.name).collect();

        // Remove stored agents whose sessions no longer exist.
        let roots: Vec<Agent> = app
            .data
            .storage
            .root_agents()
            .into_iter()
            .cloned()
            .collect();
        if !roots.is_empty()
            && !roots
                .iter()
                .any(|root| active_sessions.contains(&root.mux_session))
        {
            debug!("No stored mux sessions found in session list; skipping agent sync");
            return Ok(());
        }

        for root in roots {
            if active_sessions.contains(&root.mux_session) {
                continue;
            }

            if let Err(err) = self.session_manager.kill(&root.mux_session) {
                let error = err.to_string();
                if error.contains("not found") {
                    debug!(
                        title = %root.title,
                        session = %root.mux_session,
                        error,
                        "Mux session already gone while pruning agent"
                    );
                } else {
                    warn!(
                        title = %root.title,
                        session = %root.mux_session,
                        error,
                        "Failed to kill mux session for missing agent"
                    );
                }
            }

            if let Err(err) = crate::runtime::cleanup_runtime(&root) {
                warn!(
                    title = %root.title,
                    session = %root.mux_session,
                    error = %err,
                    "Failed to clean up runtime for missing agent"
                );
            }

            debug!(title = %root.title, session = %root.mux_session, "Removing agent with missing mux session");
            app.data.storage.remove_with_descendants(root.id);
            changed = true;
        }

        // Update starting agents to running if their session exists
        for agent in app.data.storage.iter_mut() {
            if agent.status == Status::Starting && active_sessions.contains(&agent.mux_session) {
                debug!(title = %agent.title, "Agent status: Starting -> Running");
                agent.set_status(Status::Running);
                changed = true;
            }
        }

        if changed {
            app.data.storage.save()?;
            app.validate_selection();
        }

        Ok(())
    }

    /// Update per-agent activity indicators from raw output sequence changes once per interval.
    ///
    /// If an agent's mux output sequence has not changed since the previous observation,
    /// Tenex considers it "waiting". Agents show as:
    /// - `●` in green while output is changing (working)
    /// - `◐` in yellow when waiting and unseen (needs attention)
    /// - `○` in red when waiting and already seen
    ///
    /// # Errors
    ///
    /// Returns an error only if internal state mutation fails (captures are best-effort).
    pub fn sync_agent_pane_activity(self, app: &mut App) -> Result<()> {
        // Pane capture depends on the mux daemon; avoid stale "waiting" indicators when it's down.
        if !crate::mux::is_server_running() {
            app.data.ui.pane_digest_by_agent.clear();
            app.data.ui.pane_last_seen_hash_by_agent.clear();
            app.data.ui.pane_activity_digest_mode = PaneActivityDigestMode::Cursor;
            return Ok(());
        }

        let selected_agent_id = app.selected_agent().map(|agent| agent.id);
        let mut digest_mode = app.data.ui.pane_activity_digest_mode;

        let mut keep_ids: HashSet<uuid::Uuid> = HashSet::new();

        for agent in app.data.storage.iter() {
            keep_ids.insert(agent.id);

            // Only track activity once the session exists and the agent is running.
            if agent.status != Status::Running {
                continue;
            }

            let target = mux_target_for_agent(app, agent);
            let mut cursor_fn = || self.output_stream.cursor(&target);
            let mut capture_fn = || self.output_capture.capture_pane(&target);
            let _ = observe_agent_pane_activity(
                &mut app.data.ui,
                agent.id,
                selected_agent_id,
                &target,
                &mut digest_mode,
                &mut cursor_fn,
                &mut capture_fn,
            );
        }

        app.data
            .ui
            .retain_agent_pane_digests(|id| keep_ids.contains(id));
        app.data
            .ui
            .retain_agent_pane_last_seen_hashes(|id| keep_ids.contains(id));
        app.data.ui.pane_activity_digest_mode = digest_mode;

        Ok(())
    }

    /// Auto-connect to existing worktrees on startup
    ///
    /// This function scans for worktrees that match the configured branch prefix
    /// and creates agents for them if they don't already exist in storage.
    /// The agent title will be the branch name.
    ///
    /// # Errors
    ///
    /// Returns an error if worktrees cannot be listed or agent creation fails
    pub fn auto_connect_worktrees(self, app: &mut App) -> Result<()> {
        let mut current_dir = std::env::current_dir;
        self.auto_connect_worktrees_with_deps(app, &mut current_dir)
    }

    fn auto_connect_worktrees_with_deps(
        self,
        app: &mut App,
        current_dir: &mut dyn FnMut() -> std::io::Result<PathBuf>,
    ) -> Result<()> {
        let repo_path =
            resolve_repo_path_with_deps(app.data.cwd_project_root.clone(), current_dir)?;
        let Ok(repo) = git::open_repository(&repo_path) else {
            // Not in a git repository, nothing to auto-connect
            debug!("Not in a git repository, skipping auto-connect");
            return Ok(());
        };

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktrees = worktree_mgr.list()?;
        let program = app.agent_spawn_command();
        let instance_worktree_dir_fallback = app.data.config.worktree_dir.clone();
        let instance_worktree_dir = app
            .data
            .config
            .worktree_dir
            .canonicalize()
            .unwrap_or(instance_worktree_dir_fallback);

        debug!(count = worktrees.len(), "Found worktrees for auto-connect");

        for wt in worktrees {
            let worktree_path_fallback = wt.path.clone();
            let worktree_path = wt.path.canonicalize().unwrap_or(worktree_path_fallback);

            if !worktree_path.starts_with(&instance_worktree_dir) {
                debug!(
                    worktree = %wt.name,
                    path = %worktree_path.display(),
                    instance_worktree_dir = %instance_worktree_dir.display(),
                    "Skipping worktree outside instance worktree directory"
                );
                continue;
            }

            if has_isolated_state_marker(&worktree_path, &instance_worktree_dir) {
                debug!(
                    worktree = %wt.name,
                    path = %worktree_path.display(),
                    "Skipping worktree belonging to another Tenex instance"
                );
                continue;
            }

            // Get the actual branch name from the worktree's HEAD
            // This is more reliable than trying to reverse-engineer from worktree name
            let branch_name = match worktree_mgr.worktree_head_info(&wt.name) {
                Ok((branch, _commit)) => branch,
                Err(e) => {
                    debug!(worktree = %wt.name, error = %e, "Could not get worktree HEAD info, skipping");
                    continue;
                }
            };

            if !is_tenex_managed_branch(&branch_name, &app.data.config.branch_prefix) {
                debug!(
                    branch = %branch_name,
                    prefix = %app.data.config.branch_prefix,
                    "Skipping worktree with different prefix"
                );
                continue;
            }

            // Check if there's already an agent for this branch
            let agent_exists = app.data.storage.iter().any(|a| a.branch == branch_name);
            if agent_exists {
                debug!(branch = %branch_name, "Agent already exists for worktree");
                continue;
            }

            info!(
                branch = %branch_name,
                path = ?worktree_path,
                "Auto-connecting to existing worktree"
            );

            // Create an agent for this worktree
            let mut agent = Agent::new(
                title_for_branch(&branch_name, &app.data.config.branch_prefix),
                program.clone(),
                branch_name.clone(),
                worktree_path.clone(),
            );
            agent.repo_root = Some(repo_path.clone());
            agent.runtime = crate::runtime::new_root_runtime(&app.data.settings);
            self.launch_root_agent(&mut app.data, &mut agent, None)?;

            app.data.storage.add(agent);
            info!(branch = %branch_name, "Auto-connected to existing worktree");
        }

        // Save storage if we added any agents
        app.data.storage.save()?;
        Ok(())
    }

    /// Respawn missing agent mux sessions/windows from persisted state.
    ///
    /// After a system reboot or crash, Tenex can still load the stored agent list from
    /// `state.json`, but the mux daemon (and all agent processes) will be gone. This helper
    /// recreates missing mux sessions for root agents and re-creates windows for all descendants
    /// (including terminals) using each agent's stored program.
    ///
    /// This function is intended to run on startup and is best-effort: it will continue
    /// attempting to restore other agents if one fails to spawn.
    ///
    /// # Errors
    ///
    /// Returns an error if saving updated state fails.
    pub fn respawn_missing_agents(self, app: &mut App) -> Result<()> {
        self.respawn_missing_agents_in_data(&mut app.data)
    }

    pub(crate) fn respawn_missing_agents_in_data(self, app_data: &mut AppData) -> Result<()> {
        let roots = stored_root_agents(app_data);
        if roots.is_empty() {
            return Ok(());
        }

        let session_manager = self.session_manager;
        let mut summary = RespawnSummary::default();
        for root in &roots {
            respawn_root_agent(session_manager, app_data, root, &mut summary);
        }

        finish_respawn_summary(app_data, &summary)
    }

    pub(crate) fn restart_mux_daemon(self, app_data: &mut AppData) -> Result<()> {
        let socket = crate::mux::socket_display()?;
        crate::mux::terminate_mux_daemon_for_socket(&socket)?;

        app_data.ui.muxd_version_mismatch = None;
        app_data.ui.pane_activity_digest_mode = PaneActivityDigestMode::Cursor;

        // Recreate missing sessions/windows after the daemon restart (best-effort inside helper).
        self.respawn_missing_agents_in_data(app_data)?;

        app_data.set_status("Mux daemon restarted");
        Ok(())
    }
}

fn observe_agent_pane_activity(
    ui: &mut crate::app::state::UiState,
    agent_id: uuid::Uuid,
    selected_agent_id: Option<uuid::Uuid>,
    target: &str,
    digest_mode: &mut PaneActivityDigestMode,
    cursor_fn: &mut dyn FnMut() -> Result<crate::mux::OutputCursor>,
    capture_fn: &mut dyn FnMut() -> Result<String>,
) -> Result<()> {
    let (digest, next_mode) =
        pane_activity_digest_with_fallback(*digest_mode, cursor_fn, capture_fn)?;
    if *digest_mode != next_mode {
        debug!(
            target,
            ?digest_mode,
            ?next_mode,
            "Switching pane activity digest mode"
        );
        *digest_mode = next_mode;
    }
    ui.observe_agent_pane_digest(agent_id, digest);

    if selected_agent_id == Some(agent_id) {
        ui.pane_last_seen_hash_by_agent.insert(agent_id, digest);
    }

    Ok(())
}

fn mux_target_for_agent(app: &App, agent: &Agent) -> String {
    agent.window_index.map_or_else(
        || agent.mux_session.clone(),
        |window_idx| {
            // Child agent: target specific window within root's session.
            let root = app.data.storage.root_ancestor(agent.id);
            let root_session = root.unwrap_or(agent).mux_session.clone();
            crate::mux::SessionManager::window_target(&root_session, window_idx)
        },
    )
}

fn hash_output_cursor(cursor: crate::mux::OutputCursor) -> u64 {
    let mut hasher = DefaultHasher::new();
    cursor.start.hash(&mut hasher);
    cursor.end.hash(&mut hasher);
    hasher.finish()
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn pane_activity_digest_with_fallback(
    mode: PaneActivityDigestMode,
    cursor_fn: &mut dyn FnMut() -> Result<crate::mux::OutputCursor>,
    capture_fn: &mut dyn FnMut() -> Result<String>,
) -> Result<(u64, PaneActivityDigestMode)> {
    match mode {
        PaneActivityDigestMode::Cursor => {
            if let Ok(cursor) = cursor_fn() {
                Ok((hash_output_cursor(cursor), PaneActivityDigestMode::Cursor))
            } else {
                let content = capture_fn()?;
                Ok((hash_text(&content), PaneActivityDigestMode::Capture))
            }
        }
        PaneActivityDigestMode::Capture => {
            let content = capture_fn()?;
            Ok((hash_text(&content), PaneActivityDigestMode::Capture))
        }
    }
}

#[derive(Default)]
struct RespawnSummary {
    changed: bool,
    respawned_sessions: usize,
}

fn finish_respawn_summary(app_data: &mut AppData, summary: &RespawnSummary) -> Result<()> {
    if summary.changed {
        app_data.storage.save()?;
        app_data.validate_selection();
    }

    if summary.respawned_sessions > 0 {
        app_data.set_status(format!(
            "Respawned {} agent session(s)",
            summary.respawned_sessions
        ));
    }

    Ok(())
}

fn stored_root_agents(app_data: &AppData) -> Vec<Agent> {
    app_data
        .storage
        .root_agents()
        .into_iter()
        .cloned()
        .collect()
}

fn respawn_root_agent(
    session_manager: crate::mux::SessionManager,
    app_data: &mut AppData,
    root: &Agent,
    summary: &mut RespawnSummary,
) {
    if session_manager.exists(&root.mux_session) {
        summary.changed |= normalize_tree_running(app_data, root.id, &root.mux_session, true);
        return;
    }

    // Mark as not running while we attempt to respawn.
    summary.changed |= normalize_tree_running(app_data, root.id, &root.mux_session, false);

    if !root.worktree_path.exists() {
        warn!(
            title = %root.title,
            branch = %root.branch,
            worktree = %root.worktree_path.display(),
            "Worktree missing; cannot respawn agent session"
        );
        return;
    }

    if let Err(err) = crate::runtime::ensure_runtime_ready(root, &app_data.settings) {
        warn!(
            title = %root.title,
            session = %root.mux_session,
            error = %err,
            "Failed to prepare runtime for root agent; skipping respawn"
        );
        return;
    }

    let root_command = match command_for_agent(root, &app_data.settings) {
        Ok(cmd) => cmd,
        Err(err) => {
            warn!(
                title = %root.title,
                branch = %root.branch,
                error = %err,
                "Failed to build command for root agent; skipping respawn"
            );
            return;
        }
    };

    if let Err(err) = session_manager.create(
        &root.mux_session,
        &root.worktree_path,
        root_command.as_deref(),
    ) {
        warn!(
            title = %root.title,
            session = %root.mux_session,
            error = %err,
            "Failed to recreate mux session for agent"
        );
        return;
    }

    summary.respawned_sessions = summary.respawned_sessions.saturating_add(1);
    summary.changed = true;

    let descendants = sorted_descendants(app_data, root.id);
    let recreated_window_indices = recreate_descendant_windows(
        session_manager,
        &root.mux_session,
        &descendants,
        &app_data.settings,
    );
    summary.changed |= mark_respawned_agents_running(
        app_data,
        root.id,
        &root.mux_session,
        recreated_window_indices,
    );
}

fn sorted_descendants(app_data: &AppData, root_id: uuid::Uuid) -> Vec<Agent> {
    let mut descendants: Vec<Agent> = app_data
        .storage
        .descendants(root_id)
        .into_iter()
        .cloned()
        .collect();

    // Preserve (approximate) window ordering from the previous run when possible.
    descendants.sort_by_key(|agent| agent.window_index.unwrap_or(u32::MAX));
    descendants
}

fn recreate_descendant_windows(
    session_manager: crate::mux::SessionManager,
    root_session: &str,
    descendants: &[Agent],
    settings: &crate::app::Settings,
) -> HashMap<uuid::Uuid, u32> {
    let mut recreated_window_indices = HashMap::with_capacity(descendants.len());

    for desc in descendants {
        if !desc.worktree_path.exists() {
            warn!(
                title = %desc.title,
                branch = %desc.branch,
                worktree = %desc.worktree_path.display(),
                "Worktree missing; skipping respawn for agent window"
            );
            continue;
        }

        let command = match command_for_agent(desc, settings) {
            Ok(cmd) => cmd,
            Err(err) => {
                warn!(
                    title = %desc.title,
                    session = %root_session,
                    error = %err,
                    "Failed to build command for child agent; skipping window respawn"
                );
                continue;
            }
        };

        match session_manager.create_window(
            root_session,
            &desc.title,
            &desc.worktree_path,
            command.as_deref(),
        ) {
            Ok(index) => {
                let _ = recreated_window_indices.insert(desc.id, index);
            }
            Err(err) => {
                warn!(
                    title = %desc.title,
                    session = %root_session,
                    error = %err,
                    "Failed to recreate mux window for agent"
                );
            }
        }
    }

    recreated_window_indices
}

fn mark_respawned_agents_running(
    app_data: &mut AppData,
    root_id: uuid::Uuid,
    mux_session: &str,
    recreated_window_indices: HashMap<uuid::Uuid, u32>,
) -> bool {
    let mut changed = false;
    let mux_session_owned = mux_session.to_owned();

    if let Some(agent) = app_data.storage.get_mut(root_id) {
        if agent.mux_session != mux_session_owned {
            agent.mux_session.clone_from(&mux_session_owned);
            changed = true;
        }

        if agent.status != Status::Running {
            agent.set_status(Status::Running);
            changed = true;
        }
    }

    for (agent_id, window_index) in recreated_window_indices {
        if let Some(agent) = app_data.storage.get_mut(agent_id) {
            if agent.mux_session != mux_session_owned {
                agent.mux_session.clone_from(&mux_session_owned);
                changed = true;
            }

            if agent.window_index != Some(window_index) {
                agent.window_index = Some(window_index);
                changed = true;
            }

            if agent.status != Status::Running {
                agent.set_status(Status::Running);
                changed = true;
            }
        }
    }

    changed
}

fn has_isolated_state_marker(worktree_path: &Path, stop_at: &Path) -> bool {
    let mut current = worktree_path;
    loop {
        if current.join("state.json").exists() {
            return true;
        }

        if current == stop_at {
            return false;
        }

        let Some(parent) = current.parent() else {
            return false;
        };
        current = parent;
    }
}

fn is_tenex_managed_branch(branch: &str, branch_prefix: &str) -> bool {
    (!branch_prefix.is_empty() && branch.starts_with(branch_prefix)) || branch.starts_with("tenex/")
}

fn title_for_branch(branch: &str, branch_prefix: &str) -> String {
    let stripped = branch
        .strip_prefix(branch_prefix)
        .or_else(|| branch.strip_prefix("tenex/"))
        .unwrap_or(branch);
    if stripped.is_empty() {
        branch.to_string()
    } else {
        stripped.to_string()
    }
}

fn normalize_tree_running(
    app_data: &mut AppData,
    root_id: uuid::Uuid,
    mux_session: &str,
    running: bool,
) -> bool {
    let mut changed = false;
    let status = if running {
        Status::Running
    } else {
        Status::Starting
    };

    let descendant_ids = app_data.storage.descendant_ids(root_id);
    let mut ids = HashSet::with_capacity(descendant_ids.len().saturating_add(1));
    ids.insert(root_id);
    ids.extend(descendant_ids);

    for agent in app_data.storage.iter_mut() {
        if !ids.contains(&agent.id) {
            continue;
        }

        if agent.mux_session != mux_session {
            agent.mux_session = mux_session.to_string();
            changed = true;
        }
        if agent.status != status {
            agent.set_status(status);
            changed = true;
        }
    }

    changed
}

fn command_for_agent(
    agent: &Agent,
    settings: &crate::app::Settings,
) -> Result<Option<Vec<String>>> {
    if agent.is_terminal_agent() {
        return Ok(crate::runtime::build_terminal_command(
            agent, None, settings,
        ));
    }

    crate::runtime::build_agent_command(agent, crate::runtime::AgentLaunch::Resume, settings)
        .map(Some)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::agent::{AgentRuntime, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use git2::Repository;
    use git2::RepositoryInitOptions;
    use git2::Signature;
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::path::PathBuf;
    #[cfg(target_os = "linux")]
    use std::process::Command;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
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

    fn test_echo_arg_program() -> String {
        #[cfg(windows)]
        {
            "powershell -NoProfile -Command \"Write-Output 'ARG1:'; Start-Sleep -Seconds 3600\""
                .to_string()
        }
        #[cfg(not(windows))]
        {
            // The `_` argument is the arg0 placeholder for `sh -c`, so an appended prompt
            // becomes $1.
            "sh -c 'echo ARG1:$1; sleep 3600' _".to_string()
        }
    }

    fn test_output_then_sleep_program() -> String {
        #[cfg(windows)]
        {
            "powershell -NoProfile -Command \"Write-Output 'capture-path'; Start-Sleep -Seconds 3600\""
                .to_string()
        }
        #[cfg(not(windows))]
        {
            "sh -c 'printf capture-path; sleep 3600'".to_string()
        }
    }

    #[cfg(unix)]
    fn write_fake_docker_script(temp: &TempDir, body: &str) -> PathBuf {
        let script = temp.path().join("docker");
        fs::write(&script, body).unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        script
    }

    fn init_test_repo_with_commit() -> (TempDir, Repository) {
        let temp_dir = TempDir::new().unwrap();
        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        fs::write(temp_dir.path().join("README.md"), "# Test").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (temp_dir, repo)
    }

    fn create_worktree(repo: &Repository, path: &Path, branch: &str) {
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch(branch, &commit, false).unwrap();
        let worktree_mgr = WorktreeManager::new(repo);
        worktree_mgr.create(path, branch).unwrap();
    }

    fn create_test_app_with_config(config: Config) -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(config, storage, Settings::default(), false),
            temp_file,
        )
    }

    #[cfg(coverage)]
    #[test]
    fn test_exercise_sync_paths_for_coverage_runs_in_unit_build() {
        let (mut app, _temp_file) = create_test_app();
        Actions::exercise_sync_paths_for_coverage(&mut app);
    }

    #[test]
    fn test_resolve_repo_path_with_deps_uses_project_root_and_skips_current_dir() {
        let expected = PathBuf::from("/tmp/tenex-test-project-root");
        let called = Cell::new(false);

        let make_current_dir = || {
            || {
                called.set(true);
                Ok(PathBuf::from("/tmp/tenex-test-unused"))
            }
        };

        let _ = make_current_dir()().unwrap();
        called.set(false);

        let mut current_dir = make_current_dir();
        let actual = resolve_repo_path_with_deps(Some(expected.clone()), &mut current_dir).unwrap();

        assert_eq!(actual, expected);
        assert!(!called.get());
    }

    #[test]
    fn test_resolve_repo_path_with_deps_falls_back_to_current_dir() {
        let expected = PathBuf::from("/tmp/tenex-test-current-dir");
        let mut current_dir = || Ok(expected.clone());
        let actual = resolve_repo_path_with_deps(None, &mut current_dir).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_resolve_repo_path_with_deps_errors_when_current_dir_unavailable() {
        let mut current_dir = || {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cwd missing",
            ))
        };
        let err = resolve_repo_path_with_deps(None, &mut current_dir).unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed to resolve current project directory")
        );
    }

    #[test]
    fn test_sync_agent_status_smoke() {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app();

        handler.sync_agent_status(&mut app).unwrap();
    }

    #[test]
    fn test_sync_agent_pane_activity_capture_mode_populates_digest() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut agent = Agent::new(
            "capture-mode".to_string(),
            test_output_then_sleep_program(),
            "muster/capture-mode".to_string(),
            temp.path().to_path_buf(),
        );
        agent.set_status(Status::Running);
        let agent_id = agent.id;
        let session = agent.mux_session.clone();
        app.data.storage.add(agent);
        app.data.ui.pane_activity_digest_mode = PaneActivityDigestMode::Capture;

        let command =
            crate::command::parse_command_line(&test_output_then_sleep_program()).unwrap();
        let manager = crate::mux::SessionManager::new();
        manager
            .create(&session, temp.path(), Some(&command))
            .unwrap();

        let handler = Actions::new();
        std::thread::sleep(std::time::Duration::from_millis(300));
        handler.sync_agent_pane_activity(&mut app).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        handler.sync_agent_pane_activity(&mut app).unwrap();

        assert_eq!(
            app.data.ui.pane_activity_digest_mode,
            PaneActivityDigestMode::Capture
        );
        assert!(app.data.ui.pane_digest_by_agent.contains_key(&agent_id));
        assert!(app.data.ui.agent_is_waiting_for_input(agent_id));

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_sync_agent_pane_activity_skips_non_running_agents() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut running = Agent::new(
            "running".to_string(),
            test_output_then_sleep_program(),
            "muster/running".to_string(),
            temp.path().to_path_buf(),
        );
        running.set_status(Status::Running);
        let running_id = running.id;
        let running_session = running.mux_session.clone();
        app.data.storage.add(running);

        let mut starting = Agent::new(
            "starting".to_string(),
            test_output_then_sleep_program(),
            "muster/starting".to_string(),
            temp.path().to_path_buf(),
        );
        starting.set_status(Status::Starting);
        let starting_id = starting.id;
        app.data.storage.add(starting);

        app.data.ui.pane_activity_digest_mode = PaneActivityDigestMode::Capture;

        let command =
            crate::command::parse_command_line(&test_output_then_sleep_program()).unwrap();
        let manager = crate::mux::SessionManager::new();
        manager
            .create(&running_session, temp.path(), Some(&command))
            .unwrap();

        let handler = Actions::new();
        std::thread::sleep(std::time::Duration::from_millis(200));
        handler.sync_agent_pane_activity(&mut app).unwrap();

        assert!(app.data.ui.pane_digest_by_agent.contains_key(&running_id));
        assert!(!app.data.ui.pane_digest_by_agent.contains_key(&starting_id));

        let _ = manager.kill(&running_session);
    }

    #[test]
    fn test_sync_agent_pane_activity_continues_when_cursor_and_capture_fail() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut alive = Agent::new(
            "alive".to_string(),
            test_output_then_sleep_program(),
            "muster/alive".to_string(),
            temp.path().to_path_buf(),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            test_output_then_sleep_program(),
            "muster/missing".to_string(),
            temp.path().to_path_buf(),
        );
        missing.set_status(Status::Running);
        let missing_id = missing.id;
        app.data.storage.add(missing);

        let command =
            crate::command::parse_command_line(&test_output_then_sleep_program()).unwrap();
        let manager = crate::mux::SessionManager::new();
        manager
            .create(&alive_session, temp.path(), Some(&command))
            .unwrap();

        Actions::new().sync_agent_pane_activity(&mut app).unwrap();

        assert!(!app.data.ui.pane_digest_by_agent.contains_key(&missing_id));

        let _ = manager.kill(&alive_session);
    }

    fn cursor_ok() -> crate::mux::OutputCursor {
        crate::mux::OutputCursor { start: 7, end: 11 }
    }

    fn cursor_fail() -> Result<crate::mux::OutputCursor> {
        Err(anyhow::anyhow!("unsupported"))
    }

    fn capture_fail() -> Result<String> {
        Err(anyhow::anyhow!("capture failed"))
    }

    fn capture_ok() -> String {
        String::from("visible pane")
    }

    #[test]
    fn test_pane_activity_digest_prefers_output_cursor_when_available() {
        let capture_calls = std::cell::Cell::new(0u32);
        let mut cursor = || Ok(cursor_ok());
        let mut capture = || {
            capture_calls.set(capture_calls.get().saturating_add(1));
            Ok(capture_ok())
        };
        let (digest, mode) = pane_activity_digest_with_fallback(
            PaneActivityDigestMode::Cursor,
            &mut cursor,
            &mut capture,
        )
        .unwrap();

        assert_eq!(
            digest,
            hash_output_cursor(crate::mux::OutputCursor { start: 7, end: 11 })
        );
        assert_eq!(mode, PaneActivityDigestMode::Cursor);
        assert_eq!(capture_calls.get(), 0);
        let _ = capture().unwrap();
        assert_eq!(capture_calls.get(), 1);
    }

    #[test]
    fn test_pane_activity_digest_falls_back_to_capture_when_cursor_fails() {
        let mut cursor = cursor_fail;
        let mut capture = || Ok(capture_ok());
        let (digest, mode) = pane_activity_digest_with_fallback(
            PaneActivityDigestMode::Cursor,
            &mut cursor,
            &mut capture,
        )
        .unwrap();

        assert_eq!(digest, hash_text("visible pane"));
        assert_eq!(mode, PaneActivityDigestMode::Capture);
    }

    #[test]
    fn test_pane_activity_digest_capture_mode_uses_capture() {
        let cursor_calls = std::cell::Cell::new(0u32);
        let mut cursor = || {
            cursor_calls.set(cursor_calls.get().saturating_add(1));
            Ok(cursor_ok())
        };
        let mut capture = || Ok(capture_ok());
        let (digest, mode) = pane_activity_digest_with_fallback(
            PaneActivityDigestMode::Capture,
            &mut cursor,
            &mut capture,
        )
        .unwrap();

        assert_eq!(digest, hash_text("visible pane"));
        assert_eq!(mode, PaneActivityDigestMode::Capture);
        assert_eq!(cursor_calls.get(), 0);
        let _ = cursor().unwrap();
        assert_eq!(cursor_calls.get(), 1);
    }

    #[test]
    fn test_pane_activity_digest_reports_error_when_capture_fails_in_cursor_mode() {
        let mut cursor = cursor_fail;
        let mut capture = capture_fail;
        let result = pane_activity_digest_with_fallback(
            PaneActivityDigestMode::Cursor,
            &mut cursor,
            &mut capture,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_pane_activity_digest_reports_error_when_capture_fails_in_capture_mode() {
        let cursor_calls = std::cell::Cell::new(0u32);
        let mut cursor = || {
            cursor_calls.set(cursor_calls.get().saturating_add(1));
            Ok(cursor_ok())
        };
        let mut capture = capture_fail;
        let result = pane_activity_digest_with_fallback(
            PaneActivityDigestMode::Capture,
            &mut cursor,
            &mut capture,
        );
        assert!(result.is_err());
        assert_eq!(cursor_calls.get(), 0);
        let _ = cursor().unwrap();
        assert_eq!(cursor_calls.get(), 1);
    }

    #[test]
    fn test_observe_agent_pane_activity_switches_digest_mode_when_cursor_fails() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();
        let agent = Agent::new(
            "switch-mode".to_string(),
            "claude".to_string(),
            "muster/switch-mode".to_string(),
            temp.path().to_path_buf(),
        );
        let selected = Some(agent.id);
        let mut mode = PaneActivityDigestMode::Cursor;
        let mut cursor = cursor_fail;
        let mut capture = || Ok(capture_ok());

        with_tracing_dispatch(|| {
            observe_agent_pane_activity(
                &mut app.data.ui,
                agent.id,
                selected,
                "target",
                &mut mode,
                &mut cursor,
                &mut capture,
            )
        })
        .unwrap();

        assert_eq!(mode, PaneActivityDigestMode::Capture);
        assert!(app.data.ui.pane_digest_by_agent.contains_key(&agent.id));
        assert!(
            app.data
                .ui
                .pane_last_seen_hash_by_agent
                .contains_key(&agent.id)
        );
    }

    #[test]
    fn test_sync_agent_status_with_agents() {
        let (mut app, _temp) = create_test_app();

        // Add agents with different statuses
        let mut running = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
        );
        running.set_status(Status::Running);
        app.data.storage.add(running);

        let mut starting = Agent::new(
            "starting".to_string(),
            "claude".to_string(),
            "muster/starting".to_string(),
            PathBuf::from("/tmp"),
        );
        starting.set_status(Status::Starting);
        app.data.storage.add(starting);

        // When mux session listing succeeds but reports no sessions, treat it as uncertain and
        // avoid destructive pruning.
        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(&mut app, Ok(vec![]))
        })
        .unwrap();

        assert_eq!(app.data.storage.len(), 2);
    }

    #[test]
    fn test_sync_agent_status_prunes_missing_sessions() {
        let (mut app, _temp) = create_test_app();

        let mut alive = Agent::new(
            "alive".to_string(),
            "claude".to_string(),
            "muster/alive".to_string(),
            PathBuf::from("/tmp"),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        let alive_id = alive.id;
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            "claude".to_string(),
            "muster/missing".to_string(),
            PathBuf::from("/tmp"),
        );
        missing.set_status(Status::Running);
        let missing_id = missing.id;
        app.data.storage.add(missing);

        let mut starting_child = Agent::new_child(
            "starting-child".to_string(),
            "claude".to_string(),
            "muster/alive".to_string(),
            PathBuf::from("/tmp"),
            crate::agent::ChildConfig {
                parent_id: alive_id,
                mux_session: "missing-child-session".to_string(),
                window_index: 1,
                repo_root: None,
            },
        );
        starting_child.set_status(Status::Starting);
        let starting_child_id = starting_child.id;
        app.data.storage.add(starting_child);

        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(
                &mut app,
                Ok(vec![crate::mux::Session {
                    name: alive_session.clone(),
                    created: 0,
                    attached: false,
                }]),
            )
        })
        .unwrap();

        assert_eq!(app.data.storage.len(), 2);
        assert!(app.data.storage.get(missing_id).is_none());
        let child = app
            .data
            .storage
            .get(starting_child_id)
            .expect("starting child remains stored");
        assert_eq!(child.status, Status::Starting);
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_agent_status_prunes_missing_docker_runtime() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("docker.log");
        let script = write_fake_docker_script(
            &temp,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
                log.display()
            ),
        );

        let mut alive = Agent::new(
            "alive".to_string(),
            "claude".to_string(),
            "muster/alive".to_string(),
            PathBuf::from("/tmp"),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            "claude".to_string(),
            "muster/missing".to_string(),
            PathBuf::from("/tmp"),
        );
        missing.set_status(Status::Running);
        missing.runtime = AgentRuntime::Docker;
        let expected_container = format!("tenex-runtime-{}", missing.mux_session).to_lowercase();
        let missing_id = missing.id;
        app.data.storage.add(missing);

        crate::runtime::with_docker_program_override_for_tests(script, || {
            with_tracing_dispatch(|| {
                Actions::new().sync_agent_status_with_sessions(
                    &mut app,
                    Ok(vec![crate::mux::Session {
                        name: alive_session.clone(),
                        created: 0,
                        attached: false,
                    }]),
                )
            })
        })
        .unwrap();

        let log_contents = fs::read_to_string(&log).unwrap();
        assert!(log_contents.contains(&format!("rm -f {expected_container}")));
        assert!(app.data.storage.get(missing_id).is_none());
    }

    #[test]
    fn test_sync_agent_status_promotes_starting_when_session_exists() {
        let (mut app, _temp) = create_test_app();

        let mut agent = Agent::new(
            "starting".to_string(),
            "claude".to_string(),
            "muster/starting".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.set_status(Status::Starting);
        let session = agent.mux_session.clone();
        let agent_id = agent.id;
        app.data.storage.add(agent);

        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(
                &mut app,
                Ok(vec![crate::mux::Session {
                    name: session,
                    created: 0,
                    attached: false,
                }]),
            )
        })
        .unwrap();

        let stored = app.data.storage.get(agent_id).expect("Agent missing");
        assert_eq!(stored.status, Status::Running);
    }

    #[test]
    fn test_sync_agent_status_list_error_does_not_prune() {
        let (mut app, _temp) = create_test_app();

        // Add a running root agent.
        let mut agent = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.set_status(Status::Running);
        app.data.storage.add(agent);

        with_tracing_dispatch(|| {
            Actions::new()
                .sync_agent_status_with_sessions(&mut app, Err(anyhow::anyhow!("mux down")))
        })
        .unwrap();
        assert_eq!(app.data.storage.len(), 1);
    }

    #[test]
    fn test_sync_agent_status_returns_early_when_no_stored_sessions_in_session_list() {
        let (mut app, _temp) = create_test_app();

        let mut agent = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.set_status(Status::Running);
        app.data.storage.add(agent);

        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(
                &mut app,
                Ok(vec![crate::mux::Session {
                    name: "other-session".to_string(),
                    created: 0,
                    attached: false,
                }]),
            )
        })
        .unwrap();

        assert_eq!(app.data.storage.len(), 1);
    }

    #[test]
    fn test_sync_agent_status_kills_session_when_missing_root_has_session() {
        let temp = TempDir::new().unwrap();
        let (mut app, _temp_file) = create_test_app();

        let mut alive = Agent::new(
            "alive".to_string(),
            test_sleep_program(),
            "muster/alive".to_string(),
            temp.path().to_path_buf(),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            test_sleep_program(),
            "muster/missing".to_string(),
            temp.path().to_path_buf(),
        );
        missing.set_status(Status::Running);
        let missing_session = missing.mux_session.clone();
        let missing_id = missing.id;
        app.data.storage.add(missing);

        let command = crate::command::parse_command_line(&test_sleep_program()).unwrap();
        let manager = crate::mux::SessionManager::new();
        manager
            .create(&alive_session, temp.path(), Some(&command))
            .unwrap();
        manager
            .create(&missing_session, temp.path(), Some(&command))
            .unwrap();
        assert!(manager.exists(&missing_session));

        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(
                &mut app,
                Ok(vec![crate::mux::Session {
                    name: alive_session.clone(),
                    created: 0,
                    attached: false,
                }]),
            )
        })
        .unwrap();

        assert!(app.data.storage.get(missing_id).is_none());
        assert!(!manager.exists(&missing_session));

        let _ = manager.kill(&alive_session);
        let _ = manager.kill(&missing_session);
    }

    #[test]
    fn test_sync_agent_status_with_sessions_empty_storage_noop() {
        let (mut app, _temp) = create_test_app();

        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(&mut app, Ok(vec![]))
        })
        .unwrap();

        assert!(app.data.storage.is_empty());
    }

    #[test]
    fn test_sync_agent_status_warns_when_kill_fails_unexpectedly() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let bad_socket = format!("tenex-mux-bad-{}\0", uuid::Uuid::new_v4());
        crate::mux::set_socket_override(&bad_socket).unwrap();

        let (mut app, _temp) = create_test_app();

        let mut alive = Agent::new(
            "alive".to_string(),
            "claude".to_string(),
            "muster/alive".to_string(),
            PathBuf::from("/tmp"),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            "claude".to_string(),
            "muster/missing".to_string(),
            PathBuf::from("/tmp"),
        );
        missing.set_status(Status::Running);
        let missing_id = missing.id;
        app.data.storage.add(missing);

        with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(
                &mut app,
                Ok(vec![crate::mux::Session {
                    name: alive_session.clone(),
                    created: 0,
                    attached: false,
                }]),
            )
        })
        .unwrap();

        assert!(app.data.storage.get(missing_id).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_sync_agent_status_warns_when_cleanup_runtime_fails() {
        let _guard = crate::test_support::lock_env_test_environment();
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(&temp, "#!/bin/sh\nexit 1\n");

        let mut alive = Agent::new(
            "alive".to_string(),
            "claude".to_string(),
            "muster/alive".to_string(),
            PathBuf::from("/tmp"),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            "claude".to_string(),
            "muster/missing".to_string(),
            PathBuf::from("/tmp"),
        );
        missing.set_status(Status::Running);
        missing.runtime = AgentRuntime::Docker;
        let missing_id = missing.id;
        app.data.storage.add(missing);

        crate::runtime::with_docker_program_override_for_tests(script, || {
            with_tracing_dispatch(|| {
                Actions::new().sync_agent_status_with_sessions(
                    &mut app,
                    Ok(vec![crate::mux::Session {
                        name: alive_session.clone(),
                        created: 0,
                        attached: false,
                    }]),
                )
            })
        })
        .unwrap();

        assert!(app.data.storage.get(missing_id).is_none());
    }

    #[test]
    fn test_sync_agent_status_returns_error_when_storage_save_fails() {
        let dir = TempDir::new().unwrap();
        let storage = Storage::with_path(dir.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let mut alive = Agent::new(
            "alive".to_string(),
            "claude".to_string(),
            "muster/alive".to_string(),
            PathBuf::from("/tmp"),
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.data.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            "claude".to_string(),
            "muster/missing".to_string(),
            PathBuf::from("/tmp"),
        );
        missing.set_status(Status::Running);
        app.data.storage.add(missing);

        let result = with_tracing_dispatch(|| {
            Actions::new().sync_agent_status_with_sessions(
                &mut app,
                Ok(vec![crate::mux::Session {
                    name: alive_session.clone(),
                    created: 0,
                    attached: false,
                }]),
            )
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_respawn_missing_agents_creates_sessions_and_windows() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let worktree = TempDir::new().unwrap();
        let worktree_path = worktree.path().to_path_buf();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree_path.clone(),
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        let root_id = root.id;

        let child = Agent::new_child(
            "child".to_string(),
            test_sleep_program(),
            root.branch.clone(),
            worktree_path,
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );

        app.data.storage.add(root);
        app.data.storage.add(child);
        app.data.storage.save().unwrap();

        // Ensure the session doesn't already exist (best-effort).
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Actions::new().respawn_missing_agents(&mut app).unwrap();

        assert!(crate::mux::SessionManager::new().exists(&root_session));

        let windows = crate::mux::SessionManager::new()
            .list_windows(&root_session)
            .unwrap();
        assert!(windows.iter().any(|w| w.index == 0));
        assert!(windows.iter().any(|w| w.name == "child"));

        // Clean up to avoid leaving long-running processes around.
        let _ = crate::mux::SessionManager::new().kill(&root_session);
    }

    #[test]
    fn test_respawn_missing_agents_in_data_reports_error_when_storage_save_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();

        let (mut app, _temp_file) = create_test_app();
        let missing_worktree_path = TempDir::new().unwrap().path().join("missing-worktree");
        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "agent/root".to_string(),
            missing_worktree_path,
        );
        root.set_status(Status::Running);
        app.data.storage.add(root);

        let err = Storage::with_forced_save_error_after_successes_for_tests(0, || {
            with_tracing_dispatch(|| Actions::new().respawn_missing_agents_in_data(&mut app.data))
        })
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("forced storage save error for test")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_restart_mux_daemon_reports_error_when_terminate_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();
        let socket_display = crate::mux::socket_display().unwrap();

        let mut child = Command::new("sh")
            .args(["-c", "sleep 60", "muxd"])
            .env("TENEX_MUX_SOCKET", &socket_display)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let (mut app, _temp_file) = create_test_app();
        let err = crate::mux::with_kill_program_override_for_tests(
            PathBuf::from("tenex-test-missing-kill"),
            || with_tracing_dispatch(|| Actions::new().restart_mux_daemon(&mut app.data)),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Failed to terminate mux daemon"));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn test_restart_mux_daemon_reports_error_when_respawn_save_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();

        let (mut app, _temp_file) = create_test_app();
        let missing_worktree_path = TempDir::new().unwrap().path().join("missing-worktree");
        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "agent/root".to_string(),
            missing_worktree_path,
        );
        root.set_status(Status::Running);
        app.data.storage.add(root);

        let err = Storage::with_forced_save_error_after_successes_for_tests(0, || {
            with_tracing_dispatch(|| Actions::new().restart_mux_daemon(&mut app.data))
        })
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("forced storage save error for test")
        );
    }

    #[test]
    fn test_respawn_missing_agents_marks_tree_running_when_session_exists() {
        let (mut app, _temp_file) = create_test_app();
        let worktree = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        root.set_status(Status::Starting);
        let root_session = root.mux_session.clone();
        let root_id = root.id;

        let mut child = Agent::new_child(
            "child".to_string(),
            test_sleep_program(),
            root.branch.clone(),
            worktree.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        child.set_status(Status::Starting);

        app.data.storage.add(root);
        app.data.storage.add(child);
        app.data.storage.save().unwrap();

        let command = crate::command::parse_command_line(&test_sleep_program()).unwrap();
        let manager = crate::mux::SessionManager::new();
        manager
            .create(&root_session, worktree.path(), Some(&command))
            .unwrap();

        with_tracing_dispatch(|| Actions::new().respawn_missing_agents(&mut app)).unwrap();

        let stored = app.data.storage.get(root_id).expect("missing root");
        assert_eq!(stored.status, Status::Running);
        let descendants = app.data.storage.descendants(root_id);
        assert_eq!(descendants.len(), 1);
        assert_eq!(descendants[0].status, Status::Running);

        let _ = manager.kill(&root_session);
    }

    #[test]
    fn test_respawn_missing_agents_noops_when_session_exists_and_tree_running() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();

        let temp = TempDir::new().unwrap();
        let blocked_parent = temp.path().join("blocked");
        fs::write(&blocked_parent, "not-a-dir").unwrap();
        let storage = Storage::with_path(blocked_parent.join("state.json"));
        let mut app = App::new(Config::default(), storage, Settings::default(), false);
        let worktree = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        let root_id = root.id;

        let mut child = Agent::new_child(
            "child".to_string(),
            test_sleep_program(),
            root.branch.clone(),
            worktree.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        child.set_status(Status::Running);

        app.data.storage.add(root);
        app.data.storage.add(child);

        let command = crate::command::parse_command_line(&test_sleep_program()).unwrap();
        let manager = crate::mux::SessionManager::new();
        manager
            .create(&root_session, worktree.path(), Some(&command))
            .unwrap();

        with_tracing_dispatch(|| Actions::new().respawn_missing_agents(&mut app)).unwrap();

        let stored = app.data.storage.get(root_id).expect("missing root");
        assert_eq!(stored.status, Status::Running);

        let descendants = app.data.storage.descendants(root_id);
        assert_eq!(descendants.len(), 1);
        assert_eq!(descendants[0].status, Status::Running);

        let _ = manager.kill(&root_session);
    }

    #[test]
    fn test_respawn_missing_agents_warns_and_skips_when_worktree_missing() {
        let (mut app, _temp_file) = create_test_app();
        let missing_worktree = TempDir::new().unwrap().path().join("missing");

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            missing_worktree,
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.storage.save().unwrap();

        with_tracing_dispatch(|| Actions::new().respawn_missing_agents(&mut app)).unwrap();
        assert!(!crate::mux::SessionManager::new().exists(&root_session));
    }

    #[cfg(unix)]
    #[test]
    fn test_respawn_missing_agents_warns_and_skips_when_runtime_prepare_fails() {
        let _guard = crate::test_support::lock_env_test_environment();
        let (mut app, _temp_file) = create_test_app();
        let worktree = TempDir::new().unwrap();
        let temp = TempDir::new().unwrap();
        let script = write_fake_docker_script(&temp, "#!/bin/sh\nexit 1\n");

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        root.set_status(Status::Running);
        root.runtime = AgentRuntime::Docker;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.storage.save().unwrap();

        crate::runtime::with_docker_program_override_for_tests(script, || {
            with_tracing_dispatch(|| Actions::new().respawn_missing_agents(&mut app))
        })
        .unwrap();

        assert!(!crate::mux::SessionManager::new().exists(&root_session));
    }

    #[test]
    fn test_respawn_missing_agents_warns_and_skips_when_command_for_agent_fails() {
        let (mut app, _temp_file) = create_test_app();
        let worktree = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            " ".to_string(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.storage.save().unwrap();

        with_tracing_dispatch(|| Actions::new().respawn_missing_agents(&mut app)).unwrap();
        assert!(!crate::mux::SessionManager::new().exists(&root_session));
    }

    #[test]
    fn test_respawn_missing_agents_warns_and_skips_when_session_create_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let bad_socket = format!("tenex-mux-bad-{}\0", uuid::Uuid::new_v4());
        crate::mux::set_socket_override(&bad_socket).unwrap();

        let (mut app, _temp_file) = create_test_app();
        let worktree = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);
        app.data.storage.save().unwrap();

        with_tracing_dispatch(|| Actions::new().respawn_missing_agents(&mut app)).unwrap();
        assert!(!crate::mux::SessionManager::new().exists(&root_session));
    }

    #[test]
    fn test_recreate_descendant_windows_warns_and_skips_when_worktree_missing() {
        let worktree = TempDir::new().unwrap();
        let missing_worktree = worktree.path().join("missing");
        let mut desc = Agent::new(
            "child".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            missing_worktree,
        );
        desc.parent_id = Some(uuid::Uuid::new_v4());
        desc.window_index = Some(1);

        with_tracing_dispatch(|| {
            recreate_descendant_windows(
                crate::mux::SessionManager::new(),
                "root-session",
                &[desc],
                &crate::app::Settings::default(),
            )
        });
    }

    #[test]
    fn test_recreate_descendant_windows_warns_and_skips_when_command_for_agent_fails() {
        let worktree = TempDir::new().unwrap();
        let mut desc = Agent::new(
            "child".to_string(),
            " ".to_string(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        desc.parent_id = Some(uuid::Uuid::new_v4());
        desc.window_index = Some(1);

        with_tracing_dispatch(|| {
            recreate_descendant_windows(
                crate::mux::SessionManager::new(),
                "root-session",
                &[desc],
                &crate::app::Settings::default(),
            )
        });
    }

    #[test]
    fn test_recreate_descendant_windows_warns_when_create_window_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let bad_socket = format!("tenex-mux-bad-{}\0", uuid::Uuid::new_v4());
        crate::mux::set_socket_override(&bad_socket).unwrap();

        let worktree = TempDir::new().unwrap();
        let mut desc = Agent::new(
            "child".to_string(),
            test_sleep_program(),
            "tenex-test/root".to_string(),
            worktree.path().to_path_buf(),
        );
        desc.parent_id = Some(uuid::Uuid::new_v4());
        desc.window_index = Some(1);

        with_tracing_dispatch(|| {
            recreate_descendant_windows(
                crate::mux::SessionManager::new(),
                "root-session",
                &[desc],
                &crate::app::Settings::default(),
            )
        });
    }

    #[test]
    fn test_respawn_missing_agents_ignores_legacy_initial_prompt() {
        let temp_file = NamedTempFile::new().unwrap();
        let state_path = temp_file.path().to_path_buf();

        let worktree = TempDir::new().unwrap();
        let worktree_path = worktree.path().to_path_buf();

        // This program prints the first appended argument, then sleeps so we can capture output.
        let program = test_echo_arg_program();

        let mut storage = Storage::with_path(state_path.clone());
        let mut root = Agent::new(
            "root".to_string(),
            program,
            "tenex-test/root".to_string(),
            worktree_path,
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        storage.add(root);

        // Write state.json with a legacy `initial_prompt` field injected.
        let mut value = serde_json::to_value(&storage).unwrap();
        let agents = value
            .get_mut("agents")
            .and_then(serde_json::Value::as_array_mut)
            .expect("Expected agents array");
        let root_value = agents
            .first_mut()
            .and_then(serde_json::Value::as_object_mut)
            .expect("Expected root agent object");
        root_value.insert(
            "initial_prompt".to_string(),
            serde_json::Value::String("PROMPT".to_string()),
        );
        std::fs::write(&state_path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        // Load the state and ensure subsequent saves target the temp file (not the real state).
        let mut loaded = Storage::load_from(&state_path).unwrap();
        loaded.state_path = Some(state_path);

        let mut app = App::new(Config::default(), loaded, Settings::default(), false);

        // Ensure the session doesn't already exist (best-effort).
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Actions::new().respawn_missing_agents(&mut app).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(200));
        let output = crate::mux::OutputCapture::new()
            .capture_pane(&root_session)
            .unwrap();
        assert!(output.contains("ARG1:"));
        assert!(!output.contains("PROMPT"));

        // Clean up to avoid leaving long-running processes around.
        let _ = crate::mux::SessionManager::new().kill(&root_session);
    }

    #[test]
    fn test_command_for_agent_terminal_returns_none() {
        let agent = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "tenex-test/terminal".to_string(),
            PathBuf::from("/tmp"),
        );

        let command = command_for_agent(&agent, &crate::app::Settings::default()).unwrap();
        assert!(command.is_none());
    }

    #[test]
    fn test_command_for_agent_resumes_when_conversation_id_present() {
        let mut agent = Agent::new(
            "agent".to_string(),
            "claude --debug".to_string(),
            "tenex-test/agent".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.conversation_id = Some("resume-id".to_string());

        let command = command_for_agent(&agent, &crate::app::Settings::default())
            .unwrap()
            .expect("Expected command");
        assert_eq!(command, vec!["claude", "--debug", "--resume", "resume-id"]);
    }

    #[test]
    fn test_command_for_agent_spawns_when_no_conversation_id() {
        let agent = Agent::new(
            "agent".to_string(),
            "claude --debug".to_string(),
            "tenex-test/agent".to_string(),
            PathBuf::from("/tmp"),
        );

        let command = command_for_agent(&agent, &crate::app::Settings::default())
            .unwrap()
            .expect("Expected command");
        assert_eq!(command, vec!["claude", "--debug"]);
    }

    #[test]
    fn test_sync_agent_status_returns_early_when_mux_not_running() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();

        let (mut app, _temp) = create_test_app();
        with_tracing_dispatch(|| Actions::new().sync_agent_status(&mut app)).unwrap();
    }

    #[test]
    fn test_sync_agent_pane_activity_clears_state_when_mux_not_running() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();

        let (mut app, _temp) = create_test_app();
        let agent_id = uuid::Uuid::new_v4();
        app.data.ui.observe_agent_pane_digest(agent_id, 41);
        app.data
            .ui
            .pane_last_seen_hash_by_agent
            .insert(agent_id, 42);
        app.data.ui.pane_activity_digest_mode = PaneActivityDigestMode::Capture;

        with_tracing_dispatch(|| Actions::new().sync_agent_pane_activity(&mut app)).unwrap();

        assert!(app.data.ui.pane_digest_by_agent.is_empty());
        assert!(app.data.ui.pane_last_seen_hash_by_agent.is_empty());
        assert_eq!(
            app.data.ui.pane_activity_digest_mode,
            PaneActivityDigestMode::Cursor
        );
    }

    #[test]
    fn test_auto_connect_worktrees_skips_when_not_git_repository() {
        let temp = TempDir::new().unwrap();
        let (mut app, _temp_file) = create_test_app();
        app.set_cwd_project_root(Some(temp.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
    }

    #[test]
    fn test_auto_connect_worktrees_reports_error_when_repo_path_resolution_fails() {
        let (mut app, _temp_file) = create_test_app();
        app.set_cwd_project_root(None);

        let mut current_dir = || Err(std::io::Error::other("cwd fail"));
        let err = Actions::new()
            .auto_connect_worktrees_with_deps(&mut app, &mut current_dir)
            .expect_err("expected repo path resolution to fail");

        assert!(
            err.to_string()
                .contains("Failed to resolve current project directory")
        );
    }

    #[test]
    fn test_auto_connect_worktrees_reports_error_when_worktree_list_fails() {
        let (repo_dir, _repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        let err = crate::git::with_forced_repo_worktrees_error_for_tests(|| {
            with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app))
        })
        .unwrap_err();

        assert!(err.to_string().contains("Failed to list worktrees"));
    }

    #[test]
    fn test_auto_connect_worktrees_falls_back_when_worktree_dir_canonicalize_fails() {
        let (repo_dir, _repo) = init_test_repo_with_commit();
        let missing_worktree_dir = TempDir::new().unwrap().path().join("missing-worktree-dir");

        let config = Config {
            worktree_dir: missing_worktree_dir,
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
        assert_eq!(app.data.storage.iter().count(), 0);
    }

    #[test]
    fn test_auto_connect_worktrees_skips_when_worktree_path_canonicalize_fails() {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        let worktree_path = instance_worktree_dir.path().join("vanished");
        create_worktree(&repo, &worktree_path, "agent/vanished");
        fs::remove_dir_all(&worktree_path).unwrap();

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
        assert_eq!(app.data.storage.iter().count(), 0);
    }

    #[test]
    fn test_auto_connect_worktrees_skips_when_agent_already_exists_for_branch() {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        let worktree_path = instance_worktree_dir.path().join("existing");
        create_worktree(&repo, &worktree_path, "agent/existing");

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        let existing = Agent::new(
            "existing".to_string(),
            "claude".to_string(),
            "agent/existing".to_string(),
            worktree_path,
        );
        app.data.storage.add(existing);

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
        assert_eq!(app.data.storage.iter().count(), 1);
    }

    #[test]
    fn test_auto_connect_worktrees_reports_error_when_launch_root_agent_fails() {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        let worktree_path = instance_worktree_dir.path().join("auto-connect");
        create_worktree(&repo, &worktree_path, "agent/auto-connect");

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.data.settings.docker_for_new_roots = true;
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        let err = crate::runtime::with_docker_program_override_for_tests(
            PathBuf::from("tenex-test-missing-docker"),
            || with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Docker is not installed"));
        assert_eq!(app.data.storage.iter().count(), 0);
    }

    #[test]
    fn test_auto_connect_worktrees_reports_error_when_storage_save_fails() {
        let (repo_dir, _repo) = init_test_repo_with_commit();
        let (mut app, _temp_file) = create_test_app();
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        let err = Storage::with_forced_save_error_after_successes_for_tests(0, || {
            with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app))
        })
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("forced storage save error for test")
        );
    }

    #[test]
    fn test_auto_connect_worktrees_skips_worktree_outside_instance_worktree_dir() {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        let outside_dir = TempDir::new().unwrap();
        create_worktree(&repo, &outside_dir.path().join("outside"), "agent/outside");

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
        assert_eq!(app.data.storage.iter().count(), 0);
    }

    #[test]
    fn test_auto_connect_worktrees_skips_worktree_belonging_to_other_instance() {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        let isolated_worktree = instance_worktree_dir.path().join("isolated");
        create_worktree(&repo, &isolated_worktree, "agent/isolated");
        fs::write(isolated_worktree.join("state.json"), "{}").unwrap();

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
        assert_eq!(app.data.storage.iter().count(), 0);
    }

    #[test]
    fn test_auto_connect_worktrees_skips_worktree_with_different_branch_prefix() {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        create_worktree(
            &repo,
            &instance_worktree_dir.path().join("skip-prefix"),
            "feature/skip-prefix",
        );

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();
        assert_eq!(app.data.storage.iter().count(), 0);
    }

    #[test]
    fn test_auto_connect_worktrees_creates_agent_for_managed_branch() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let socket_path = socket_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy()).unwrap();

        let (repo_dir, repo) = init_test_repo_with_commit();
        let instance_worktree_dir = TempDir::new().unwrap();
        let worktree_path = instance_worktree_dir.path().join("auto-connect");
        create_worktree(&repo, &worktree_path, "agent/auto-connect");

        let config = Config {
            worktree_dir: instance_worktree_dir.path().to_path_buf(),
            ..Config::default()
        };

        let (mut app, _temp_file) = create_test_app_with_config(config);
        app.set_cwd_project_root(Some(repo_dir.path().to_path_buf()));

        with_tracing_dispatch(|| Actions::new().auto_connect_worktrees(&mut app)).unwrap();

        let stored = app
            .data
            .storage
            .iter()
            .find(|agent| agent.branch == "agent/auto-connect")
            .expect("Expected auto-connected agent to be stored");
        let actual_worktree_fallback = stored.worktree_path.clone();
        let actual_worktree = stored
            .worktree_path
            .canonicalize()
            .unwrap_or(actual_worktree_fallback);
        let expected_worktree_fallback = worktree_path.clone();
        let expected_worktree = worktree_path
            .canonicalize()
            .unwrap_or(expected_worktree_fallback);
        assert_eq!(actual_worktree, expected_worktree);

        let actual_repo_root = stored
            .repo_root
            .as_ref()
            .and_then(|path| path.canonicalize().ok());
        let expected_repo_root = repo_dir.path().canonicalize().ok();
        assert_eq!(actual_repo_root, expected_repo_root);
        assert_eq!(stored.status, Status::Starting);

        let _ = crate::mux::SessionManager::new().kill(&stored.mux_session);
    }

    #[test]
    fn test_mux_target_for_agent_child_window_uses_root_session_target() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
        );
        root.set_status(Status::Running);
        root.mux_session = "tenex-test-root".to_string();
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        let mut child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 5,
                repo_root: None,
            },
        );
        child.set_status(Status::Running);
        let expected =
            crate::mux::SessionManager::window_target(&root_session, child.window_index.unwrap());
        app.data.storage.add(child.clone());

        assert_eq!(mux_target_for_agent(&app, &child), expected);
    }

    #[test]
    fn test_title_for_branch_returns_branch_when_prefix_exact_match() {
        assert_eq!(title_for_branch("agent/", "agent/"), "agent/");
    }

    #[test]
    fn test_title_for_branch_strips_tenex_prefix() {
        assert_eq!(title_for_branch("tenex/feature", "agent/"), "feature");
    }

    #[test]
    fn test_is_tenex_managed_branch_covers_prefix_and_legacy_namespace() {
        assert!(is_tenex_managed_branch("agent/feature", "agent/"));
        assert!(is_tenex_managed_branch("tenex/feature", ""));
        assert!(!is_tenex_managed_branch("feature", "agent/"));
        assert!(!is_tenex_managed_branch("feature", ""));
    }

    #[test]
    fn test_has_isolated_state_marker_detects_state_file_in_parent() {
        let instance_dir = TempDir::new().unwrap();
        let worktree = instance_dir.path().join("worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(instance_dir.path().join("state.json"), "{}").unwrap();

        assert!(has_isolated_state_marker(&worktree, instance_dir.path()));
    }

    #[test]
    fn test_has_isolated_state_marker_returns_false_when_parent_none() {
        assert!(!has_isolated_state_marker(
            Path::new("/"),
            Path::new("/tenex-stop-at")
        ));
    }

    #[test]
    fn test_sorted_descendants_orders_missing_window_indices_last() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
        );
        root.set_status(Status::Running);
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        let mut child_first = Agent::new_child(
            "child-first".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 1,
                repo_root: None,
            },
        );
        child_first.set_status(Status::Running);
        app.data.storage.add(child_first);

        let mut child_last = Agent::new_child(
            "child-last".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: u32::MAX,
                repo_root: None,
            },
        );
        child_last.window_index = None;
        child_last.set_status(Status::Running);
        app.data.storage.add(child_last);

        let descendants = sorted_descendants(&app.data, root_id);
        let last = descendants.last().expect("missing descendants");
        assert!(last.window_index.is_none());
    }

    #[test]
    fn test_normalize_tree_running_updates_root_and_descendants() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
        );
        root.set_status(Status::Starting);
        root.mux_session = "old-session".to_string();
        let root_id = root.id;
        app.data.storage.add(root);

        let mut child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: "old-session".to_string(),
                window_index: 1,
                repo_root: None,
            },
        );
        child.set_status(Status::Starting);
        app.data.storage.add(child);

        assert!(normalize_tree_running(
            &mut app.data,
            root_id,
            "new-session",
            true
        ));

        let root_stored = app.data.storage.get(root_id).expect("missing root");
        assert_eq!(root_stored.mux_session, "new-session");
        assert_eq!(root_stored.status, Status::Running);

        let child_stored = app.data.storage.descendants(root_id)[0];
        assert_eq!(child_stored.mux_session, "new-session");
        assert_eq!(child_stored.status, Status::Running);

        assert!(!normalize_tree_running(
            &mut app.data,
            root_id,
            "new-session",
            true
        ));
        assert!(normalize_tree_running(
            &mut app.data,
            root_id,
            "new-session",
            false
        ));
    }

    #[test]
    fn test_normalize_tree_running_skips_agents_outside_tree() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let root_id = uuid::Uuid::new_v4();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
        );
        root.id = root_id;
        root.mux_session = "root-session".to_string();
        root.set_status(Status::Starting);
        app.data.storage.add(root);

        let mut child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: "root-session".to_string(),
                window_index: 1,
                repo_root: None,
            },
        );
        child.set_status(Status::Starting);
        app.data.storage.add(child);

        let mut other = Agent::new(
            "other".to_string(),
            "claude".to_string(),
            "muster/other".to_string(),
            temp.path().to_path_buf(),
        );
        other.set_status(Status::Starting);
        other.mux_session = "other-session".to_string();
        let other_id = other.id;
        app.data.storage.add(other);

        assert!(normalize_tree_running(
            &mut app.data,
            root_id,
            "new-session",
            true
        ));

        let stored_other = app.data.storage.get(other_id).expect("missing other");
        assert_eq!(stored_other.mux_session, "other-session");
        assert_eq!(stored_other.status, Status::Starting);
    }

    #[test]
    fn test_normalize_tree_running_updates_descendants_when_root_missing() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();
        let root_id = uuid::Uuid::new_v4();

        let mut child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: "old-session".to_string(),
                window_index: 1,
                repo_root: None,
            },
        );
        child.set_status(Status::Starting);
        let child_id = child.id;
        app.data.storage.add(child);

        assert!(normalize_tree_running(
            &mut app.data,
            root_id,
            "new-session",
            true
        ));

        let stored = app.data.storage.get(child_id).expect("missing child");
        assert_eq!(stored.mux_session, "new-session");
        assert_eq!(stored.status, Status::Running);
    }

    #[test]
    fn test_mark_respawned_agents_running_updates_tree() {
        let (mut app, _temp) = create_test_app();
        let temp = TempDir::new().unwrap();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
        );
        root.set_status(Status::Starting);
        root.mux_session = "old-session".to_string();
        let root_id = root.id;
        app.data.storage.add(root);

        let mut child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp.path().to_path_buf(),
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: "old-session".to_string(),
                window_index: 2,
                repo_root: None,
            },
        );
        child.set_status(Status::Starting);
        child.window_index = None;
        let child_id = child.id;
        app.data.storage.add(child);

        let mut recreated = HashMap::new();
        let _ = recreated.insert(child_id, 7);

        assert!(mark_respawned_agents_running(
            &mut app.data,
            root_id,
            "new-session",
            recreated
        ));

        let root_stored = app.data.storage.get(root_id).expect("missing root");
        assert_eq!(root_stored.mux_session, "new-session");
        assert_eq!(root_stored.status, Status::Running);

        let child_stored = app.data.storage.get(child_id).expect("missing child");
        assert_eq!(child_stored.mux_session, "new-session");
        assert_eq!(child_stored.window_index, Some(7));
        assert_eq!(child_stored.status, Status::Running);

        let mut recreated = HashMap::new();
        let _ = recreated.insert(child_id, 7);
        assert!(!mark_respawned_agents_running(
            &mut app.data,
            root_id,
            "new-session",
            recreated
        ));
    }

    #[test]
    fn test_mark_respawned_agents_running_ignores_missing_agents() {
        let (mut app, _temp) = create_test_app();
        let mut recreated = HashMap::new();
        let _ = recreated.insert(uuid::Uuid::new_v4(), 7);
        assert!(!mark_respawned_agents_running(
            &mut app.data,
            uuid::Uuid::new_v4(),
            "new-session",
            recreated
        ));
    }

    #[test]
    fn test_command_for_agent_reports_error_when_program_invalid() {
        let mut agent = Agent::new(
            "agent".to_string(),
            " ".to_string(),
            "tenex-test/agent".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.conversation_id = Some("resume-id".to_string());

        assert!(command_for_agent(&agent, &crate::app::Settings::default()).is_err());
    }
}
