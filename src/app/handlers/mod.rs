//! Action handlers for the application
//!
//! This module contains the `Actions` struct and all action handlers,
//! organized into focused submodules by domain.

mod agent_lifecycle;
mod broadcast;
mod git_ops;
mod preview;
mod swarm;
mod sync;
mod window;

use crate::config::Action;
use crate::git::{self, WorktreeManager};
use crate::mux::{OutputCapture, SessionManager};
use crate::state::{
    AppMode, ConfirmPushForPRMode, ConfirmPushMode, PreviewFocusedMode, RenameBranchMode,
};
use anyhow::Result;

use super::{App, AppData};
use crate::action::{CancelAction, ConfirmYesAction, SubmitAction, UnfocusPreviewAction, ValidIn};

/// Handler for application actions
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    /// Mux session manager
    pub(crate) session_manager: SessionManager,
    /// Output capture
    pub(crate) output_capture: OutputCapture,
}

impl Actions {
    /// Create a new action handler
    #[must_use]
    pub const fn new() -> Self {
        Self {
            session_manager: SessionManager::new(),
            output_capture: OutputCapture::new(),
        }
    }

    /// Handle a keybinding action
    ///
    /// # Errors
    ///
    /// Returns an error if the action fails
    pub fn handle_action(self, app: &mut App, action: Action) -> Result<()> {
        match (&app.mode, action) {
            (AppMode::Normal(_), action) => crate::action::dispatch_normal_mode(app, action)?,
            (AppMode::Scrolling(_), action) => {
                crate::action::dispatch_scrolling_mode(app, action)?;
            }
            (AppMode::Confirming(state), Action::Confirm) => {
                let next = ConfirmYesAction.execute(*state, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::Confirming(state), Action::Cancel) => {
                let next = CancelAction.execute(*state, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::ConfirmPush(_), Action::Confirm) => {
                let next = ConfirmYesAction.execute(ConfirmPushMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::ConfirmPush(_), Action::Cancel) => {
                let next = CancelAction.execute(ConfirmPushMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::ConfirmPushForPR(_), Action::Confirm) => {
                let next = ConfirmYesAction.execute(ConfirmPushForPRMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::ConfirmPushForPR(_), Action::Cancel) => {
                let next = CancelAction.execute(ConfirmPushForPRMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::RenameBranch(_), Action::Confirm) => {
                let next = SubmitAction.execute(RenameBranchMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::RenameBranch(_), Action::Cancel) => {
                let next = CancelAction.execute(RenameBranchMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (AppMode::PreviewFocused(_), Action::UnfocusPreview) => {
                let next = UnfocusPreviewAction.execute(PreviewFocusedMode, &mut app.data)?;
                app.apply_mode(next);
            }
            (_, Action::Cancel) => {
                app.exit_mode();
            }
            _ => {}
        }
        Ok(())
    }

    /// Reset all agents and state
    pub(crate) fn reset_all(self, app_data: &mut AppData) -> Result<()> {
        for agent in app_data.storage.iter() {
            let _ = self.session_manager.kill(&agent.mux_session);

            if !agent.is_git_workspace() {
                continue;
            }

            let repo_path = agent
                .repo_root
                .clone()
                .or_else(|| std::env::current_dir().ok());
            let Some(repo_path) = repo_path else {
                continue;
            };

            let Ok(repo) = git::open_repository(&repo_path) else {
                continue;
            };

            let worktree_mgr = WorktreeManager::new(&repo);
            let delete_branch = agent.branch.starts_with(&app_data.config.branch_prefix)
                || agent.branch.starts_with("tenex/");
            let _ = if delete_branch {
                worktree_mgr.remove(&agent.branch)
            } else {
                worktree_mgr.remove_worktree_only(&agent.branch)
            };
        }

        app_data.storage.clear();
        app_data.storage.save()?;
        app_data.validate_selection();

        app_data.set_status("All agents reset");
        Ok(())
    }
}

impl Default for Actions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Status, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::*;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    #[test]
    fn test_handler_new() {
        let handler = Actions::new();
        assert!(!format!("{:?}", handler.session_manager).is_empty());
    }

    #[test]
    fn test_handler_default() {
        let handler = Actions::default();
        assert!(!format!("{:?}", handler.output_capture).is_empty());
    }

    #[test]
    fn test_handle_action_new_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::NewAgent)?;
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        Ok(())
    }

    #[test]
    fn test_handle_action_new_agent_with_prompt() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::NewAgentWithPrompt)?;
        assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
        Ok(())
    }

    #[test]
    fn test_handle_action_help() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Help)?;
        assert_eq!(app.mode, AppMode::Help(HelpMode));
        Ok(())
    }

    #[test]
    fn test_handle_action_quit_no_agents() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Quit)?;
        assert!(app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_action_switch_tab() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::SwitchTab)?;
        assert_eq!(app.data.active_tab, super::super::state::Tab::Diff);
        Ok(())
    }

