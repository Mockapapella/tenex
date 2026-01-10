//! Slash command palette and related pickers

use crate::app::App;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `CommandPalette` mode
pub fn handle_command_palette_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_command_palette_mode(app, code)
}

/// Handle key events in `ModelSelector` mode
pub fn handle_model_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_model_selector_mode(app, code)
}

/// Handle key events in `SettingsMenu` mode
pub fn handle_settings_menu_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_settings_menu_mode(app, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::{AppMode, CommandPaletteMode, ModelSelectorMode, SettingsMenuMode};
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
        app.apply_mode(CommandPaletteMode.into());

        handle_command_palette_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_command_palette_up_down_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CommandPaletteMode.into());
        assert_eq!(app.data.command_palette.selected, 0);

        handle_command_palette_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.command_palette.selected, 1);

        handle_command_palette_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.command_palette.selected, 0);
        Ok(())
    }

    #[test]
    fn test_command_palette_char_input() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CommandPaletteMode.into());
        assert_eq!(app.data.input.buffer, "/");

        handle_command_palette_mode(&mut app, KeyCode::Char('m'))?;
        assert_eq!(app.data.input.buffer, "/m");
        assert_eq!(app.data.command_palette.selected, 0);
        Ok(())
    }

    #[test]
    fn test_command_palette_backspace_exits_on_empty() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CommandPaletteMode.into());
        assert_eq!(app.data.input.buffer, "/");

        handle_command_palette_mode(&mut app, KeyCode::Backspace)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_command_palette_enter_runs_help() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CommandPaletteMode.into());
        // Navigate to /help (index 1)
        handle_command_palette_mode(&mut app, KeyCode::Down)?;

        handle_command_palette_mode(&mut app, KeyCode::Enter)?;
        assert!(matches!(app.mode, AppMode::Help(_)));
        Ok(())
    }

    #[test]
    fn test_command_palette_cursor_movement() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(CommandPaletteMode.into());
        app.data.input.buffer = "/agents".to_string();
        app.data.input.cursor = 7;

        handle_command_palette_mode(&mut app, KeyCode::Home)?;
        assert_eq!(app.data.input.cursor, 0);

        handle_command_palette_mode(&mut app, KeyCode::End)?;
        assert_eq!(app.data.input.cursor, 7);

        handle_command_palette_mode(&mut app, KeyCode::Left)?;
        assert_eq!(app.data.input.cursor, 6);

        handle_command_palette_mode(&mut app, KeyCode::Right)?;
        assert_eq!(app.data.input.cursor, 7);
        Ok(())
    }

    // ========== ModelSelector mode tests ==========

    #[test]
    fn test_model_selector_esc_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ModelSelectorMode.into());

        handle_model_selector_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.model_selector.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_model_selector_up_down_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ModelSelectorMode.into());
        let initial = app.data.model_selector.selected;

        handle_model_selector_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.model_selector.selected, (initial + 1) % 3);

        handle_model_selector_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.model_selector.selected, initial);
        Ok(())
    }

    #[test]
    fn test_model_selector_filter_input() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ModelSelectorMode.into());

        handle_model_selector_mode(&mut app, KeyCode::Char('c'))?;
        assert_eq!(app.data.model_selector.filter, "c");

        handle_model_selector_mode(&mut app, KeyCode::Backspace)?;
        assert!(app.data.model_selector.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_model_selector_other_keys_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ModelSelectorMode.into());
        let selected = app.data.model_selector.selected;

        handle_model_selector_mode(&mut app, KeyCode::Tab)?;
        assert_eq!(app.data.model_selector.selected, selected);
        assert_eq!(app.mode, ModelSelectorMode.into());
        Ok(())
    }

    // ========== SettingsMenu mode tests ==========

    #[test]
    fn test_settings_menu_esc_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(SettingsMenuMode.into());

        handle_settings_menu_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_settings_menu_up_down_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(SettingsMenuMode.into());
        assert_eq!(app.data.settings_menu.selected, 0);

        handle_settings_menu_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.settings_menu.selected, 1);

        handle_settings_menu_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.settings_menu.selected, 0);
        Ok(())
    }
}
