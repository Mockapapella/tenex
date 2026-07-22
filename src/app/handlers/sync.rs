//! Sync operations: agent status synchronization and auto-connect

use crate::agent::{Agent, Status};
use crate::git::{self, WorktreeManager};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::path::Path;
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::{App, AppData, PaneActivityDigestMode};

impl Actions {
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
        let repo_path = match app.data.cwd_project_root.clone() {
            Some(root) => root,
            None => {
                std::env::current_dir().context("Failed to resolve current project directory")?
            }
        };
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

    fn respawn_missing_agents_in_data(self, app_data: &mut AppData) -> Result<()> {
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
