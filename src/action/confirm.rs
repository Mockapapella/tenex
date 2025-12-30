//! Confirmation-mode action types (new architecture).

use crate::action::{BackspaceAction, CancelAction, CharInputAction, SubmitAction, ValidIn};
use crate::app::{Actions, AppData, ConfirmAction};
use crate::config::Action as KeyAction;
use crate::state::{
    ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode, KeyboardRemapPromptMode, ModeUnion,
    ReconnectPromptMode, RenameBranchMode, UpdatePromptMode, UpdateRequestedMode,
};
use anyhow::Result;

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
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ConfirmingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data
            .actions
            .handle_action(app_data.app, KeyAction::Confirm)?;
        Ok(ModeUnion::Legacy(app_data.mode.clone()))
    }
}

impl ValidIn<ConfirmingMode> for ConfirmNoAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ConfirmingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.exit_mode();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ConfirmingMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if state.action == ConfirmAction::WorktreeConflict {
            app_data.spawn.worktree_conflict = None;
        }
        app_data.exit_mode();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ConfirmingMode> for WorktreeReconnectAction {
    type NextState = ModeUnion;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
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
    type NextState = ModeUnion;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if state.action != ConfirmAction::WorktreeConflict {
            return Ok(state.into());
        }

        app_data.exit_mode();
        app_data.actions.recreate_worktree(app_data.app)?;
        Ok(ModeUnion::Legacy(app_data.mode.clone()))
    }
}

impl ValidIn<ConfirmPushMode> for ConfirmYesAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: ConfirmPushMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();

        if let Err(err) = Actions::execute_push(app_data.app) {
            app_data.set_error(format!("Push failed: {err:#}"));
        }

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<ConfirmPushMode> for ConfirmNoAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ConfirmPushMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ConfirmPushMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ConfirmPushMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ConfirmPushForPRMode> for ConfirmYesAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: ConfirmPushForPRMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();

        if let Err(err) = Actions::execute_push_and_open_pr(app_data.app) {
            app_data.set_error(format!("Failed to push and open PR: {err:#}"));
        }

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<ConfirmPushForPRMode> for ConfirmNoAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ConfirmPushForPRMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ConfirmPushForPRMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ConfirmPushForPRMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<RenameBranchMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: RenameBranchMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();

        if app_data.confirm_rename_branch()
            && let Err(err) = Actions::execute_rename(app_data.app)
        {
            app_data.set_error(format!("Rename failed: {err:#}"));
        }

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<RenameBranchMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RenameBranchMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<RenameBranchMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RenameBranchMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(RenameBranchMode.into())
    }
}

impl ValidIn<RenameBranchMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RenameBranchMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(RenameBranchMode.into())
    }
}

impl ValidIn<KeyboardRemapPromptMode> for ConfirmYesAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: KeyboardRemapPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.accept_keyboard_remap();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<KeyboardRemapPromptMode> for ConfirmNoAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: KeyboardRemapPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.decline_keyboard_remap();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<KeyboardRemapPromptMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: KeyboardRemapPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.decline_keyboard_remap();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<UpdatePromptMode> for ConfirmYesAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: UpdatePromptMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(UpdateRequestedMode { info: state.info }.into())
    }
}

impl ValidIn<UpdatePromptMode> for ConfirmNoAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: UpdatePromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.exit_mode();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<UpdatePromptMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: UpdatePromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.exit_mode();
        Ok(ModeUnion::normal())
    }
}
