//! Slash command palette and related pickers

use crate::app::{Actions, App};
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `CommandPalette` mode
pub fn handle_command_palette_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_command_palette_mode(app, action_handler, code)
}

/// Handle key events in `ModelSelector` mode
pub fn handle_model_selector_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_model_selector_mode(app, action_handler, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{Mode, Settings};
    use crate::config::Config;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    // ========== CommandPalette mode tests ==========

    #[test]
    fn test_command_palette_esc_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.mode, Mode::CommandPalette);

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_command_palette_up_down_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.command_palette.selected, 0);

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.command_palette.selected, 1);

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.command_palette.selected, 0);
        Ok(())
    }

    #[test]
    fn test_command_palette_char_input() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.input.buffer, "/");

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Char('m'))?;
        assert_eq!(app.input.buffer, "/m");
        assert_eq!(app.command_palette.selected, 0);
        Ok(())
    }

    #[test]
    fn test_command_palette_backspace_exits_on_empty() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.input.buffer, "/");

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Backspace)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_command_palette_enter_runs_help() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        // Navigate to /help (index 1)
        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Down)?;

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Enter)?;
        assert_eq!(app.mode, Mode::Help);
        Ok(())
    }

    #[test]
    fn test_command_palette_cursor_movement() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        app.input.buffer = "/agents".to_string();
        app.input.cursor = 7;

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Home)?;
        assert_eq!(app.input.cursor, 0);

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::End)?;
        assert_eq!(app.input.cursor, 7);

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Left)?;
        assert_eq!(app.input.cursor, 6);

        handle_command_palette_mode(&mut app, Actions::new(), KeyCode::Right)?;
        assert_eq!(app.input.cursor, 7);
        Ok(())
    }

    // ========== ModelSelector mode tests ==========

    #[test]
    fn test_model_selector_esc_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();
        assert_eq!(app.mode, Mode::ModelSelector);

        handle_model_selector_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.model_selector.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_model_selector_up_down_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();
        let initial = app.model_selector.selected;

        handle_model_selector_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.model_selector.selected, (initial + 1) % 3);

        handle_model_selector_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.model_selector.selected, initial);
        Ok(())
    }

    #[test]
    fn test_model_selector_filter_input() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();

        handle_model_selector_mode(&mut app, Actions::new(), KeyCode::Char('c'))?;
        assert_eq!(app.model_selector.filter, "c");

        handle_model_selector_mode(&mut app, Actions::new(), KeyCode::Backspace)?;
        assert!(app.model_selector.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_model_selector_other_keys_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();
        let selected = app.model_selector.selected;

        handle_model_selector_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.model_selector.selected, selected);
        assert_eq!(app.mode, Mode::ModelSelector);
        Ok(())
    }
}
