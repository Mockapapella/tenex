//! Window operations: resize and adjust window indices

use std::collections::HashSet;

use tracing::{debug, warn};

use super::Actions;
use crate::app::{App, AppData};

impl Actions {
    /// Resize all agent mux windows to match the preview pane dimensions
    ///
    /// This ensures the terminal output renders correctly in the preview pane.
    pub fn resize_agent_windows(self, app: &mut App) -> bool {
        let Some((width, height)) = app.data.ui.preview_dimensions else {
            return true;
        };

        self.resize_agent_windows_to_dimensions(app, width, height)
    }

    pub(crate) fn resize_agent_windows_to_dimensions(
        self,
        app: &mut App,
        width: u16,
        height: u16,
    ) -> bool {
        if width == 0 || height == 0 {
            warn!(width, height, "Skipping zero-sized agent preview resize");
            app.set_status(format!(
                "Preview is too small to resize agents: {width}x{height}"
            ));
            return false;
        }

        let targets = self.resize_targets(app);
        let mut resized_all = true;
        for target in targets {
            resized_all &= self.resize_agent_window_target(app, &target, width, height);
        }
        resized_all
    }

    fn resize_targets(self, app: &App) -> Vec<String> {
        let mut targets = Vec::new();
        for agent in app.data.storage.iter() {
            if agent.is_root() {
                // Root agent: resize the session
                if self.session_manager.exists(&agent.mux_session) {
                    targets.push(agent.mux_session.clone());
                }
            } else if let Some(window_idx) = agent.window_index {
                // Child agent: resize the specific window
                let mut root_agent = agent;
                while let Some(parent_id) = root_agent.parent_id {
                    let Some(parent) = app.data.storage.get(parent_id) else {
                        break;
                    };
                    root_agent = parent;
                }
                if self.session_manager.exists(&root_agent.mux_session) {
                    let window_target = crate::mux::SessionManager::window_target(
                        &root_agent.mux_session,
                        window_idx,
                    );
                    targets.push(window_target);
                }
            }
        }
        targets
    }

    fn resize_agent_window_target(
        self,
        app: &mut App,
        target: &str,
        width: u16,
        height: u16,
    ) -> bool {
        match self.session_manager.resize_window(target, width, height) {
            Ok(()) => true,
            Err(err) => {
                warn!(
                    target,
                    width,
                    height,
                    error = %err,
                    "Failed to resize agent preview"
                );
                app.set_status(format!("Failed to resize agent preview: {err}"));
                false
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
    adjust_window_indices_after_deletions(app_data, root_id, &[deleted_agent_id], deleted_indices);
}

pub fn adjust_window_indices_after_deletions(
    app_data: &mut AppData,
    root_id: uuid::Uuid,
    deleted_agent_ids: &[uuid::Uuid],
    deleted_indices: &[u32],
) {
    if deleted_indices.is_empty() {
        return;
    }

    let mut sorted_deleted: Vec<u32> = deleted_indices.to_vec();
    sorted_deleted.sort_unstable();

    let mut deleted_ids = HashSet::new();
    for deleted_agent_id in deleted_agent_ids {
        deleted_ids.insert(*deleted_agent_id);
        deleted_ids.extend(app_data.storage.descendant_ids(*deleted_agent_id));
    }

    let descendants_to_update: Vec<uuid::Uuid> = app_data
        .storage
        .descendants(root_id)
        .iter()
        .filter(|a| !deleted_ids.contains(&a.id))
        .map(|a| a.id)
        .collect();

    adjust_window_indices_for_agent_ids(app_data, &descendants_to_update, &sorted_deleted);
}

fn adjust_window_indices_for_agent_ids(
    app_data: &mut AppData,
    agent_ids: &[uuid::Uuid],
    deleted_indices_sorted: &[u32],
) {
    // Update each remaining agent's window index
    for agent_id in agent_ids {
        let Some(agent) = app_data.storage.get_mut(*agent_id) else {
            continue;
        };
        if let Some(current_idx) = agent.window_index {
            // Count how many deleted indices are less than current index
            let decrement = u32::try_from(
                deleted_indices_sorted
                    .iter()
                    .filter(|&&d| d < current_idx)
                    .count(),
            )
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
