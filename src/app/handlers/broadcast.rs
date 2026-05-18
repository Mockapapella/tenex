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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::app::App;
    use crate::app::Settings;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn is_error_modal(mode: &AppMode) -> bool {
        matches!(mode, AppMode::ErrorModal(_))
    }

    #[test]
    fn test_is_error_modal_covers_true_and_false() {
        assert!(is_error_modal(
            &ErrorModalMode {
                message: "test".to_string(),
            }
            .into()
        ));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
    }

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

        // No agent selected - should return error modal mode
        let next = handler
            .broadcast_to_leaves(&mut app.data, "test message")
            .expect("broadcast should succeed");
        assert!(is_error_modal(&next));
    }

    #[test]
    fn test_broadcast_to_leaves_with_agent_no_children() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with no children
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        let next = handler
            .broadcast_to_leaves(&mut app.data, "test message")
            .expect("broadcast should succeed");
        assert!(is_error_modal(&next));
    }

    #[test]
    fn test_broadcast_to_leaves_with_agent_sends_when_session_exists() {
        let socket = format!("tenex-broadcast-leaves-{}", std::process::id());
        let _ = crate::mux::set_socket_override(&socket);

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().expect("create temp dir");
        let agent = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp_dir.path().to_path_buf(),
        );
        let mux_session = agent.mux_session.clone();
        app.data.storage.add(agent);

        handler
            .session_manager
            .create(&mux_session, temp_dir.path(), None)
            .expect("create session");
        let next =
            with_tracing_dispatch(|| handler.broadcast_to_leaves(&mut app.data, "test message"))
                .expect("broadcast should succeed");
        assert_eq!(next, AppMode::normal());
        assert_eq!(
            app.data.ui.status_message,
            Some("Broadcast sent to 1 agent(s)".to_string())
        );

        let _ = handler.session_manager.kill(&mux_session);
    }

    #[test]
    fn test_broadcast_to_leaves_uses_window_target_when_window_index_is_set() {
        let socket = format!(
            "tenex-broadcast-leaves-window-target-{}",
            std::process::id()
        );
        let _ = crate::mux::set_socket_override(&socket);

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().expect("create temp dir");
        let mut agent = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            temp_dir.path().to_path_buf(),
        );
        agent.window_index = Some(0);
        let mux_session = agent.mux_session.clone();
        app.data.storage.add(agent);

        handler
            .session_manager
            .create(&mux_session, temp_dir.path(), None)
            .expect("create session");
        let next =
            with_tracing_dispatch(|| handler.broadcast_to_leaves(&mut app.data, "test message"))
                .expect("broadcast should succeed");
        assert_eq!(next, AppMode::normal());
        assert_eq!(
            app.data.ui.status_message,
            Some("Broadcast sent to 1 agent(s)".to_string())
        );

        let _ = handler.session_manager.kill(&mux_session);
    }

    #[test]
    fn test_broadcast_to_leaves_with_children() {
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
                    repo_root: None,
                },
            ));
        }

        // Broadcast when sessions don't exist - send_keys fails, so no messages sent
        // This exercises the "No leaf agents found" path since send_keys fails
        let next = handler
            .broadcast_to_leaves(&mut app.data, "test message")
            .expect("broadcast should succeed");
        assert!(is_error_modal(&next));
    }
}
