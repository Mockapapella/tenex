//! Sync operations: agent status synchronization and auto-connect

use crate::agent::{Agent, Status};
use crate::git::{self, WorktreeManager};
use anyhow::{Context, Result};
use tracing::{debug, info};

use super::Actions;
use crate::app::state::App;

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
        Self::sync_agent_status_with_sessions(app, sessions)
    }

    fn sync_agent_status_with_sessions(
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
        if sessions.is_empty() && !app.storage.is_empty() {
            debug!("Mux session list empty; skipping agent sync");
            return Ok(());
        }

        let active_sessions: std::collections::HashSet<String> =
            sessions.into_iter().map(|s| s.name).collect();

        // Remove stored agents whose sessions no longer exist.
        let roots: Vec<Agent> = app.storage.root_agents().into_iter().cloned().collect();
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

            debug!(title = %root.title, session = %root.mux_session, "Removing agent with missing mux session");
            app.storage.remove_with_descendants(root.id);
            changed = true;
        }

        // Update starting agents to running if their session exists
        for agent in app.storage.iter_mut() {
            if agent.status == Status::Starting && active_sessions.contains(&agent.mux_session) {
                debug!(title = %agent.title, "Agent status: Starting -> Running");
                agent.set_status(Status::Running);
                changed = true;
            }
        }

        if changed {
            app.storage.save()?;
            app.validate_selection();
        }

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

        debug!(count = worktrees.len(), "Found worktrees for auto-connect");

        for wt in worktrees {
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
            if !branch_name.starts_with(&app.config.branch_prefix) {
                debug!(branch = %branch_name, prefix = %app.config.branch_prefix, "Skipping worktree with different prefix");
                continue;
            }

            // Check if there's already an agent for this branch
            let agent_exists = app.storage.iter().any(|a| a.branch == branch_name);
            if agent_exists {
                debug!(branch = %branch_name, "Agent already exists for worktree");
                continue;
            }

            info!(branch = %branch_name, path = ?wt.path, "Auto-connecting to existing worktree");

            // Create an agent for this worktree
            let agent = Agent::new(
                branch_name.clone(), // Use branch name as title
                program.clone(),
                branch_name.clone(),
                wt.path.clone(),
                None, // No initial prompt
            );

            // Create mux session and start the agent program
            let command = crate::command::build_command_argv(&program, None)?;
            self.session_manager
                .create(&agent.mux_session, &wt.path, Some(&command))?;

            // Resize the session to match preview dimensions if available
            if let Some((width, height)) = app.ui.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&agent.mux_session, width, height);
            }

            app.storage.add(agent);
            info!(branch = %branch_name, "Auto-connected to existing worktree");
        }

        // Save storage if we added any agents
        app.storage.save()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

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
            None,
        );
        running.set_status(Status::Running);
        app.storage.add(running);

        let mut starting = Agent::new(
            "starting".to_string(),
            "claude".to_string(),
            "muster/starting".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        starting.set_status(Status::Starting);
        app.storage.add(starting);

        // When mux session listing succeeds but reports no sessions, treat it as uncertain and
        // avoid destructive pruning.
        Actions::sync_agent_status_with_sessions(&mut app, Ok(vec![]))?;

        assert_eq!(app.storage.len(), 2);
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
            None,
        );
        alive.set_status(Status::Running);
        let alive_session = alive.mux_session.clone();
        app.storage.add(alive);

        let mut missing = Agent::new(
            "missing".to_string(),
            "claude".to_string(),
            "muster/missing".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        missing.set_status(Status::Running);
        let missing_id = missing.id;
        app.storage.add(missing);

        Actions::sync_agent_status_with_sessions(
            &mut app,
            Ok(vec![crate::mux::Session {
                name: alive_session,
                created: 0,
                attached: false,
            }]),
        )?;

        assert_eq!(app.storage.len(), 1);
        assert!(app.storage.get(missing_id).is_none());
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
            None,
        );
        agent.set_status(Status::Starting);
        let session = agent.mux_session.clone();
        let agent_id = agent.id;
        app.storage.add(agent);

        Actions::sync_agent_status_with_sessions(
            &mut app,
            Ok(vec![crate::mux::Session {
                name: session,
                created: 0,
                attached: false,
            }]),
        )?;

        assert_eq!(
            app.storage.get(agent_id).ok_or("Agent missing")?.status,
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
            None,
        );
        agent.set_status(Status::Running);
        app.storage.add(agent);

        Actions::sync_agent_status_with_sessions(&mut app, Err(anyhow::anyhow!("mux down")))?;
        assert_eq!(app.storage.len(), 1);
        Ok(())
    }
}
