//! Confirmation-mode action types (new architecture).

use crate::action::{BackspaceAction, CancelAction, CharInputAction, SubmitAction, ValidIn};
use crate::app::{Actions, AppData};
use crate::state::{
    AppMode, ConfirmAction, ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode, ErrorModalMode,
    KeyboardRemapPromptMode, PreviewFocusedMode, ReconnectPromptMode, RenameBranchMode,
    SynthesisPromptMode, UpdatePromptMode, UpdateRequestedMode,
};
use anyhow::Result;
use tracing::warn;

#[cfg(test)]
thread_local! {
    static TEST_FORCE_CONFIRM_ACTION_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
/// Run `f` with confirmation actions forced to return an error.
///
/// This is test-only scaffolding used to assert dispatch error propagation without
/// relying on external state.
pub fn with_forced_confirm_action_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    TEST_FORCE_CONFIRM_ACTION_ERROR.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
pub(super) fn force_confirm_action_error_if_enabled_for_tests() -> Result<()> {
    if TEST_FORCE_CONFIRM_ACTION_ERROR.with(std::cell::Cell::get) {
        anyhow::bail!("forced confirm action error for test");
    }
    Ok(())
}

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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
        match state.action {
            ConfirmAction::Kill => {
                Actions::new().kill_agent(app_data)?;
            }
            ConfirmAction::InterruptAgent => {
                if let Some(agent) = app_data.selected_agent()
                    && !agent.is_terminal_agent()
                {
                    let target = agent.window_index.map_or_else(
                        || agent.mux_session.clone(),
                        |idx| format!("{}:{}", agent.mux_session, idx),
                    );
                    let keys = [String::from("\u{3}")];
                    if let Err(err) =
                        crate::mux::SessionManager::new().send_keys_batch(&target, &keys)
                    {
                        warn!("Failed to send Ctrl+C to {target}: {err:#}");
                    }
                }

                return Ok(PreviewFocusedMode.into());
            }
            ConfirmAction::Reset => {
                Actions::new().reset_all(app_data)?;
            }
            ConfirmAction::RestartMuxDaemon => {
                if let Err(err) = Actions::new().restart_mux_daemon(app_data) {
                    return Ok(ErrorModalMode {
                        message: format!("Failed to restart mux daemon: {err}"),
                    }
                    .into());
                }
            }
            ConfirmAction::Quit => {
                app_data.should_quit = true;
            }
            ConfirmAction::Synthesize => {
                return Ok(SynthesisPromptMode.into());
            }
            ConfirmAction::WorktreeConflict => {}
            ConfirmAction::SwitchBranch => {
                return Actions::new().switch_branch(app_data);
            }
        }

        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if state.action == ConfirmAction::InterruptAgent {
            return Ok(PreviewFocusedMode.into());
        }
        if state.action == ConfirmAction::SwitchBranch {
            app_data.git_op.clear();
            app_data.review.clear();
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
        if state.action == ConfirmAction::WorktreeConflict {
            app_data.spawn.worktree_conflict = None;
            return Ok(AppMode::normal());
        }
        if state.action == ConfirmAction::InterruptAgent {
            return Ok(PreviewFocusedMode.into());
        }
        if state.action == ConfirmAction::SwitchBranch {
            app_data.git_op.clear();
            app_data.review.clear();
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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
        app_data.git_op.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<RenameBranchMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, state: RenameBranchMode, app_data: &mut AppData) -> Result<Self::NextState> {
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
        let new_name = app_data.input.buffer.trim().to_string();
        if new_name.is_empty() {
            return Ok(state.into());
        }

        app_data.git_op.set_branch_name(new_name);
        Actions::execute_rename(app_data)
    }
}

impl ValidIn<RenameBranchMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: RenameBranchMode, app_data: &mut AppData) -> Result<Self::NextState> {
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
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
        #[cfg(test)]
        force_confirm_action_error_if_enabled_for_tests()?;
        Ok(AppMode::normal())
    }
}

#[cfg(test)]
mod tests;
