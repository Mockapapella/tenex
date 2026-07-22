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
use crate::mux::{OutputCapture, OutputStream, SessionManager};
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
    session_manager: SessionManager,
    /// Output capture
    output_capture: OutputCapture,
    /// Raw output stream
    output_stream: OutputStream,
}

impl Actions {
    /// Create a new action handler
    #[must_use]
    pub const fn new() -> Self {
        Self {
            session_manager: SessionManager::new(),
            output_capture: OutputCapture::new(),
            output_stream: OutputStream::new(),
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
        let roots: Vec<_> = app_data
            .storage
            .root_agents()
            .into_iter()
            .cloned()
            .collect();

        for agent in roots {
            let _ = self.session_manager.kill(&agent.mux_session);

            if let Err(err) = crate::runtime::cleanup_runtime(&agent) {
                tracing::warn!(
                    session = %agent.mux_session,
                    error = %err,
                    "Failed to clean up runtime during reset"
                );
            }

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
