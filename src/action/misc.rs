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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Status, Storage};
    use std::path::PathBuf;

    fn make_agent(title: &str, status: Status) -> Agent {
        let pid = std::process::id();
        let mut agent = Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("tenex-action-misc-test-{pid}/{title}"),
            PathBuf::from(format!("/tmp/tenex-action-misc-test-{pid}/{title}")),
        );
        agent.set_status(status);
        agent
    }

    #[test]
    fn test_help_action_resets_help_scroll_in_normal() -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            crate::config::Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );
        data.ui.help_scroll = 123;

        let next = HelpAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::Help(_)));
        assert_eq!(data.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_quit_action_enters_confirming_when_running_agents()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        storage.add(make_agent("running", Status::Running));
        let mut data = AppData::new(
            crate::config::Config::default(),
            storage,
            crate::app::Settings::default(),
            false,
        );

        let next = QuitAction.execute(NormalMode, &mut data)?;
        assert_eq!(
            next,
            ConfirmingMode {
                action: ConfirmAction::Quit
            }
            .into()
        );
        assert!(!data.should_quit);
        Ok(())
    }

    #[test]
    fn test_quit_action_sets_should_quit_when_no_running_agents()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        storage.add(make_agent("starting", Status::Starting));
        let mut data = AppData::new(
            crate::config::Config::default(),
            storage,
            crate::app::Settings::default(),
            false,
        );

        let next = QuitAction.execute(NormalMode, &mut data)?;
        assert_eq!(next, AppMode::normal());
        assert!(data.should_quit);
        Ok(())
    }

    #[test]
    fn test_quit_action_scrolling_returns_scrolling_when_quitting()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut storage = Storage::new();
        storage.add(make_agent("starting", Status::Starting));
        let mut data = AppData::new(
            crate::config::Config::default(),
            storage,
            crate::app::Settings::default(),
            false,
        );

        let next = QuitAction.execute(ScrollingMode, &mut data)?;
        assert_eq!(next, ScrollingMode.into());
        assert!(data.should_quit);
        Ok(())
    }

    #[test]
    fn test_command_palette_action_enters_command_palette_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            crate::config::Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = CommandPaletteAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::CommandPalette(_)));
        Ok(())
    }

    #[test]
    fn test_cancel_action_clears_input_in_normal() -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            crate::config::Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );
        data.input.buffer = "hello".to_string();
        data.input.cursor = data.input.buffer.len();

        let next = CancelAction.execute(NormalMode, &mut data)?;
        assert_eq!(next, AppMode::normal());
        assert!(data.input.buffer.is_empty());
        assert_eq!(data.input.cursor, 0);
        Ok(())
    }

    #[test]
    fn test_cancel_action_does_not_clear_input_in_scrolling()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            crate::config::Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );
        data.input.buffer = "hello".to_string();
        data.input.cursor = data.input.buffer.len();

        let next = CancelAction.execute(ScrollingMode, &mut data)?;
        assert_eq!(next, AppMode::normal());
        assert_eq!(data.input.buffer, "hello");
        assert_eq!(data.input.cursor, 5);
        Ok(())
    }
}
