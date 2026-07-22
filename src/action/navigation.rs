use crate::action::ValidIn;
use crate::app::{AppData, Tab};
use crate::state::{AppMode, DiffFocusedMode, NormalMode, PreviewFocusedMode, ScrollingMode};
use anyhow::Result;

/// Normal-mode action: switch the detail pane tab (Preview/Diff).
#[derive(Debug, Clone, Copy, Default)]
pub struct SwitchTabAction;

impl ValidIn<NormalMode> for SwitchTabAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SwitchTabAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for SwitchTabAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(AppMode::normal())
    }
}

/// Normal-mode action: select the next agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct NextAgentAction;

impl ValidIn<NormalMode> for NextAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_next();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for NextAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_next();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: select the previous agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrevAgentAction;

impl ValidIn<NormalMode> for PrevAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_prev();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for PrevAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_prev();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: highlight the selected agent's project header.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectProjectHeaderAction;

impl ValidIn<NormalMode> for SelectProjectHeaderAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_project_header();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SelectProjectHeaderAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_project_header();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: highlight the first agent under the selected project.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectProjectFirstAgentAction;

impl ValidIn<NormalMode> for SelectProjectFirstAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_first_agent_in_selected_project();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SelectProjectFirstAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_first_agent_in_selected_project();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: scroll up in the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollUpAction;

impl ValidIn<NormalMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: scroll down in the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollDownAction;

impl ValidIn<NormalMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: scroll to the top of the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollTopAction;

impl ValidIn<NormalMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: scroll to the bottom of the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollBottomAction;

impl ValidIn<NormalMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: focus the preview pane (forward keys to mux).
#[derive(Debug, Clone, Copy, Default)]
pub struct FocusPreviewAction;

impl ValidIn<NormalMode> for FocusPreviewAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            match app_data.active_tab {
                Tab::Preview => Ok(PreviewFocusedMode.into()),
                Tab::Diff => Ok(DiffFocusedMode.into()),
                Tab::Commits => Ok(AppMode::normal()),
            }
        } else {
            Ok(AppMode::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for FocusPreviewAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            match app_data.active_tab {
                Tab::Preview => Ok(PreviewFocusedMode.into()),
                Tab::Diff => Ok(DiffFocusedMode.into()),
                Tab::Commits => Ok(ScrollingMode.into()),
            }
        } else {
            Ok(ScrollingMode.into())
        }
    }
}