    #[test]
    fn test_handle_action_navigation() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        for i in 0..3 {
            app.data.storage.add(Agent::new(
                format!("agent{i}"),
                "claude".to_string(),
                format!("muster/agent{i}"),
                PathBuf::from("/tmp"),
            ));
        }

        assert_eq!(app.data.selected, 1);
        handler.handle_action(&mut app, Action::NextAgent)?;
        assert_eq!(app.data.selected, 2);
        handler.handle_action(&mut app, Action::PrevAgent)?;
        assert_eq!(app.data.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_action_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::ScrollDown)?;
        assert_eq!(app.data.ui.preview_scroll, 5);

        handler.handle_action(&mut app, Action::ScrollUp)?;
        assert_eq!(app.data.ui.preview_scroll, 0);

        handler.handle_action(&mut app, Action::ScrollTop)?;
        assert_eq!(app.data.ui.preview_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_action_cancel() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.enter_mode(CreatingMode.into());
        handler.handle_action(&mut app, Action::Cancel)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_kill_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Kill)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_focus_preview_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // FocusPreview does nothing when no agent is selected (stays in Normal mode)
        let result = handler.handle_action(&mut app, Action::FocusPreview);
        assert!(result.is_ok());
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_quit_with_running_agents() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add a running agent
        let mut agent = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.set_status(Status::Running);
        app.data.storage.add(agent);

