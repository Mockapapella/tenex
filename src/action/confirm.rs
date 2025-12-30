//! Confirmation-mode action types (new architecture).

use crate::action::{BackspaceAction, CancelAction, CharInputAction, SubmitAction, ValidIn};
use crate::app::{Actions, AppData};
use crate::state::{
    AppMode, ConfirmAction, ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode, ErrorModalMode,
    KeyboardRemapPromptMode, ReconnectPromptMode, RenameBranchMode, UpdatePromptMode,
    UpdateRequestedMode,
};
use anyhow::Result;
use tracing::warn;

/// Confirmation action: accept/confirm (Y/y).
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfirmYesAction;

/// Confirmation action: reject/decline (N/n).
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfirmNoAction;

/// Worktree conflict action: reconnect to existing worktree (R/r).
#[derive(Debug, Clone, Copy, Default)]
pub struct WorktreeReconnectAction;

/// Worktree conflict action: recreate worktree (D/d).
#[derive(Debug, Clone, Copy, Default)]
pub struct WorktreeRecreateAction;

impl ValidIn<ConfirmingMode> for ConfirmYesAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        match state.action {
            ConfirmAction::Kill => {
                Actions::new().kill_agent(app_data)?;
            }
            ConfirmAction::Reset => {
                Actions::new().reset_all(app_data)?;
            }
            ConfirmAction::Quit => {
                app_data.should_quit = true;
            }
            ConfirmAction::Synthesize => {
                return Actions::new().synthesize(app_data);
            }
            ConfirmAction::WorktreeConflict => {}
        }

        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(self, _state: ConfirmingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if state.action == ConfirmAction::WorktreeConflict {
            app_data.spawn.worktree_conflict = None;
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for WorktreeReconnectAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if state.action != ConfirmAction::WorktreeConflict {
            return Ok(state.into());
        }

        if let Some(conflict) = app_data.spawn.worktree_conflict.as_ref() {
            app_data.input.buffer = conflict.prompt.clone().unwrap_or_default();
            app_data.input.cursor = app_data.input.buffer.len();
        }

        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<ConfirmingMode> for WorktreeRecreateAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if state.action != ConfirmAction::WorktreeConflict {
            return Ok(state.into());
        }

        Actions::new().recreate_worktree(app_data)
    }
}

impl ValidIn<ConfirmPushMode> for ConfirmYesAction {
    type NextState = AppMode;

    fn execute(self, _state: ConfirmPushMode, app_data: &mut AppData) -> Result<Self::NextState> {
        Actions::execute_push(app_data)
    }
}

impl ValidIn<ConfirmPushMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(self, _state: ConfirmPushMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.git_op.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmPushMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: ConfirmPushMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.git_op.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmPushForPRMode> for ConfirmYesAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ConfirmPushForPRMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        Actions::execute_push_and_open_pr(app_data)
    }
}

impl ValidIn<ConfirmPushForPRMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ConfirmPushForPRMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.git_op.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmPushForPRMode> for CancelAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ConfirmPushForPRMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.git_op.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<RenameBranchMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, state: RenameBranchMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let new_name = app_data.input.buffer.trim().to_string();
        if new_name.is_empty() {
            return Ok(state.into());
        }

        app_data.git_op.set_branch_name(new_name);
        Actions::execute_rename(app_data).or_else(|err| {
            Ok(ErrorModalMode {
                message: format!("Rename failed: {err:#}"),
            }
            .into())
        })
    }
}

impl ValidIn<RenameBranchMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: RenameBranchMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.git_op.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<RenameBranchMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(self, _state: RenameBranchMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.insert_char(self.0);
        Ok(RenameBranchMode.into())
    }
}

impl ValidIn<RenameBranchMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(self, _state: RenameBranchMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.backspace();
        Ok(RenameBranchMode.into())
    }
}

impl ValidIn<KeyboardRemapPromptMode> for ConfirmYesAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: KeyboardRemapPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        if let Err(e) = app_data.settings.enable_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<KeyboardRemapPromptMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: KeyboardRemapPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        if let Err(e) = app_data.settings.decline_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<KeyboardRemapPromptMode> for CancelAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: KeyboardRemapPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        if let Err(e) = app_data.settings.decline_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<UpdatePromptMode> for ConfirmYesAction {
    type NextState = AppMode;

    fn execute(self, state: UpdatePromptMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(UpdateRequestedMode { info: state.info }.into())
    }
}

impl ValidIn<UpdatePromptMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(self, _state: UpdatePromptMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<UpdatePromptMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: UpdatePromptMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}
