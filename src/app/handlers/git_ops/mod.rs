//! Git operations: Push, Rename Branch, Open PR, Rebase, Merge

mod merge;
mod open_pr;
mod push;
mod rebase;
mod rename;

use crate::agent::{Agent, AgentRuntime, ChildConfig};
use crate::mux::SessionManager;
use anyhow::Result;
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::AppMode;

use super::Actions;

impl Actions {
    /// Spawn a terminal for resolving conflicts
    fn spawn_conflict_terminal(
        app_data: &mut AppData,
        title: &str,
        startup_command: &str,
    ) -> Result<AppMode> {
        let agent_id = app_data
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID"))?;

        let root = app_data
            .storage
            .root_ancestor(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?
            .clone();
        Self::spawn_conflict_terminal_at(
            app_data,
            title,
            startup_command,
            &root.worktree_path,
            &root.branch,
            root.runtime,
        )
    }

    fn spawn_conflict_terminal_at(
        app_data: &mut AppData,
        title: &str,
        startup_command: &str,
        worktree_path: &std::path::Path,
        branch: &str,
        runtime: AgentRuntime,
    ) -> Result<AppMode> {
        let agent_id = app_data
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID"))?;
        let root = app_data
            .storage
            .root_ancestor(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;
        let root_session = root.mux_session.clone();
        let root_id = root.id;
        let repo_root = root.repo_root.clone();
        let runtime_scope = root.effective_runtime_scope().to_string();

        debug!(title, startup_command, "Creating conflict terminal");

        let window_index = app_data.storage.reserve_window_indices(root_id);

        let mut terminal = Agent::new_child(
            title.to_string(),
            "terminal".to_string(),
            branch.to_string(),
            worktree_path.to_path_buf(),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index,
                repo_root,
            },
        );
        terminal.is_terminal = true;
        terminal.runtime = runtime;
        terminal.runtime_scope = runtime_scope;

        let session_manager = SessionManager::new();
        crate::runtime::ensure_runtime_ready(&terminal, &app_data.settings)?;
        let terminal_command = crate::runtime::build_terminal_command(
            &terminal,
            Some(startup_command),
            &app_data.settings,
        );
        let actual_index = session_manager.create_window(
            &root_session,
            terminal.id,
            title,
            worktree_path,
            terminal_command.as_deref(),
        )?;

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let window_target = SessionManager::window_target(&root_session, actual_index);
            let _ = session_manager.resize_window(&window_target, width, height);
        }

        terminal.window_index = Some(actual_index);

        let window_target = SessionManager::window_target(&root_session, actual_index);
        if terminal_command.is_none() {
            session_manager.send_keys_and_submit(&window_target, startup_command)?;
        }

        app_data.storage.add(terminal);

        for agent in app_data.storage.iter_mut() {
            if agent.id == root_id {
                agent.collapsed = false;
            }
        }

        app_data.storage.save()?;

        app_data.git_op.clear();
        app_data.review.clear();

        info!(
            title,
            "Conflict terminal created - user can resolve conflicts"
        );
        app_data.set_status(format!("Opened terminal for conflict resolution: {title}"));
        Ok(AppMode::normal())
    }
}
