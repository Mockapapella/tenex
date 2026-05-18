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

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    // ========== CommandPalette mode tests ==========

    #[test]
    fn test_command_palette_esc_exits() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());

        handle_command_palette_mode(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_command_palette_up_down_navigation() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());
        assert_eq!(app.data.command_palette.selected, 0);

        handle_command_palette_mode(&mut app, KeyCode::Down).unwrap();
        assert_eq!(app.data.command_palette.selected, 1);

        handle_command_palette_mode(&mut app, KeyCode::Up).unwrap();
        assert_eq!(app.data.command_palette.selected, 0);
    }

    #[test]
    fn test_command_palette_char_input() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());
        assert_eq!(app.data.input.buffer, "/");

        handle_command_palette_mode(&mut app, KeyCode::Char('m')).unwrap();
        assert_eq!(app.data.input.buffer, "/m");
        assert_eq!(app.data.command_palette.selected, 0);
    }

    #[test]
    fn test_command_palette_backspace_exits_on_empty() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());
        assert_eq!(app.data.input.buffer, "/");

        handle_command_palette_mode(&mut app, KeyCode::Backspace).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_command_palette_enter_runs_help() {
        fn is_help_mode(mode: &AppMode) -> bool {
            matches!(mode, AppMode::Help(_))
        }

        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());
        // Navigate to /help (index 3)
        handle_command_palette_mode(&mut app, KeyCode::Down).unwrap();
        handle_command_palette_mode(&mut app, KeyCode::Down).unwrap();
        handle_command_palette_mode(&mut app, KeyCode::Down).unwrap();

        handle_command_palette_mode(&mut app, KeyCode::Enter).unwrap();
        assert!(is_help_mode(&app.mode));
        assert!(!is_help_mode(&AppMode::normal()));
    }

    #[test]
    fn test_command_palette_cursor_movement() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());
        app.data.input.buffer = "/agents".to_string();
        app.data.input.cursor = 7;

        handle_command_palette_mode(&mut app, KeyCode::Home).unwrap();
        assert_eq!(app.data.input.cursor, 0);

        handle_command_palette_mode(&mut app, KeyCode::End).unwrap();
        assert_eq!(app.data.input.cursor, 7);

        handle_command_palette_mode(&mut app, KeyCode::Left).unwrap();
        assert_eq!(app.data.input.cursor, 6);

        handle_command_palette_mode(&mut app, KeyCode::Right).unwrap();
        assert_eq!(app.data.input.cursor, 7);
    }

    // ========== ModelSelector mode tests ==========

    #[test]
    fn test_model_selector_esc_exits() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ModelSelectorMode.into());

        handle_model_selector_mode(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.model_selector.filter.is_empty());
    }

    #[test]
    fn test_model_selector_up_down_navigation() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ModelSelectorMode.into());
        let initial = app.data.model_selector.selected;

        handle_model_selector_mode(&mut app, KeyCode::Down).unwrap();
        assert_eq!(app.data.model_selector.selected, (initial + 1) % 3);

        handle_model_selector_mode(&mut app, KeyCode::Up).unwrap();
        assert_eq!(app.data.model_selector.selected, initial);
    }

    #[test]
    fn test_model_selector_filter_input() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ModelSelectorMode.into());

        handle_model_selector_mode(&mut app, KeyCode::Char('c')).unwrap();
        assert_eq!(app.data.model_selector.filter, "c");

        handle_model_selector_mode(&mut app, KeyCode::Backspace).unwrap();
        assert!(app.data.model_selector.filter.is_empty());
    }

    #[test]
    fn test_model_selector_other_keys_ignored() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ModelSelectorMode.into());
        let selected = app.data.model_selector.selected;

        handle_model_selector_mode(&mut app, KeyCode::Tab).unwrap();
        assert_eq!(app.data.model_selector.selected, selected);
        assert_eq!(app.mode, ModelSelectorMode.into());
    }

    // ========== SettingsMenu mode tests ==========

    #[test]
    fn test_settings_menu_esc_exits() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(SettingsMenuMode.into());

        handle_settings_menu_mode(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_settings_menu_up_down_navigation() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(SettingsMenuMode.into());
        assert_eq!(app.data.settings_menu.selected, 0);

        handle_settings_menu_mode(&mut app, KeyCode::Down).unwrap();
        assert_eq!(app.data.settings_menu.selected, 1);

        handle_settings_menu_mode(&mut app, KeyCode::Up).unwrap();
        assert_eq!(app.data.settings_menu.selected, 0);
    }
}
