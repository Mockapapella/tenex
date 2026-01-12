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
use crate::app::App;

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

    /// Update per-agent activity indicators by diffing pane output once per interval.
    ///
    /// If the visible pane content for an agent has not changed since the previous observation,
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
            return Ok(());
        }

        let selected_agent_id = app.selected_agent().map(|agent| agent.id);

        let mut keep_ids: HashSet<uuid::Uuid> = HashSet::new();

        for agent in app.data.storage.iter() {
            keep_ids.insert(agent.id);

            // Only track activity once the session exists and the agent is running.
            if agent.status != Status::Running {
                continue;
            }

            let target = mux_target_for_agent(app, agent);
            let Ok(content) = self.output_capture.capture_pane(&target) else {
                continue;
            };

            let digest = hash_text(&content);
            app.data.ui.observe_agent_pane_digest(agent.id, digest);

            if selected_agent_id == Some(agent.id) {
                app.data
                    .ui
                    .pane_last_seen_hash_by_agent
                    .insert(agent.id, digest);
            }
        }

        app.data
            .ui
            .retain_agent_pane_digests(|id| keep_ids.contains(id));
        app.data
            .ui
            .retain_agent_pane_last_seen_hashes(|id| keep_ids.contains(id));

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
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let Ok(repo) = git::open_repository(&repo_path) else {
            // Not in a git repository, nothing to auto-connect
            debug!("Not in a git repository, skipping auto-connect");
            return Ok(());
        };

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktrees = worktree_mgr.list()?;
        let program = app.agent_spawn_command();
        let session_prefix = app.data.storage.instance_session_prefix();
        let instance_worktree_dir = app
            .data
            .config
            .worktree_dir
            .canonicalize()
            .unwrap_or_else(|_| app.data.config.worktree_dir.clone());

        debug!(count = worktrees.len(), "Found worktrees for auto-connect");

        for wt in worktrees {
            let worktree_path = wt.path.canonicalize().unwrap_or_else(|_| wt.path.clone());

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

            // Only process worktrees that match our branch prefix
            if !branch_name.starts_with(&app.data.config.branch_prefix) {
                debug!(branch = %branch_name, prefix = %app.data.config.branch_prefix, "Skipping worktree with different prefix");
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
                branch_name.clone(), // Use branch name as title
                program.clone(),
                branch_name.clone(),
                worktree_path.clone(),
            );
            agent.mux_session = format!("{session_prefix}{}", agent.short_id());

            // Create mux session and start the agent program
            let command = crate::command::build_command_argv(&program, None)?;
            self.session_manager
                .create(&agent.mux_session, &worktree_path, Some(&command))?;

            // Resize the session to match preview dimensions if available
            if let Some((width, height)) = app.data.ui.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&agent.mux_session, width, height);
            }

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
        let roots = stored_root_agents(app);
        if roots.is_empty() {
            return Ok(());
        }

        let session_manager = self.session_manager;
        let mut summary = RespawnSummary::default();
        for root in &roots {
            respawn_root_agent(session_manager, app, root, &mut summary);
        }

        if summary.changed {
            app.data.storage.save()?;
            app.validate_selection();
        }

        if summary.respawned_sessions > 0 {
            app.data.set_status(format!(
                "Respawned {} agent session(s)",
                summary.respawned_sessions
            ));
        }

        Ok(())
    }
}

