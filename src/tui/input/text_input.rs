//! Text input mode key handling
//!
//! Handles key events for modes that involve text input:
//! - `Creating` (new agent name)
//! - `Prompting` (new agent with prompt)
//! - `ChildPrompt` (task for children)
//! - `Broadcasting` (message to leaves)
//! - `ReconnectPrompt` (reconnect with edited prompt)
//! - `TerminalPrompt` (terminal startup command)

use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use tenex::app::{Actions, App, Mode};
use uuid::Uuid;

/// Handle key events in text input modes
pub fn handle_text_input_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    match code {
        KeyCode::Enter if modifiers.contains(KeyModifiers::ALT) => {
            // Alt+Enter inserts a newline
            app.handle_char('\n');
        }
        KeyCode::Enter => {
            handle_enter(app, action_handler);
        }
        KeyCode::Esc => {
            handle_escape(app);
        }
        KeyCode::Char(c) => app.handle_char(c),
        KeyCode::Backspace => app.handle_backspace(),
        KeyCode::Delete => app.handle_delete(),
        KeyCode::Left => app.input.cursor_left(),
        KeyCode::Right => app.input.cursor_right(),
        KeyCode::Up => app.input.cursor_up(),
        KeyCode::Down => app.input.cursor_down(),
        KeyCode::Home => app.input.cursor_home(),
        KeyCode::End => app.input.cursor_end(),
        _ => {}
    }
}

/// Handle Enter key in text input modes
fn handle_enter(app: &mut App, action_handler: Actions) {
    let input = app.input.buffer.clone();

    if matches!(app.mode, Mode::CustomAgentCommand) && input.trim().is_empty() {
        app.set_status("Custom agent command cannot be empty");
        return;
    }

    // Some modes allow empty input, others don't
    if !input.is_empty()
        || matches!(
            app.mode,
            Mode::ReconnectPrompt | Mode::Prompting | Mode::ChildPrompt | Mode::TerminalPrompt
        )
    {
        // Remember the original mode before the action
        let original_mode = app.mode.clone();

        let result = match app.mode {
            Mode::Creating => action_handler.create_agent(app, &input, None),
            Mode::Prompting => {
                let short_id = &Uuid::new_v4().to_string()[..8];
                let title = format!("Agent ({short_id})");
                let prompt = if input.is_empty() {
                    None
                } else {
                    Some(input.as_str())
                };
                action_handler.create_agent(app, &title, prompt)
            }
            Mode::ChildPrompt => {
                let prompt = if input.is_empty() {
                    None
                } else {
                    Some(input.as_str())
                };
                action_handler.spawn_children(app, prompt)
            }
            Mode::Broadcasting => action_handler.broadcast_to_leaves(app, &input),
            Mode::ReconnectPrompt => {
                // Update the prompt in the conflict info and reconnect
                if let Some(ref mut conflict) = app.spawn.worktree_conflict {
                    conflict.prompt = if input.is_empty() { None } else { Some(input) };
                }
                action_handler.reconnect_to_worktree(app)
            }
            Mode::TerminalPrompt => {
                let command = if input.is_empty() {
                    None
                } else {
                    Some(input.as_str())
                };
                action_handler.spawn_terminal(app, command)
            }
            Mode::CustomAgentCommand => {
                let command = input.trim().to_string();
                app.set_custom_agent_command_and_save(command);
                Ok(())
            }
            _ => Ok(()),
        };

        if let Err(e) = result {
            app.set_error(format!("Failed: {e:#}"));
            // Don't call exit_mode() - set_error already set ErrorModal mode
            return;
        }

        // Only exit mode if it wasn't changed by the action
        // (e.g., create_agent might set Confirming mode for worktree conflicts)
        if app.mode == original_mode {
            app.exit_mode();
        }
        return;
    }

    // Empty input in Creating mode - just exit
    app.exit_mode();
}

/// Handle Escape key in text input modes
fn handle_escape(app: &mut App) {
    // For ReconnectPrompt, cancel and clear conflict info
    if matches!(app.mode, Mode::ReconnectPrompt) {
        app.spawn.worktree_conflict = None;
    }
    app.exit_mode();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use tenex::agent::Storage;
    use tenex::app::Settings;
    use tenex::config::Config;

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
        );
        assert_eq!(app.input.buffer, "a");

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        );
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
        );
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
        );
        assert_eq!(app.input.buffer, "est");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_cursor_movement() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();
        app.input.cursor = 2;

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.input.cursor, 1);

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(app.input.cursor, 2);

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(app.input.cursor, 0);

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::End, KeyModifiers::NONE);
        assert_eq!(app.input.cursor, 4);

        Ok(())
    }

    #[test]
    fn test_handle_text_input_escape() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_escape_reconnect_prompt_clears_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReconnectPrompt;
        app.spawn.worktree_conflict = Some(tenex::app::WorktreeConflictInfo {
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

        handle_escape(&mut app);

        assert!(app.spawn.worktree_conflict.is_none());
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_enter_empty_creating_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = String::new();

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_alt_enter_inserts_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;

        handle_text_input_mode(&mut app, Actions::new(), KeyCode::Enter, KeyModifiers::ALT);
        assert!(app.input.buffer.contains('\n'));

        Ok(())
    }
}
