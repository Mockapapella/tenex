use crate::action::ValidIn;
use crate::app::{AppData, ConfirmAction, Mode};
use crate::state::{ModeUnion, NormalMode};
use anyhow::Result;

/// Normal-mode action: open the help overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct HelpAction;

impl ValidIn<NormalMode> for HelpAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.ui.help_scroll = 0;
        Ok(ModeUnion::Legacy(Mode::Help))
    }
}

/// Normal-mode action: quit the application (or enter quit confirmation).
#[derive(Debug, Clone, Copy, Default)]
pub struct QuitAction;

impl ValidIn<NormalMode> for QuitAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.has_running_agents() {
            Ok(ModeUnion::Legacy(Mode::Confirming(ConfirmAction::Quit)))
        } else {
            app_data.should_quit = true;
            Ok(ModeUnion::normal())
        }
    }
}

/// Normal-mode action: open the slash command palette.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandPaletteAction;

impl ValidIn<NormalMode> for CommandPaletteAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, _app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        Ok(ModeUnion::Legacy(Mode::CommandPalette))
    }
}

/// Normal-mode action: cancel/escape (no-op in normal; clears input state).
#[derive(Debug, Clone, Copy, Default)]
pub struct CancelAction;

impl ValidIn<NormalMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(_state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        // Match legacy behavior (Esc in Normal clears any leftover input state).
        app_data.input.clear();
        Ok(ModeUnion::normal())
    }
}
