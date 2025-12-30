use crate::action::ValidIn;
use crate::app::{AppData, Mode, Tab};
use crate::state::{ModeUnion, NormalMode};
use anyhow::Result;

/// Normal-mode action: switch the detail pane tab (Preview/Diff).
#[derive(Debug, Clone, Copy, Default)]
pub struct SwitchTabAction;

impl ValidIn<NormalMode> for SwitchTabAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: select the next agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct NextAgentAction;

impl ValidIn<NormalMode> for NextAgentAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.select_next();
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: select the previous agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrevAgentAction;

impl ValidIn<NormalMode> for PrevAgentAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.select_prev();
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: scroll up in the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollUpAction;

impl ValidIn<NormalMode> for ScrollUpAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: scroll down in the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollDownAction;

impl ValidIn<NormalMode> for ScrollDownAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: scroll to the top of the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollTopAction;

impl ValidIn<NormalMode> for ScrollTopAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: scroll to the bottom of the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollBottomAction;

impl ValidIn<NormalMode> for ScrollBottomAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(ModeUnion::normal())
    }
}

/// Normal-mode action: focus the preview pane (forward keys to mux).
#[derive(Debug, Clone, Copy, Default)]
pub struct FocusPreviewAction;

impl ValidIn<NormalMode> for FocusPreviewAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            app_data.active_tab = Tab::Preview;
            Ok(ModeUnion::Legacy(Mode::PreviewFocused))
        } else {
            Ok(ModeUnion::normal())
        }
    }
}
