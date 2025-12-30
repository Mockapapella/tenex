//! Text input mode key handling
//!
//! Handles key events for modes that involve text input:
//! - `Creating` (new agent name)
//! - `Prompting` (new agent with prompt)
//! - `ChildPrompt` (task for children)
//! - `Broadcasting` (message to leaves)
//! - `ReconnectPrompt` (reconnect with edited prompt)
//! - `TerminalPrompt` (terminal startup command)

use crate::app::{Actions, App, Mode};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Handle key events in text input modes
pub fn handle_text_input_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    match app.mode {
        Mode::Creating => {
            crate::action::dispatch_creating_mode(app, action_handler, code, modifiers)?;
        }
        Mode::Prompting => {
            crate::action::dispatch_prompting_mode(app, action_handler, code, modifiers)?;
        }
        Mode::ChildPrompt => {
            crate::action::dispatch_child_prompt_mode(app, action_handler, code, modifiers)?;
        }
        Mode::Broadcasting => {
            crate::action::dispatch_broadcasting_mode(app, action_handler, code, modifiers)?;
        }
        Mode::ReconnectPrompt => {
            crate::action::dispatch_reconnect_prompt_mode(app, action_handler, code, modifiers)?;
        }
        Mode::TerminalPrompt => {
            crate::action::dispatch_terminal_prompt_mode(app, action_handler, code, modifiers)?;
        }
        Mode::CustomAgentCommand => {
            crate::action::dispatch_custom_agent_command_mode(
                app,
                action_handler,
                code,
                modifiers,
            )?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    #[test]
    fn test_handle_text_input_char() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.input.buffer, "a");

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.input.buffer, "ab");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.input.buffer, "tes");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_delete() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();
        app.input.cursor = 0;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            KeyCode::Delete,
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.input.buffer, "est");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_cursor_movement() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();
        app.input.cursor = 2;

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Left, KeyModifiers::NONE)?;
        assert_eq!(app.input.cursor, 1);

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Right, KeyModifiers::NONE)?;
        assert_eq!(app.input.cursor, 2);

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Home, KeyModifiers::NONE)?;
        assert_eq!(app.input.cursor, 0);

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::End, KeyModifiers::NONE)?;
        assert_eq!(app.input.cursor, 4);

        Ok(())
    }

    #[test]
    fn test_handle_text_input_escape() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_escape_reconnect_prompt_clears_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReconnectPrompt;
        app.spawn.worktree_conflict = Some(crate::app::WorktreeConflictInfo {
            title: "test".to_string(),
            branch: "test".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp"),
            prompt: None,
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "abc1234".to_string(),
            swarm_child_count: None,
        });

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Esc, KeyModifiers::NONE)?;

        assert!(app.spawn.worktree_conflict.is_none());
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_enter_empty_creating_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = String::new();

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_alt_enter_inserts_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Enter, KeyModifiers::ALT)?;
        assert!(app.input.buffer.contains('\n'));

        Ok(())
    }

    #[test]
    fn test_text_input_actions_covered_for_all_modes() -> Result<(), Box<dyn std::error::Error>> {
        let modes = [
            Mode::Creating,
            Mode::Prompting,
            Mode::ChildPrompt,
            Mode::Broadcasting,
            Mode::ReconnectPrompt,
            Mode::TerminalPrompt,
            Mode::CustomAgentCommand,
        ];

        for mode in modes {
            let (mut app, _temp) = create_test_app()?;
            app.mode = mode.clone();
            app.input.buffer = "hello world".to_string();
            app.input.cursor = app.input.buffer.len();

            // Delete word, then clear line.
            handle_text_input_mode(
                &mut app,
                Actions::new(),
                KeyCode::Char('w'),
                KeyModifiers::CONTROL,
            )?;
            handle_text_input_mode(
                &mut app,
                Actions::new(),
                KeyCode::Char('u'),
                KeyModifiers::CONTROL,
            )?;
            assert!(app.input.buffer.is_empty());

            // Insert, backspace, delete, and cursor movement.
            handle_text_input_mode(
                &mut app,
                Actions::new(),
                KeyCode::Char('a'),
                KeyModifiers::NONE,
            )?;
            handle_text_input_mode(
                &mut app,
                Actions::new(),
                KeyCode::Backspace,
                KeyModifiers::NONE,
            )?;
            handle_text_input_mode(
                &mut app,
                Actions::new(),
                KeyCode::Char('b'),
                KeyModifiers::NONE,
            )?;
            handle_text_input_mode(
                &mut app,
                Actions::new(),
                KeyCode::Delete,
                KeyModifiers::NONE,
            )?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::Left, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::Right, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::Up, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::Down, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::Home, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::End, KeyModifiers::NONE)?;
            handle_text_input_mode(&mut app, Actions::new(), KeyCode::F(12), KeyModifiers::NONE)?;

            // Cancel (Esc) returns to Normal and clears any ReconnectPrompt conflict.
            if matches!(mode, Mode::ReconnectPrompt) {
                app.mode = Mode::ReconnectPrompt;
                app.spawn.worktree_conflict = Some(crate::app::WorktreeConflictInfo {
                    title: "test".to_string(),
                    branch: "test".to_string(),
                    worktree_path: std::path::PathBuf::from("/tmp"),
                    prompt: None,
                    existing_branch: None,
                    existing_commit: None,
                    current_branch: "main".to_string(),
                    current_commit: "abc1234".to_string(),
                    swarm_child_count: None,
                });
            }

            handle_text_input_mode(&mut app, Actions::new(), KeyCode::Esc, KeyModifiers::NONE)?;
            assert_eq!(app.mode, Mode::Normal);
            assert!(app.spawn.worktree_conflict.is_none());
        }

        Ok(())
    }

    #[test]
    fn test_text_input_submit_error_paths() -> Result<(), Box<dyn std::error::Error>> {
        let original_dir = std::env::current_dir()?;
        let non_git_dir = TempDir::new()?;
        std::env::set_current_dir(non_git_dir.path())?;

        let (mut app, _temp) = create_test_app()?;
        let handler = Actions::new();

        app.mode = Mode::Creating;
        app.input.buffer = "agent".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        app.mode = Mode::Prompting;
        app.input.buffer = "prompt".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        app.mode = Mode::ChildPrompt;
        app.input.buffer = "task".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        app.mode = Mode::Broadcasting;
        app.input.buffer = "msg".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        app.mode = Mode::ReconnectPrompt;
        app.input.buffer = "new prompt".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        app.mode = Mode::TerminalPrompt;
        app.input.buffer = "echo hi".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        app.mode = Mode::CustomAgentCommand;
        app.input.buffer = "   ".to_string();
        app.input.cursor = app.input.buffer.len();
        handle_text_input_mode(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::CustomAgentCommand);

        std::env::set_current_dir(original_dir)?;
        Ok(())
    }
}
