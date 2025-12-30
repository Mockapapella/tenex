use crate::action::ValidIn;
use crate::app::AppData;
use crate::state::{
    AppMode, CommandPaletteMode, ConfirmAction, ConfirmingMode, HelpMode, NormalMode, ScrollingMode,
};
use anyhow::Result;

/// Normal-mode action: open the help overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct HelpAction;

impl ValidIn<NormalMode> for HelpAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.ui.help_scroll = 0;
        Ok(HelpMode.into())
    }
}

impl ValidIn<ScrollingMode> for HelpAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.ui.help_scroll = 0;
        Ok(HelpMode.into())
    }
}

/// Normal-mode action: quit the application (or enter quit confirmation).
#[derive(Debug, Clone, Copy, Default)]
pub struct QuitAction;

impl ValidIn<NormalMode> for QuitAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.has_running_agents() {
            Ok(ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into())
        } else {
            app_data.should_quit = true;
            Ok(AppMode::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for QuitAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.has_running_agents() {
            Ok(ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into())
        } else {
            app_data.should_quit = true;
            Ok(ScrollingMode.into())
        }
    }
}

/// Normal-mode action: open the slash command palette.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandPaletteAction;

impl ValidIn<NormalMode> for CommandPaletteAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<ScrollingMode> for CommandPaletteAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(CommandPaletteMode.into())
    }
}

/// Normal-mode action: cancel/escape (no-op in normal; clears input state).
#[derive(Debug, Clone, Copy, Default)]
pub struct CancelAction;

impl ValidIn<NormalMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        // Match legacy behavior (Esc in Normal clears any leftover input state).
        app_data.input.clear();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}
