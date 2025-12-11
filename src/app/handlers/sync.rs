//! Sync operations: agent status synchronization and auto-connect

use crate::agent::{Agent, Status};
use crate::git::{self, WorktreeManager};
use anyhow::{Context, Result};
use tracing::{debug, info};

use super::Actions;
use crate::app::state::App;

impl Actions {
    /// Check and update agent statuses based on tmux sessions
    ///
    /// # Errors
    ///
    /// Returns an error if status sync fails
    pub fn sync_agent_status(self, app: &mut App) -> Result<()> {
        let mut changed = false;

        // Fetch all sessions once instead of calling exists() per agent
        // This reduces subprocess calls from O(n) to O(1)
        let active_sessions: std::collections::HashSet<String> = self
            .session_manager
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.name)
            .collect();

        // Collect IDs of dead agents to remove
        let dead_agents: Vec<uuid::Uuid> = app
            .storage
            .iter()
            .filter(|agent| !active_sessions.contains(&agent.tmux_session))
            .map(|agent| {
                debug!(title = %agent.title, "Removing dead agent (session not found)");
                agent.id
            })
            .collect();

        // Remove dead agents
        for id in dead_agents {
            app.storage.remove(id);
            changed = true;
        }

        // Update starting agents to running if their session exists
        for agent in app.storage.iter_mut() {
            if agent.status == Status::Starting {
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
                app.config.default_program.clone(),
                branch_name.clone(),
                wt.path.clone(),
                None, // No initial prompt
            );

            // Create tmux session and start the agent program
            let command = app.config.default_program.clone();
            self.session_manager
                .create(&agent.tmux_session, &wt.path, Some(&command))?;

            // Resize the session to match preview dimensions if available
            if let Some((width, height)) = app.ui.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&agent.tmux_session, width, height);
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
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((App::new(Config::default(), storage), temp_file))
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
        let handler = Actions::new();
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

        // Sync should remove dead agents (no sessions exist)
        handler.sync_agent_status(&mut app)?;

        // All agents should be removed since their sessions don't exist
        assert_eq!(app.storage.len(), 0);
        Ok(())
    }
}
