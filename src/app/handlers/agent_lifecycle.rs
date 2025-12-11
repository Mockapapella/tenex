//! Agent lifecycle operations: create, kill, reconnect

use crate::agent::{Agent, ChildConfig};
use crate::git::{self, WorktreeManager};
use crate::tmux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::state::{App, Mode, WorktreeConflictInfo};

impl Actions {
    /// Create a new agent
    ///
    /// If a worktree with the same name already exists, this will prompt the user
    /// to either reconnect to the existing worktree or recreate it from scratch.
    ///
    /// # Errors
    ///
    /// Returns an error if agent creation fails
    pub fn create_agent(self, app: &mut App, title: &str, prompt: Option<&str>) -> Result<()> {
        debug!(title, prompt, "Creating new agent");

        let branch = app.config.generate_branch_name(title);
        let worktree_path = app.config.worktree_dir.join(&branch);
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;

        let worktree_mgr = WorktreeManager::new(&repo);

        // Check if worktree/branch already exists - prompt user for action
        if worktree_mgr.exists(&branch) {
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

            app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
                title: title.to_string(),
                prompt: prompt.map(String::from),
                branch: branch.clone(),
                worktree_path: worktree_path.clone(),
                existing_branch,
                existing_commit,
                current_branch,
                current_commit,
                swarm_child_count: None, // Not a swarm creation
            });
            app.enter_mode(Mode::Confirming(
                crate::app::state::ConfirmAction::WorktreeConflict,
            ));
            return Ok(());
        }

        self.create_agent_internal(app, title, prompt, &branch, &worktree_path)
    }

    /// Internal function to actually create the agent after conflict resolution
    pub(crate) fn create_agent_internal(
        self,
        app: &mut App,
        title: &str,
        prompt: Option<&str>,
        branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<()> {
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let worktree_mgr = WorktreeManager::new(&repo);

        worktree_mgr.create_with_new_branch(worktree_path, branch)?;

        let agent = Agent::new(
            title.to_string(),
            app.config.default_program.clone(),
            branch.to_string(),
            worktree_path.to_path_buf(),
            prompt.map(String::from),
        );

        let mut command = app.config.default_program.clone();
        if let Some(p) = prompt {
            // Pass prompt as positional argument (works for codex, claude, etc.)
            command = format!("{} \"{}\"", command, p.replace('"', "\\\""));
        }

        self.session_manager
            .create(&agent.tmux_session, worktree_path, Some(&command))?;

        // Resize the new session to match preview dimensions
        if let Some((width, height)) = app.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&agent.tmux_session, width, height);
        }

        app.storage.add(agent);
        app.storage.save()?;

        info!(title, %branch, "Agent created successfully");
        app.set_status(format!("Created agent: {title}"));
        Ok(())
    }

    /// Reconnect to an existing worktree (user chose to keep it)
    ///
    /// # Errors
    ///
    /// Returns an error if the tmux session cannot be created or storage fails
    pub fn reconnect_to_worktree(self, app: &mut App) -> Result<()> {
        let conflict = app
            .spawn
            .worktree_conflict
            .take()
            .ok_or_else(|| anyhow::anyhow!("No worktree conflict info available"))?;

        debug!(branch = %conflict.branch, swarm_child_count = ?conflict.swarm_child_count, "Reconnecting to existing worktree");

        // Check if this is a swarm creation (has child count)
        if let Some(child_count) = conflict.swarm_child_count {
            // Create root agent for swarm
            let root_agent = Agent::new(
                conflict.title.clone(),
                app.config.default_program.clone(),
                conflict.branch.clone(),
                conflict.worktree_path.clone(),
                None, // Root doesn't get the prompt
            );

            let root_session = root_agent.tmux_session.clone();
            let root_id = root_agent.id;

            // Create the root's tmux session
            self.session_manager.create(
                &root_session,
                &conflict.worktree_path,
                Some(&app.config.default_program),
            )?;

            // Resize the session to match preview dimensions
            if let Some((width, height)) = app.ui.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&root_session, width, height);
            }

            app.storage.add(root_agent);

            // Now spawn the children
            let task = conflict.prompt.as_deref().unwrap_or("");
            self.spawn_children_for_root(
                app,
                &root_session,
                &conflict.worktree_path,
                &conflict.branch,
                root_id,
                child_count,
                task,
            )?;

            info!(title = %conflict.title, branch = %conflict.branch, child_count, "Reconnected swarm to existing worktree");
            app.set_status(format!("Reconnected swarm: {}", conflict.title));
        } else {
            // Single agent reconnect
            let agent = Agent::new(
                conflict.title.clone(),
                app.config.default_program.clone(),
                conflict.branch.clone(),
                conflict.worktree_path.clone(),
                conflict.prompt.clone(),
            );

            let mut command = app.config.default_program.clone();
            if let Some(ref p) = conflict.prompt {
                command = format!("{} \"{}\"", command, p.replace('"', "\\\""));
            }

            self.session_manager.create(
                &agent.tmux_session,
                &conflict.worktree_path,
                Some(&command),
            )?;

            // Resize the new session to match preview dimensions
            if let Some((width, height)) = app.ui.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&agent.tmux_session, width, height);
            }

            app.storage.add(agent);

            info!(title = %conflict.title, branch = %conflict.branch, "Reconnected to existing worktree");
            app.set_status(format!("Reconnected to: {}", conflict.title));
        }

        app.storage.save()?;
        Ok(())
    }

    /// Recreate the worktree (user chose to delete and start fresh)
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be removed/recreated or agent creation fails
    pub fn recreate_worktree(self, app: &mut App) -> Result<()> {
        let conflict = app
            .spawn
            .worktree_conflict
            .take()
            .ok_or_else(|| anyhow::anyhow!("No worktree conflict info available"))?;

        debug!(branch = %conflict.branch, swarm_child_count = ?conflict.swarm_child_count, "Recreating worktree from scratch");

        // Remove existing worktree first
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let worktree_mgr = WorktreeManager::new(&repo);
        worktree_mgr.remove(&conflict.branch)?;

        // Check if this is a swarm creation
        if let Some(child_count) = conflict.swarm_child_count {
            // Set up app state for spawn_children
            app.spawn.spawning_under = None;
            app.spawn.child_count = child_count;

            // Call spawn_children with the task/prompt (if any)
            self.spawn_children(app, conflict.prompt.as_deref())
        } else {
            // Single agent creation
            self.create_agent_internal(
                app,
                &conflict.title,
                conflict.prompt.as_deref(),
                &conflict.branch,
                &conflict.worktree_path,
            )
        }
    }

    /// Kill the selected agent (and all its descendants)
    pub(crate) fn kill_agent(self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            let is_root = agent.is_root();
            let session = agent.tmux_session.clone();
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
                // Root agent: kill entire session and worktree
                // First kill all descendant windows in descending order
                // (in case any are in other sessions, and to handle renumbering)
                let descendants = app.storage.descendants(agent_id);
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

                // Brief delay to allow tmux processes to terminate
                // tmux kill-session sends SIGTERM and returns immediately,
                // but processes may still be running and have files open
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Remove worktree
                let repo_path = std::env::current_dir()?;
                if let Ok(repo) = git::open_repository(&repo_path) {
                    let worktree_mgr = WorktreeManager::new(&repo);
                    if let Err(e) = worktree_mgr.remove(&worktree_name) {
                        warn!("Failed to remove worktree: {e}");
                        app.set_status(format!("Warning: {e}"));
                    }
                }
            } else {
                // Child agent: kill just this window and its descendants
                // Get the root's session for killing windows
                let root = app.storage.root_ancestor(agent_id);
                let root_session = root.map_or_else(|| session.clone(), |r| r.tmux_session.clone());
                let root_id = root.map(|r| r.id);

                // Collect all window indices being deleted
                let mut deleted_indices: Vec<u32> = Vec::new();
                let descendants = app.storage.descendants(agent_id);
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
                // This prevents tmux renumbering from affecting indices we haven't killed yet
                deleted_indices.sort_unstable_by(|a, b| b.cmp(a));
                for idx in &deleted_indices {
                    let _ = self.session_manager.kill_window(&root_session, *idx);
                }

                // Update window indices for remaining agents under the same root
                // When tmux has renumber-windows on, indices shift down
                if let Some(rid) = root_id {
                    super::window::adjust_window_indices_after_deletion(
                        app,
                        rid,
                        agent_id,
                        &deleted_indices,
                    );
                }
            }

            // Remove agent and all descendants from storage
            app.storage.remove_with_descendants(agent_id);

            app.validate_selection();
            app.storage.save()?;

            // Immediately update preview/diff to show the newly selected agent
            let _ = self.update_preview(app);
            let _ = self.update_diff(app);

            app.set_status("Agent killed");
        }
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
    pub fn spawn_terminal(self, app: &mut App, startup_command: Option<&str>) -> Result<()> {
        // Must have a selected agent
        let selected = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        // Get the root ancestor to use its tmux session
        let selected_id = selected.id;
        let root = app
            .storage
            .root_ancestor(selected_id)
            .ok_or_else(|| anyhow::anyhow!("Could not find root agent"))?;

        let root_session = root.tmux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();
        let root_id = root.id;

        let title = app.next_terminal_name();
        debug!(title, startup_command, "Creating new terminal");

        // Reserve a window index
        let window_index = app.storage.reserve_window_indices(root_id);

        // Create child agent marked as terminal
        let mut terminal = Agent::new_child(
            title.clone(),
            "bash".to_string(),
            branch,
            worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session.clone(),
                window_index,
            },
        );
        terminal.is_terminal = true;

        // Create window in the root's session (no command - just a shell)
        let actual_index =
            self.session_manager
                .create_window(&root_session, &title, &worktree_path, None)?;

        // Resize the new window to match preview dimensions
        if let Some((width, height)) = app.ui.preview_dimensions {
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

        app.storage.add(terminal);

        // Expand the parent to show the new terminal
        if let Some(parent) = app.storage.get_mut(root_id) {
            parent.collapsed = false;
        }

        app.storage.save()?;

        info!(title, "Terminal created successfully");
        app.set_status(format!("Created terminal: {title}"));
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
    fn test_reconnect_to_worktree_no_conflict_info() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No conflict info set - should error
        let result = handler.reconnect_to_worktree(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_recreate_worktree_no_conflict_info() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No conflict info set - should error
        let result = handler.recreate_worktree(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_handle_confirm_kill() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp/nonexistent"),
            None,
        ));

        // Enter confirming mode for kill
        app.enter_mode(Mode::Confirming(crate::app::state::ConfirmAction::Kill));

        // Confirm should kill and exit mode
        handler.handle_action(&mut app, crate::config::Action::Confirm)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_kill_agent_root() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a root agent
        app.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Kill should work (session doesn't exist, but should not error)
        handler.kill_agent(&mut app)?;
        assert_eq!(app.storage.len(), 0);
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
            None,
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session,
                window_index: 2,
            },
        );
        app.storage.add(child);

        // Select the child (it's the second visible agent)
        app.select_next();

        // Kill child should remove just the child
        handler.kill_agent(&mut app)?;
        assert_eq!(app.storage.len(), 1);
        assert!(app.storage.get(root_id).is_some());
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
            None,
        );
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add children
        for i in 0..3 {
            app.storage.add(Agent::new_child(
                format!("child{i}"),
                "claude".to_string(),
                "muster/root".to_string(),
                PathBuf::from("/tmp"),
                None,
                ChildConfig {
                    parent_id: root_id,
                    tmux_session: root_session.clone(),
                    window_index: i + 2,
                },
            ));
        }

        // Kill root should remove all
        handler.kill_agent(&mut app)?;
        assert_eq!(app.storage.len(), 0);
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
            None,
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp/worktree"),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session,
                window_index: 2,
            },
        );
        let child_id = child.id;
        app.storage.add(child);

        // Select the child (second visible agent)
        app.select_next();
        assert_eq!(app.selected_agent().map(|a| a.id), Some(child_id));

        // Spawn terminal - should fail because tmux session doesn't exist
        let result = handler.spawn_terminal(&mut app, None);

        // Should fail because tmux session doesn't exist
        assert!(result.is_err());
        Ok(())
    }
}
