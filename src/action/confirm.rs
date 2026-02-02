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
        }

        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for ConfirmNoAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        if state.action == ConfirmAction::InterruptAgent {
            return Ok(PreviewFocusedMode.into());
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<ConfirmingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, state: ConfirmingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if state.action == ConfirmAction::WorktreeConflict {
            app_data.spawn.worktree_conflict = None;
            return Ok(AppMode::normal());
        }
        if state.action == ConfirmAction::InterruptAgent {
            return Ok(PreviewFocusedMode.into());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;
    use crate::agent::Storage;
    use crate::app::{Settings, WorktreeConflictInfo};
    use crate::config::Config;
    use std::path::PathBuf;

    fn empty_data() -> AppData {
        AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        )
    }

    fn make_conflict(prompt: Option<&str>) -> WorktreeConflictInfo {
        WorktreeConflictInfo {
            title: "conflict-title".to_string(),
            prompt: prompt.map(str::to_string),
            branch: "tenex/conflict-title".to_string(),
            worktree_path: PathBuf::from("/tmp/tenex-confirm-action-conflict"),
            existing_branch: Some("tenex/conflict-title".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        }
    }

    #[test]
    fn test_confirm_yes_quit_sets_should_quit() -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        let state = ConfirmingMode {
            action: ConfirmAction::Quit,
        };

        let next = ConfirmYesAction.execute(state, &mut data)?;
        assert_eq!(next, AppMode::normal());
        assert!(data.should_quit);
        Ok(())
    }

    #[test]
    fn test_confirm_yes_synthesize_enters_synthesis_prompt()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        let state = ConfirmingMode {
            action: ConfirmAction::Synthesize,
        };

        let next = ConfirmYesAction.execute(state, &mut data)?;
        assert_eq!(next, AppMode::SynthesisPrompt(SynthesisPromptMode));
        Ok(())
    }

    #[test]
    fn test_confirm_interrupt_agent_yes_returns_to_preview_focus()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        let state = ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        };

        let next = ConfirmYesAction.execute(state, &mut data)?;
        assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
        Ok(())
    }

    #[test]
    fn test_confirm_interrupt_agent_sends_ctrl_c_to_selected_agent_when_present()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        data.storage.add(Agent::new(
            "test-agent".to_string(),
            "bash".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        ));
        data.selected = 0;

        let state = ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        };

        let next = ConfirmYesAction.execute(state, &mut data)?;
        assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
        Ok(())
    }

    #[test]
    fn test_confirm_interrupt_agent_no_returns_to_preview_focus()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        let state = ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        };

        let next = ConfirmNoAction.execute(state, &mut data)?;
        assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
        Ok(())
    }

    #[test]
    fn test_confirm_interrupt_agent_cancel_returns_to_preview_focus()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        let state = ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        };

        let next = CancelAction.execute(state, &mut data)?;
        assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
        Ok(())
    }

    #[test]
    fn test_cancel_action_clears_worktree_conflict() -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        data.spawn.worktree_conflict = Some(make_conflict(Some("prompt")));

        let state = ConfirmingMode {
            action: ConfirmAction::WorktreeConflict,
        };
        let next = CancelAction.execute(state, &mut data)?;

        assert_eq!(next, AppMode::normal());
        assert!(data.spawn.worktree_conflict.is_none());
        Ok(())
    }

    #[test]
    fn test_worktree_reconnect_action_noop_when_not_in_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        let state = ConfirmingMode {
            action: ConfirmAction::Kill,
        };

        let next = WorktreeReconnectAction.execute(state, &mut data)?;
        assert_eq!(next, state.into());
        Ok(())
    }

    #[test]
    fn test_worktree_reconnect_action_enters_prompt_and_preloads_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        data.spawn.worktree_conflict = Some(make_conflict(Some("hello world")));

        let state = ConfirmingMode {
            action: ConfirmAction::WorktreeConflict,
        };
        let next = WorktreeReconnectAction.execute(state, &mut data)?;

        assert!(matches!(next, AppMode::ReconnectPrompt(_)));
        assert_eq!(data.input.buffer, "hello world");
        assert_eq!(data.input.cursor, data.input.buffer.len());
        Ok(())
    }

    #[test]
    fn test_submit_action_in_rename_branch_mode_noops_on_empty_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        data.input.buffer = "   ".to_string();

        let state = RenameBranchMode;
        let next = SubmitAction.execute(state, &mut data)?;
        assert_eq!(next, state.into());
        Ok(())
    }

    #[test]
    fn test_char_and_backspace_in_rename_branch_mode() -> Result<(), Box<dyn std::error::Error>> {
        let mut data = empty_data();
        data.input.buffer = String::new();
        data.input.cursor = 0;

        let state = RenameBranchMode;
        let next = CharInputAction('a').execute(state, &mut data)?;
        assert_eq!(next, state.into());
        assert_eq!(data.input.buffer, "a");
        assert_eq!(data.input.cursor, 1);

        let next = BackspaceAction.execute(state, &mut data)?;
        assert_eq!(next, state.into());
        assert!(data.input.buffer.is_empty());
        assert_eq!(data.input.cursor, 0);
        Ok(())
    }
}
