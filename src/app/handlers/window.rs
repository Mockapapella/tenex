//! Window operations: resize and adjust window indices

use tracing::debug;

use super::Actions;
use crate::app::state::App;

impl Actions {
    /// Resize all agent tmux windows to match the preview pane dimensions
    ///
    /// This ensures the terminal output renders correctly in the preview pane.
    pub fn resize_agent_windows(&self, app: &App) {
        let Some((width, height)) = app.preview_dimensions else {
            return;
        };

        for agent in app.storage.iter() {
            if agent.is_root() {
                // Root agent: resize the session
                if self.session_manager.exists(&agent.tmux_session) {
                    let _ = self
                        .session_manager
                        .resize_window(&agent.tmux_session, width, height);
                }
            } else if let Some(window_idx) = agent.window_index {
                // Child agent: resize the specific window
                let root = app.storage.root_ancestor(agent.id);
                if let Some(root_agent) = root
                    && self.session_manager.exists(&root_agent.tmux_session)
                {
                    let window_target = crate::tmux::SessionManager::window_target(
                        &root_agent.tmux_session,
                        window_idx,
                    );
                    let _ = self
                        .session_manager
                        .resize_window(&window_target, width, height);
                }
            }
        }
    }
}

/// Adjust window indices for all agents under a root after windows are deleted
///
/// This handles the case where tmux has `renumber-windows on` and
/// window indices shift after windows are deleted. We compute the new
/// indices mathematically rather than relying on window names.
pub fn adjust_window_indices_after_deletion(
    app: &mut App,
    root_id: uuid::Uuid,
    deleted_agent_id: uuid::Uuid,
    deleted_indices: &[u32],
) {
    if deleted_indices.is_empty() {
        return;
    }

    // Sort deleted indices for efficient counting
    let mut sorted_deleted: Vec<u32> = deleted_indices.to_vec();
    sorted_deleted.sort_unstable();

    // Get all descendants of the root (excluding the deleted agent and its descendants)
    let descendants_to_update: Vec<uuid::Uuid> = app
        .storage
        .descendants(root_id)
        .iter()
        .filter(|a| a.id != deleted_agent_id)
        .filter(|a| !app.storage.descendant_ids(deleted_agent_id).contains(&a.id))
        .map(|a| a.id)
        .collect();

    // Update each remaining agent's window index
    for agent_id in descendants_to_update {
        if let Some(agent) = app.storage.get_mut(agent_id)
            && let Some(current_idx) = agent.window_index
        {
            // Count how many deleted indices are less than current index
            let decrement =
                u32::try_from(sorted_deleted.iter().filter(|&&d| d < current_idx).count())
                    .unwrap_or(0);
            if decrement > 0 {
                let new_idx = current_idx - decrement;
                debug!(
                    agent_id = %agent.short_id(),
                    agent_title = %agent.title,
                    %current_idx,
                    %new_idx,
                    "Adjusting window index after deletion"
                );
                agent.window_index = Some(new_idx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::config::Config;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        App::new(Config::default(), Storage::default())
    }

    #[test]
    fn test_resize_agent_windows_no_dimensions() {
        let handler = Actions::new();
        let app = create_test_app();

        // Should not panic when no dimensions are set
        handler.resize_agent_windows(&app);
        assert!(app.preview_dimensions.is_none());
    }

    #[test]
    fn test_resize_agent_windows_with_dimensions() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Set preview dimensions
        app.set_preview_dimensions(100, 50);

        // Add a root agent (session won't exist, but should not error)
        app.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Should not panic when resizing non-existent sessions
        handler.resize_agent_windows(&app);
        assert_eq!(app.preview_dimensions, Some((100, 50)));
    }

    #[test]
    fn test_resize_agent_windows_with_child_agents() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Set preview dimensions
        app.set_preview_dimensions(80, 40);

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

        // Add a child agent
        app.storage.add(Agent::new_child(
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
        ));

        // Should handle both root and child agents without panicking
        handler.resize_agent_windows(&app);
    }
}
