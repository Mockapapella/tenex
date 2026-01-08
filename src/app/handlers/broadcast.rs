//! Broadcast operations: send messages to leaf agents

use crate::mux::SessionManager;
use anyhow::Result;
use tracing::{info, warn};

use super::Actions;
use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode};

impl Actions {
    /// Broadcast a message to the selected agent and all its leaf descendants
    ///
    /// Leaf agents are agents that have no children. Parent agents are skipped
    /// but their children are still traversed.
    ///
    /// # Errors
    ///
    /// Returns an error if broadcasting fails
    pub fn broadcast_to_leaves(self, app_data: &mut AppData, message: &str) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected".to_string(),
            }
            .into());
        };

        let agent_id = agent.id;
        let mut sent_count = 0;

        // Collect all agents to broadcast to (selected + descendants)
        let mut targets: Vec<uuid::Uuid> = vec![agent_id];
        targets.extend(app_data.storage.descendant_ids(agent_id));

        // Filter to only leaf agents (excluding terminals) and send message
        for target_id in targets {
            if !app_data.storage.has_children(target_id)
                && let Some(target_agent) = app_data.storage.get(target_id)
                && !target_agent.is_terminal
            {
                // Determine the mux target (session or window)
                let target = if let Some(window_idx) = target_agent.window_index {
                    // Child agent: use window target within root's session
                    let root = app_data.storage.root_ancestor(target_id);
                    let root_session = root.map_or_else(
                        || target_agent.mux_session.clone(),
                        |r| r.mux_session.clone(),
                    );
                    SessionManager::window_target(&root_session, window_idx)
                } else {
                    // Root agent: use session directly
                    target_agent.mux_session.clone()
                };

                // Send the message and submit it (program-specific)
                if self
                    .session_manager
                    .send_keys_and_submit_for_program(&target, &target_agent.program, message)
                    .is_ok()
                {
                    sent_count += 1;
                }
            }
        }

        if sent_count > 0 {
            info!(
                sent_count,
                message_len = message.len(),
                "Broadcast sent to leaf agents"
            );
            app_data.set_status(format!("Broadcast sent to {sent_count} agent(s)"));
            return Ok(AppMode::normal());
        }
        warn!(%agent_id, "No leaf agents found to broadcast to");
        Ok(ErrorModalMode {
            message: "No leaf agents found to broadcast to".to_string(),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::app::App;
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
    fn test_broadcast_to_leaves_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent selected - should return error modal mode
        let next = handler.broadcast_to_leaves(&mut app.data, "test message")?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_broadcast_to_leaves_with_agent_no_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with no children
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        let next = handler.broadcast_to_leaves(&mut app.data, "test message")?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_broadcast_to_leaves_with_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent (expanded to show children)
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add children (leaves)
        for i in 0..2 {
            app.data.storage.add(Agent::new_child(
                format!("child{i}"),
                "claude".to_string(),
                "muster/root".to_string(),
                PathBuf::from("/tmp"),
                ChildConfig {
                    parent_id: root_id,
                    mux_session: root_session.clone(),
                    window_index: i + 2,
                },
            ));
        }

        // Broadcast when sessions don't exist - send_keys fails, so no messages sent
        // This exercises the "No leaf agents found" path since send_keys fails
        let next = handler.broadcast_to_leaves(&mut app.data, "test message")?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }
}
