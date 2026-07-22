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
        let sent_count = {
            let mut sent_count = 0;

            let mut targets = Vec::new();
            targets.push(agent);
            targets.extend(app_data.storage.descendants(agent_id));

            // Filter to only leaf agents (excluding terminals) and send message
            for target_agent in targets {
                if !app_data.storage.has_children(target_agent.id)
                    && !target_agent.is_terminal_agent()
                {
                    // Determine the mux target (session or window)
                    let target = target_agent.window_index.map_or_else(
                        || target_agent.mux_session.clone(),
                        |window_idx| {
                            SessionManager::window_target(&target_agent.mux_session, window_idx)
                        },
                    );

                    // Send the message and submit it (program-specific)
                    if self
                        .session_manager
                        .send_keys_and_submit_for_agent(&target, target_agent, message)
                        .is_ok()
                    {
                        sent_count += 1;
                    }
                }
            }

            sent_count
        };

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
