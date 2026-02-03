//! Window operations: resize and adjust window indices

use tracing::debug;

use super::Actions;
use crate::app::{App, AppData};

impl Actions {
    /// Resize all agent mux windows to match the preview pane dimensions
    ///
    /// This ensures the terminal output renders correctly in the preview pane.
    pub fn resize_agent_windows(&self, app: &App) {
        let Some((width, height)) = app.data.ui.preview_dimensions else {
            return;
        };

        for agent in app.data.storage.iter() {
            if agent.is_root() {
                // Root agent: resize the session
                if self.session_manager.exists(&agent.mux_session) {
                    let _ = self
                        .session_manager
                        .resize_window(&agent.mux_session, width, height);
                }
            } else if let Some(window_idx) = agent.window_index {
                // Child agent: resize the specific window
                let root = app.data.storage.root_ancestor(agent.id);
                if let Some(root_agent) = root
                    && self.session_manager.exists(&root_agent.mux_session)
                {
                    let window_target = crate::mux::SessionManager::window_target(
                        &root_agent.mux_session,
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
/// This handles the case where the mux renumbers windows and
/// window indices shift after windows are deleted. We compute the new
/// indices mathematically rather than relying on window names.
pub fn adjust_window_indices_after_deletion(
    app_data: &mut AppData,
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
    let descendants_to_update: Vec<uuid::Uuid> = app_data
        .storage
        .descendants(root_id)
        .iter()
        .filter(|a| a.id != deleted_agent_id)
        .filter(|a| {
            !app_data
                .storage
                .descendant_ids(deleted_agent_id)
                .contains(&a.id)
        })
        .map(|a| a.id)
        .collect();

    // Update each remaining agent's window index
    for agent_id in descendants_to_update {
        if let Some(agent) = app_data.storage.get_mut(agent_id)
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
    use crate::app::Settings;
    use crate::config::Config;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        )
    }

    #[test]
    fn test_resize_agent_windows_no_dimensions() {
        let handler = Actions::new();
        let app = create_test_app();

        // Should not panic when no dimensions are set
        handler.resize_agent_windows(&app);
        assert!(app.data.ui.preview_dimensions.is_none());
    }

    #[test]
    fn test_resize_agent_windows_with_dimensions() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Set preview dimensions
        app.set_preview_dimensions(100, 50);

        // Add a root agent (session won't exist, but should not error)
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Should not panic when resizing non-existent sessions
        handler.resize_agent_windows(&app);
        assert_eq!(app.data.ui.preview_dimensions, Some((100, 50)));
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
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add a child agent
        app.data.storage.add(Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        ));

        // Should handle both root and child agents without panicking
        handler.resize_agent_windows(&app);
    }

    #[test]
    fn test_adjust_window_indices_empty_deleted() {
        let mut app = create_test_app();

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        app.data.storage.add(root);

        // Call with empty deleted indices - should do nothing
        adjust_window_indices_after_deletion(&mut app.data, root_id, uuid::Uuid::new_v4(), &[]);
    }

    #[test]
    fn test_adjust_window_indices_single_deletion() {
        let mut app = create_test_app();

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add child agents with window indices
        let deleted_child = Agent::new_child(
            "deleted".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
                repo_root: None,
            },
        );
        let deleted_id = deleted_child.id;
        app.data.storage.add(deleted_child);

        let surviving_child = Agent::new_child(
            "surviving".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 4,
                repo_root: None,
            },
        );
        let surviving_id = surviving_child.id;
        app.data.storage.add(surviving_child);

        // Delete window index 2
        adjust_window_indices_after_deletion(&mut app.data, root_id, deleted_id, &[2]);

        // The surviving agent at index 4 should be decremented to 3
        let surviving = app.data.storage.get(surviving_id);
        assert!(surviving.is_some(), "Surviving agent should exist");
        if let Some(agent) = surviving {
            assert_eq!(agent.window_index, Some(3));
        }
    }

    #[test]
    fn test_adjust_window_indices_multiple_deletions() {
        let mut app = create_test_app();

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add surviving child with window index 5
        let surviving_child = Agent::new_child(
            "surviving".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 5,
                repo_root: None,
            },
        );
        let surviving_id = surviving_child.id;
        app.data.storage.add(surviving_child);

        // Delete windows at indices 2 and 3
        adjust_window_indices_after_deletion(&mut app.data, root_id, uuid::Uuid::new_v4(), &[2, 3]);

        // The surviving agent at index 5 should be decremented by 2 (two indices below it deleted)
        let surviving = app.data.storage.get(surviving_id);
        assert!(surviving.is_some(), "Surviving agent should exist");
        if let Some(agent) = surviving {
            assert_eq!(agent.window_index, Some(3));
        }
    }

    #[test]
    fn test_adjust_window_indices_no_change_needed() {
        let mut app = create_test_app();

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add child with window index 1
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 1,
                repo_root: None,
            },
        );
        let child_id = child.id;
        app.data.storage.add(child);

        // Delete window at index 5 (higher than child's index)
        adjust_window_indices_after_deletion(&mut app.data, root_id, uuid::Uuid::new_v4(), &[5]);

        // The child at index 1 should not change (deleted index was higher)
        let child_agent = app.data.storage.get(child_id);
        assert!(child_agent.is_some(), "Child agent should exist");
        if let Some(agent) = child_agent {
            assert_eq!(agent.window_index, Some(1));
        }
    }
}