        // Quit should enter confirming mode
        handler.handle_action(&mut app, Action::Quit)?;
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Quit,
            })
        );
        Ok(())
    }

    #[test]
    fn test_handle_kill_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Kill should enter confirming mode
        handler.handle_action(&mut app, Action::Kill)?;
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Kill,
            })
        );
        Ok(())
    }

    #[test]
    fn test_handle_confirm_quit() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Enter confirming mode for quit
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        handler.handle_action(&mut app, Action::Confirm)?;
        assert!(app.data.should_quit);
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_confirm_reset() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add agents
        for i in 0..3 {
            app.data.storage.add(Agent::new(
                format!("agent{i}"),
                "claude".to_string(),
                format!("muster/agent{i}"),
                PathBuf::from("/tmp"),
            ));
        }

        // Enter confirming mode for reset
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Reset,
            }
            .into(),
        );

        handler.handle_action(&mut app, Action::Confirm)?;
        assert_eq!(app.data.storage.len(), 0);
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_focus_preview_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "test-session".to_string(),
            PathBuf::from("/tmp"),
        ));

        // FocusPreview should enter PreviewFocused mode
        let result = handler.handle_action(&mut app, Action::FocusPreview);
        assert!(result.is_ok());
        assert_eq!(app.mode, AppMode::PreviewFocused(PreviewFocusedMode));

        // UnfocusPreview should exit to Normal mode
        let result = handler.handle_action(&mut app, Action::UnfocusPreview);
        assert!(result.is_ok());
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_toggle_collapse_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Should not error with no agent selected
        handler.handle_action(&mut app, Action::ToggleCollapse)?;
        Ok(())
    }

    #[test]
    fn test_toggle_collapse_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Should not error when agent has no children
        handler.handle_action(&mut app, Action::ToggleCollapse)?;
        Ok(())
    }

    #[test]
    fn test_handle_action_spawn_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::SpawnChildren)?;
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        assert!(app.data.spawn.spawning_under.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_action_add_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        handler.handle_action(&mut app, Action::AddChildren)?;
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        assert_eq!(app.data.spawn.spawning_under, Some(agent_id));
        Ok(())
    }

    #[test]
    fn test_handle_action_add_children_terminal() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::{Agent, ChildConfig};
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        let mut terminal = Agent::new_child(
            "Terminal 1".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        app.data.selected = 2;

        handler.handle_action(&mut app, Action::AddChildren)?;
        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.spawn.spawning_under.is_none());
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Cannot spawn children under a terminal")
        );
        Ok(())
    }

    #[test]
    fn test_handle_action_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No agent - should not enter confirming mode
        handler.handle_action(&mut app, Action::Synthesize)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_action_synthesize_with_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add parent agent
        let parent = Agent::new(
            "parent".to_string(),
            "claude".to_string(),
            "tenex/parent".to_string(),
            PathBuf::from("/tmp"),
        );
        let parent_id = parent.id;
        app.data.storage.add(parent);

        // Add child agent
        let mut child = Agent::new(
            "child".to_string(),
            "claude".to_string(),
            "tenex/child".to_string(),
            PathBuf::from("/tmp"),
        );
        child.parent_id = Some(parent_id);
        app.data.storage.add(child);

        // With children - should enter confirming mode
        handler.handle_action(&mut app, Action::Synthesize)?;
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Synthesize,
            })
        );
        Ok(())
    }

    #[test]
    fn test_handle_action_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add agent with no children
        app.data.storage.add(Agent::new(
            "parent".to_string(),
            "claude".to_string(),
            "tenex/parent".to_string(),
            PathBuf::from("/tmp"),
        ));

        // No children - should show error modal, not enter confirming mode
        handler.handle_action(&mut app, Action::Synthesize)?;
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_action_toggle_collapse() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No agent - should not error
        handler.handle_action(&mut app, Action::ToggleCollapse)?;
        Ok(())
    }

    #[test]
    fn test_handle_action_broadcast_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No agent - should not enter mode
        handler.handle_action(&mut app, Action::Broadcast)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_action_broadcast_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.handle_action(&mut app, Action::Broadcast)?;
        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
        Ok(())
    }

    #[test]
    fn test_handle_scroll_bottom() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::ScrollBottom)?;
        // ScrollBottom calls scroll_to_bottom(10000, 0) so preview_scroll becomes 10000
        assert_eq!(app.data.ui.preview_scroll, 10000);
        Ok(())
    }

    #[test]
    fn test_handle_action_review_swarm_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No agent - should show ReviewInfo
        handler.handle_action(&mut app, Action::ReviewSwarm)?;
        assert_eq!(app.mode, AppMode::ReviewInfo(ReviewInfoMode));
        Ok(())
    }

    #[test]
    fn test_handle_action_review_swarm_terminal() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        handler.handle_action(&mut app, Action::ReviewSwarm)?;
        assert_eq!(app.mode, AppMode::normal());
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Select a non-terminal agent for review swarm")
        );
        Ok(())
    }

    #[test]
    fn test_review_state_cleared() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Set up some review state
        app.data.review.branches = vec![crate::git::BranchInfo {
            name: "test".to_string(),
            full_name: "refs/heads/test".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        }];
        app.data.review.filter = "filter".to_string();
        app.data.review.selected = 1;

        // Clear the state
        app.clear_review_state();

        assert!(app.data.review.branches.is_empty());
        assert!(app.data.review.filter.is_empty());
        assert_eq!(app.data.review.selected, 0);
        assert!(app.data.review.base_branch.is_none());
        Ok(())
    }

    #[test]
    fn test_review_info_mode_exit() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Enter ReviewInfo mode
        app.show_review_info();
        assert_eq!(app.mode, AppMode::ReviewInfo(ReviewInfoMode));

        // Cancel should exit
        handler.handle_action(&mut app, Action::Cancel)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_git_op_state_cleared_properly() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;

        // Set up git op state
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test-branch".to_string();
        app.data.git_op.original_branch = "original".to_string();
        app.data.git_op.base_branch = "main".to_string();
        app.data.git_op.has_unpushed = true;
        app.data.git_op.is_root_rename = true;

        // Clear the state
        app.clear_git_op_state();

        // Verify all fields are cleared
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.original_branch.is_empty());
        assert!(app.data.git_op.base_branch.is_empty());
        assert!(!app.data.git_op.has_unpushed);
        assert!(!app.data.git_op.is_root_rename);
        Ok(())
    }

    #[test]
    fn test_worktree_conflict_info_struct() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::WorktreeConflictInfo;

        let (mut app, _temp) = create_test_app()?;

        // Set up conflict info manually
        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "test".to_string(),
            prompt: Some("test prompt".to_string()),
            branch: "tenex/test".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/test"),
            repo_root: std::path::PathBuf::from("/tmp"),
            existing_branch: Some("tenex/test".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        });

        // Verify the conflict info is set
        assert!(
            app.data.spawn.worktree_conflict.is_some(),
            "Expected worktree_conflict to be set"
        );
        let info = app
            .data
            .spawn
            .worktree_conflict
            .as_ref()
            .ok_or("conflict info not set")?;
        assert_eq!(info.title, "test");
        assert_eq!(info.swarm_child_count, None);
        Ok(())
    }

    #[test]
    fn test_worktree_conflict_info_swarm() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::WorktreeConflictInfo;

        let (mut app, _temp) = create_test_app()?;

        // Set up conflict info for a swarm
        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "swarm".to_string(),
            prompt: Some("swarm task".to_string()),
            branch: "tenex/swarm".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/swarm"),
            repo_root: std::path::PathBuf::from("/tmp"),
            existing_branch: Some("tenex/swarm".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: Some(3),
        });

        assert!(
            app.data.spawn.worktree_conflict.is_some(),
            "Expected worktree_conflict to be set"
        );
        let info = app
            .data
            .spawn
            .worktree_conflict
            .as_ref()
            .ok_or("conflict info not set")?;
        assert_eq!(info.swarm_child_count, Some(3));
        Ok(())
    }

    // === Terminal Spawning Tests ===

    #[test]
    fn test_spawn_terminal_requires_selected_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No agent selected - SpawnTerminal should do nothing
        handler.handle_action(&mut app, Action::SpawnTerminal)?;
        assert_eq!(app.data.storage.len(), 0);
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_spawn_terminal_prompted_requires_selected_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No agent selected - SpawnTerminalPrompted should not enter mode
        handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_spawn_terminal_prompted_enters_mode_with_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        // With agent selected - should enter TerminalPrompt mode
        handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::TerminalPrompt(TerminalPromptMode));
        Ok(())
    }

    #[test]
    fn test_spawn_terminal_increments_counter() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let (mut app, _temp) = create_test_app()?;

        // Add an agent
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Counter starts at 0
        assert_eq!(app.data.spawn.terminal_counter, 0);

        // Get first terminal name
        let name1 = app.next_terminal_name();
        assert_eq!(name1, "Terminal 1");
        assert_eq!(app.data.spawn.terminal_counter, 1);

        // Get second terminal name
        let name2 = app.next_terminal_name();
        assert_eq!(name2, "Terminal 2");
        assert_eq!(app.data.spawn.terminal_counter, 2);
        Ok(())
    }

    #[test]
    fn test_terminal_is_marked_as_terminal() {
        use crate::agent::{Agent, ChildConfig};
        use std::path::PathBuf;

        // Create a terminal child
        let mut terminal = Agent::new_child(
            "Terminal 1".to_string(),
            "terminal".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: uuid::Uuid::new_v4(),
                mux_session: "test-session".to_string(),
                window_index: 2,
                repo_root: None,
            },
        );
        terminal.is_terminal = true;

        assert!(terminal.is_terminal);
        assert_eq!(terminal.program, "terminal");
    }

    #[test]
    fn test_terminal_spawning_flow_end_to_end() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // 1. Without agent - [t] does nothing
        handler.handle_action(&mut app, Action::SpawnTerminal)?;
        assert_eq!(app.data.storage.len(), 0);

        // 2. Without agent - [T] does nothing
        handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::normal());

        // 3. Add an agent
        app.data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
        ));

        // 4. With agent - [T] enters prompt mode
        handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::TerminalPrompt(TerminalPromptMode));

        // 5. Cancel and verify we're back to normal
        app.exit_mode();
        assert_eq!(app.mode, AppMode::normal());

        Ok(())
    }

    // === New Handler Helper Function Tests ===

    #[test]
    fn test_handle_action_unfocus_preview() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::PreviewFocused(PreviewFocusedMode);

        handler.handle_action(&mut app, Action::UnfocusPreview)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_action_unfocus_preview_not_in_preview() -> Result<(), Box<dyn std::error::Error>>
    {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::normal();

        // Should not change mode if not in PreviewFocused
        handler.handle_action(&mut app, Action::UnfocusPreview)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_kill_action_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Kill)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_kill_action_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.handle_action(&mut app, Action::Kill)?;
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Kill,
            })
        );
        Ok(())
    }

    #[test]
    fn test_handle_quit_action_no_running_agents() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Quit)?;
        assert!(app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_quit_action_with_running_agents() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        let mut agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.status = Status::Running;
        app.data.storage.add(agent);

        handler.handle_action(&mut app, Action::Quit)?;
        assert!(!app.data.should_quit);
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Quit,
            })
        );
        Ok(())
    }

    #[test]
    fn test_handle_add_children_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::AddChildren)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Synthesize)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.handle_action(&mut app, Action::Synthesize)?;
        // Should show error, not enter mode
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_broadcast_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::Broadcast)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_broadcast_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.handle_action(&mut app, Action::Broadcast)?;
        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
        Ok(())
    }

    #[test]
    fn test_handle_spawn_terminal_prompted_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_spawn_terminal_prompted_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::TerminalPrompt(TerminalPromptMode));
        Ok(())
    }
}
