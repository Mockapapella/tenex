//! Git operations: Push, Rename Branch, Open PR, Rebase, Merge

mod merge;
mod open_pr;
mod push;
mod rebase;
mod rename;

use crate::agent::{Agent, ChildConfig};
use crate::mux::SessionManager;
use anyhow::Result;
use tracing::{debug, info};

use crate::app::state::App;

use super::Actions;

impl Actions {
    /// Spawn a terminal for resolving conflicts
    fn spawn_conflict_terminal(app: &mut App, title: &str, startup_command: &str) -> Result<()> {
        let agent_id = app
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID"))?;

        // Verify agent exists
        if app.storage.get(agent_id).is_none() {
            anyhow::bail!("Agent not found");
        }

        // Get the root ancestor to use its mux session
        let root = app
            .storage
            .root_ancestor(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Could not find root agent"))?;

        let root_session = root.mux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();
        let root_id = root.id;

        debug!(title, startup_command, "Creating conflict terminal");

        // Reserve a window index
        let window_index = app.storage.reserve_window_indices(root_id);

        // Create child agent marked as terminal
        let mut terminal = Agent::new_child(
            title.to_string(),
            "terminal".to_string(),
            branch,
            worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index,
            },
        );
        terminal.is_terminal = true;

        // Create session manager and window
        let session_manager = SessionManager::new();
        let actual_index =
            session_manager.create_window(&root_session, title, &worktree_path, None)?;

        // Resize the new window to match preview dimensions
        if let Some((width, height)) = app.ui.preview_dimensions {
            let window_target = SessionManager::window_target(&root_session, actual_index);
            let _ = session_manager.resize_window(&window_target, width, height);
        }

        // Update window index if it differs
        terminal.window_index = Some(actual_index);

        // Send the startup command
        let window_target = SessionManager::window_target(&root_session, actual_index);
        session_manager.send_keys_and_submit(&window_target, startup_command)?;

        app.storage.add(terminal);

        // Expand the parent to show the new terminal
        if let Some(parent) = app.storage.get_mut(root_id) {
            parent.collapsed = false;
        }

        app.storage.save()?;

        // Clear git op state and exit mode
        app.clear_git_op_state();
        app.clear_review_state();
        app.exit_mode();

        info!(
            title,
            "Conflict terminal created - user can resolve conflicts"
        );
        app.set_status(format!("Opened terminal for conflict resolution: {title}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests;

