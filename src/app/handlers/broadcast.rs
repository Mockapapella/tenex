//! Broadcast operations: send messages to leaf agents

use crate::tmux::SessionManager;
use anyhow::Result;
use tracing::{info, warn};

use super::Actions;
use crate::app::state::App;

impl Actions {
    /// Broadcast a message to the selected agent and all its leaf descendants
    ///
    /// Leaf agents are agents that have no children. Parent agents are skipped
    /// but their children are still traversed.
    ///
    /// # Errors
    ///
    /// Returns an error if broadcasting fails
    pub fn broadcast_to_leaves(self, app: &mut App, message: &str) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let mut sent_count = 0;

        // Collect all agents to broadcast to (selected + descendants)
        let mut targets: Vec<uuid::Uuid> = vec![agent_id];
        targets.extend(app.storage.descendant_ids(agent_id));

        // Filter to only leaf agents (excluding terminals) and send message
        for target_id in targets {
            if !app.storage.has_children(target_id)
                && let Some(target_agent) = app.storage.get(target_id)
                && !target_agent.is_terminal
            {
                // Determine the tmux target (session or window)
                let tmux_target = if let Some(window_idx) = target_agent.window_index {
                    // Child agent: use window target within root's session
                    let root = app.storage.root_ancestor(target_id);
                    let root_session = root.map_or_else(
                        || target_agent.tmux_session.clone(),
                        |r| r.tmux_session.clone(),
                    );
                    SessionManager::window_target(&root_session, window_idx)
                } else {
                    // Root agent: use session directly
                    target_agent.tmux_session.clone()
                };

                // Send the message and submit it
                if self
                    .session_manager
                    .send_keys_and_submit(&tmux_target, message)
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
            app.set_status(format!("Broadcast sent to {sent_count} agent(s)"));
        } else {
            warn!(%agent_id, "No leaf agents found to broadcast to");
            app.set_error("No leaf agents found to broadcast to");
        }

        Ok(())
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
    fn test_broadcast_to_leaves_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent selected - should return error
        let result = handler.broadcast_to_leaves(&mut app, "test message");
        assert!(result.is_err());
    }

    #[test]
    fn test_broadcast_to_leaves_with_agent_no_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with no children
        app.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Broadcast should set error when no children
        handler.broadcast_to_leaves(&mut app, "test message")?;
        assert!(app.ui.last_error.is_some());
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
            None,
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add children (leaves)
        for i in 0..2 {
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

        // Broadcast when sessions don't exist - send_keys fails, so no messages sent
        // This exercises the "No leaf agents found" path since send_keys fails
        handler.broadcast_to_leaves(&mut app, "test message")?;
        // Since sessions don't exist, send_keys fails and error is set
        assert!(app.ui.last_error.is_some());
        Ok(())
    }
}
