//! Slash command palette and related pickers

use crate::app::App;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `CommandPalette` mode
pub fn handle_command_palette_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.exit_mode(),
        KeyCode::Enter => app.confirm_slash_command_selection(),
        KeyCode::Up => app.select_prev_slash_command(),
        KeyCode::Down => app.select_next_slash_command(),
        KeyCode::Char(c) => {
            app.handle_char(c);
            app.reset_slash_command_selection();
        }
        KeyCode::Backspace => {
            if app.input.buffer.trim() == "/" {
                app.exit_mode();
            } else {
                app.handle_backspace();
                app.reset_slash_command_selection();
                if app.input.buffer.trim().is_empty() {
                    app.exit_mode();
                }
            }
        }
        KeyCode::Delete => app.handle_delete(),
        KeyCode::Left => app.input.cursor_left(),
        KeyCode::Right => app.input.cursor_right(),
        KeyCode::Home => app.input.cursor_home(),
        KeyCode::End => app.input.cursor_end(),
        _ => {}
    }
}

/// Handle key events in `ModelSelector` mode
pub fn handle_model_selector_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.model_selector.clear();
            app.exit_mode();
        }
        KeyCode::Enter => app.confirm_model_program_selection(),
        KeyCode::Up => app.select_prev_model_program(),
        KeyCode::Down => app.select_next_model_program(),
        KeyCode::Char(c) => app.handle_model_filter_char(c),
        KeyCode::Backspace => app.handle_model_filter_backspace(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{Mode, OverlayMode, Settings};
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
    fn test_command_palette_esc_exits() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.mode, Mode::Overlay(OverlayMode::CommandPalette));

        handle_command_palette_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_command_palette_up_down_navigation() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.command_palette.selected, 0);

        handle_command_palette_mode(&mut app, KeyCode::Down);
        assert_eq!(app.command_palette.selected, 1);

        handle_command_palette_mode(&mut app, KeyCode::Up);
        assert_eq!(app.command_palette.selected, 0);
        Ok(())
    }

    #[test]
    fn test_command_palette_char_input() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.input.buffer, "/");

        handle_command_palette_mode(&mut app, KeyCode::Char('m'));
        assert_eq!(app.input.buffer, "/m");
        assert_eq!(app.command_palette.selected, 0);
        Ok(())
    }

    #[test]
    fn test_command_palette_backspace_exits_on_empty() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        assert_eq!(app.input.buffer, "/");

        handle_command_palette_mode(&mut app, KeyCode::Backspace);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_command_palette_enter_runs_help() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        // Navigate to /help (index 1)
        handle_command_palette_mode(&mut app, KeyCode::Down);

        handle_command_palette_mode(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
        Ok(())
    }

    #[test]
    fn test_command_palette_cursor_movement() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_command_palette();
        app.input.buffer = "/agents".to_string();
        app.input.cursor = 7;

        handle_command_palette_mode(&mut app, KeyCode::Home);
        assert_eq!(app.input.cursor, 0);

        handle_command_palette_mode(&mut app, KeyCode::End);
        assert_eq!(app.input.cursor, 7);

        handle_command_palette_mode(&mut app, KeyCode::Left);
        assert_eq!(app.input.cursor, 6);

        handle_command_palette_mode(&mut app, KeyCode::Right);
        assert_eq!(app.input.cursor, 7);
        Ok(())
    }

    // ========== ModelSelector mode tests ==========

    #[test]
    fn test_model_selector_esc_exits() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();
        assert_eq!(app.mode, Mode::Overlay(OverlayMode::ModelSelector));

        handle_model_selector_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.model_selector.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_model_selector_up_down_navigation() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();
        let initial = app.model_selector.selected;

        handle_model_selector_mode(&mut app, KeyCode::Down);
        assert_eq!(app.model_selector.selected, (initial + 1) % 3);

        handle_model_selector_mode(&mut app, KeyCode::Up);
        assert_eq!(app.model_selector.selected, initial);
        Ok(())
    }

    #[test]
    fn test_model_selector_filter_input() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();

        handle_model_selector_mode(&mut app, KeyCode::Char('c'));
        assert_eq!(app.model_selector.filter, "c");

        handle_model_selector_mode(&mut app, KeyCode::Backspace);
        assert!(app.model_selector.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_model_selector_other_keys_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.start_model_selector();
        let selected = app.model_selector.selected;

        handle_model_selector_mode(&mut app, KeyCode::Tab);
        assert_eq!(app.model_selector.selected, selected);
        assert_eq!(app.mode, Mode::Overlay(OverlayMode::ModelSelector));
        Ok(())
    }
}