fn mux_target_for_agent(app: &App, agent: &Agent) -> String {
    agent.window_index.map_or_else(
        || agent.mux_session.clone(),
        |window_idx| {
            // Child agent: target specific window within root's session.
            let root = app.data.storage.root_ancestor(agent.id);
            let root_session =
                root.map_or_else(|| agent.mux_session.clone(), |r| r.mux_session.clone());
            crate::mux::SessionManager::window_target(&root_session, window_idx)
        },
    )
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[derive(Default)]
struct RespawnSummary {
    changed: bool,
    respawned_sessions: usize,
}

fn stored_root_agents(app: &App) -> Vec<Agent> {
    app.data
        .storage
        .root_agents()
        .into_iter()
        .cloned()
        .collect()
}

fn respawn_root_agent(
    session_manager: crate::mux::SessionManager,
    app: &mut App,
    root: &Agent,
    summary: &mut RespawnSummary,
) {
    if session_manager.exists(&root.mux_session) {
        summary.changed |= normalize_tree_running(app, root.id, &root.mux_session, true);
        return;
    }

    // Mark as not running while we attempt to respawn.
    summary.changed |= normalize_tree_running(app, root.id, &root.mux_session, false);

    if !root.worktree_path.exists() {
        warn!(
            title = %root.title,
            branch = %root.branch,
            worktree = %root.worktree_path.display(),
            "Worktree missing; cannot respawn agent session"
        );
        return;
    }

    let root_command = match command_for_agent(root) {
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

    let descendants = sorted_descendants(app, root.id);
    let recreated_window_indices =
        recreate_descendant_windows(session_manager, &root.mux_session, &descendants);
    summary.changed |=
        mark_respawned_agents_running(app, root.id, &root.mux_session, recreated_window_indices);
}

fn sorted_descendants(app: &App, root_id: uuid::Uuid) -> Vec<Agent> {
    let mut descendants: Vec<Agent> = app
        .data
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

        let command = match command_for_agent(desc) {
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
    app: &mut App,
    root_id: uuid::Uuid,
    mux_session: &str,
    recreated_window_indices: HashMap<uuid::Uuid, u32>,
) -> bool {
    let mut changed = false;
    let mux_session_owned = mux_session.to_owned();

    if let Some(agent) = app.data.storage.get_mut(root_id) {
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
        if let Some(agent) = app.data.storage.get_mut(agent_id) {
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

fn normalize_tree_running(
    app: &mut App,
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

    if let Some(agent) = app.data.storage.get_mut(root_id) {
        if agent.mux_session != mux_session {
            agent.mux_session = mux_session.to_string();
            changed = true;
        }
        if agent.status != status {
            agent.set_status(status);
            changed = true;
        }
    }

    let descendant_ids = app.data.storage.descendant_ids(root_id);
    for agent_id in descendant_ids {
        if let Some(agent) = app.data.storage.get_mut(agent_id) {
            if agent.mux_session != mux_session {
                agent.mux_session = mux_session.to_string();
                changed = true;
            }
            if agent.status != status {
                agent.set_status(status);
                changed = true;
            }
        }
    }

    changed
}

fn command_for_agent(agent: &Agent) -> Result<Option<Vec<String>>> {
    if agent.is_terminal || agent.program == "terminal" {
        return Ok(None);
    }

    Ok(Some(crate::command::build_command_argv(
        &agent.program,
        None,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
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

    #[test]
    fn test_sync_agent_status() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.sync_agent_status(&mut app)?;
        Ok(())
    }

    #[test]
    fn test_sync_agent_status_with_agents() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

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
        Actions::new().sync_agent_status_with_sessions(&mut app, Ok(vec![]))?;

        assert_eq!(app.data.storage.len(), 2);
        Ok(())
    }

    #[test]
    fn test_sync_agent_status_prunes_missing_sessions() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

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

        Actions::new().sync_agent_status_with_sessions(
            &mut app,
            Ok(vec![crate::mux::Session {
                name: alive_session,
                created: 0,
                attached: false,
            }]),
        )?;

        assert_eq!(app.data.storage.len(), 1);
        assert!(app.data.storage.get(missing_id).is_none());
        Ok(())
    }

    #[test]
    fn test_sync_agent_status_promotes_starting_when_session_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

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

        Actions::new().sync_agent_status_with_sessions(
            &mut app,
            Ok(vec![crate::mux::Session {
                name: session,
                created: 0,
                attached: false,
            }]),
        )?;

        assert_eq!(
            app.data
                .storage
                .get(agent_id)
                .ok_or("Agent missing")?
                .status,
            Status::Running
        );
        Ok(())
    }

    #[test]
    fn test_sync_agent_status_list_error_does_not_prune() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;

        // Add a running root agent.
        let mut agent = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.set_status(Status::Running);
        app.data.storage.add(agent);

        Actions::new()
            .sync_agent_status_with_sessions(&mut app, Err(anyhow::anyhow!("mux down")))?;
        assert_eq!(app.data.storage.len(), 1);
        Ok(())
    }

    #[test]
    fn test_respawn_missing_agents_creates_sessions_and_windows()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let worktree = TempDir::new()?;
        let worktree_path = worktree.path().to_path_buf();

        let mut root = Agent::new(
            "root".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            "tenex-test/root".to_string(),
            worktree_path.clone(),
        );
        root.set_status(Status::Running);
        let root_session = root.mux_session.clone();
        let root_id = root.id;

        let child = Agent::new_child(
            "child".to_string(),
            "sh -c 'sleep 3600'".to_string(),
            root.branch.clone(),
            worktree_path,
            crate::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 1,
            },
        );

        app.data.storage.add(root);
        app.data.storage.add(child);
        app.data.storage.save()?;

        // Ensure the session doesn't already exist (best-effort).
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Actions::new().respawn_missing_agents(&mut app)?;

        assert!(crate::mux::SessionManager::new().exists(&root_session));

        let windows = crate::mux::SessionManager::new().list_windows(&root_session)?;
        assert!(windows.iter().any(|w| w.index == 0));
        assert!(windows.iter().any(|w| w.name == "child"));

        // Clean up to avoid leaving long-running processes around.
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Ok(())
    }

    #[test]
    fn test_respawn_missing_agents_ignores_legacy_initial_prompt()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_file = NamedTempFile::new()?;
        let state_path = temp_file.path().to_path_buf();

        let worktree = TempDir::new()?;
        let worktree_path = worktree.path().to_path_buf();

        // This program prints $1, then sleeps so we can capture output. The `_` argument is the
        // arg0 placeholder for `sh -c`, so an appended prompt becomes $1.
        let program = "sh -c 'echo ARG1:$1; sleep 3600' _".to_string();

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
        let mut value = serde_json::to_value(&storage)?;
        let agents = value
            .get_mut("agents")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("Expected agents array")?;
        let root_value = agents
            .first_mut()
            .and_then(serde_json::Value::as_object_mut)
            .ok_or("Expected root agent object")?;
        root_value.insert(
            "initial_prompt".to_string(),
            serde_json::Value::String("PROMPT".to_string()),
        );
        std::fs::write(&state_path, serde_json::to_string_pretty(&value)?)?;

        // Load the state and ensure subsequent saves target the temp file (not the real state).
        let mut loaded = Storage::load_from(&state_path)?;
        loaded.state_path = Some(state_path);

        let mut app = App::new(Config::default(), loaded, Settings::default(), false);

        // Ensure the session doesn't already exist (best-effort).
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Actions::new().respawn_missing_agents(&mut app)?;

        std::thread::sleep(std::time::Duration::from_millis(200));
        let output = crate::mux::OutputCapture::new().capture_pane(&root_session)?;
        assert!(
            output.contains("ARG1:"),
            "Expected respawned agent output to include ARG1:, got: {output:?}"
        );
        assert!(
            !output.contains("PROMPT"),
            "Respawn should not replay legacy initial_prompt. Got: {output:?}"
        );

        // Clean up to avoid leaving long-running processes around.
        let _ = crate::mux::SessionManager::new().kill(&root_session);

        Ok(())
    }
}
