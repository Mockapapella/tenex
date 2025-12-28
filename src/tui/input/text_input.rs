//! Text input mode key handling
//!
//! Handles key events for modes that involve text input:
//! - `TextInputKind::Creating` (new agent name)
//! - `TextInputKind::Prompting` (new agent with prompt)
//! - `TextInputKind::ChildPrompt` (task for children)
//! - `TextInputKind::Broadcasting` (message to leaves)
//! - `TextInputKind::ReconnectPrompt` (reconnect with edited prompt)
//! - `TextInputKind::TerminalPrompt` (terminal startup command)
//! - `TextInputKind::CustomAgentCommand` (custom spawn command)
//! - `TextInputKind::RenameBranch` (rename flow input)

use crate::app::{Actions, App, TextInputKind};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use uuid::Uuid;

/// Handle key events in text input modes
pub fn handle_text_input_mode(
    app: &mut App,
    action_handler: Actions,
    kind: TextInputKind,
    code: KeyCode,
    modifiers: KeyModifiers,
) {
    if kind == TextInputKind::RenameBranch {
        handle_rename_branch_key(app, code);
        return;
    }

    match code {
        KeyCode::Enter if modifiers.contains(KeyModifiers::ALT) => {
            // Alt+Enter inserts a newline
            app.handle_char('\n');
        }
        KeyCode::Enter => {
            handle_enter(app, action_handler, kind);
        }
        KeyCode::Esc => {
            handle_escape(app, kind);
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
fn handle_enter(app: &mut App, action_handler: Actions, kind: TextInputKind) {
    let input = app.input.buffer.clone();

    if kind == TextInputKind::CustomAgentCommand && input.trim().is_empty() {
        app.set_status("Custom agent command cannot be empty");
        return;
    }

    // Some modes allow empty input, others don't
    if !input.is_empty() || kind.allows_empty_submit() {
        // Remember the original mode before the action (some actions transition to another mode).
        let original_mode = app.mode.clone();

        let result = match kind {
            TextInputKind::Creating => action_handler.create_agent(app, &input, None),
            TextInputKind::Prompting => {
                let short_id = &Uuid::new_v4().to_string()[..8];
                let title = format!("Agent ({short_id})");
                let prompt = if input.is_empty() {
                    None
                } else {
                    Some(input.as_str())
                };
                action_handler.create_agent(app, &title, prompt)
            }
            TextInputKind::ChildPrompt => {
                let prompt = if input.is_empty() {
                    None
                } else {
                    Some(input.as_str())
                };
                action_handler.spawn_children(app, prompt)
            }
            TextInputKind::Broadcasting => action_handler.broadcast_to_leaves(app, &input),
            TextInputKind::ReconnectPrompt => {
                // Update the prompt in the conflict info and reconnect
                if let Some(ref mut conflict) = app.spawn.worktree_conflict {
                    conflict.prompt = if input.is_empty() { None } else { Some(input) };
                }
                action_handler.reconnect_to_worktree(app)
            }
            TextInputKind::TerminalPrompt => {
                let command = if input.is_empty() {
                    None
                } else {
                    Some(input.as_str())
                };
                action_handler.spawn_terminal(app, command)
            }
            TextInputKind::CustomAgentCommand => {
                let command = input.trim().to_string();
                app.set_custom_agent_command_and_save(command);
                Ok(())
            }
            TextInputKind::RenameBranch => Ok(()),
        };

        if let Err(e) = result {
            app.set_error(format!("Failed: {e:#}"));
            // Don't call exit_mode() - set_error already set ErrorModal mode
            return;
        }

        // Only exit mode if it wasn't changed by the action
        // (e.g., create_agent might set a worktree conflict confirmation overlay)
        if app.mode == original_mode {
            app.exit_mode();
        }
        return;
    }

    // Empty input where it's not meaningful - just exit.
    app.exit_mode();
}

/// Handle Escape key in text input modes
fn handle_escape(app: &mut App, kind: TextInputKind) {
    // For ReconnectPrompt, cancel and clear conflict info
    if kind == TextInputKind::ReconnectPrompt {
        app.spawn.worktree_conflict = None;
    }
    app.exit_mode();
}

fn handle_rename_branch_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            if app.confirm_rename_branch()
                && let Err(e) = Actions::execute_rename(app)
            {
                app.set_error(format!("Rename failed: {e:#}"));
            }
            // If rename failed (empty name), stay in mode.
        }
        KeyCode::Esc => {
            app.clear_git_op_state();
            app.exit_mode();
        }
        KeyCode::Char(c) => app.handle_char(c),
        KeyCode::Backspace => app.handle_backspace(),
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

    #[test]
    fn test_handle_text_input_char() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.buffer, "a");

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Char('b'),
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.buffer, "ab");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Backspace,
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.buffer, "tes");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_delete() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
        app.input.buffer = "test".to_string();
        app.input.cursor = 0;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Delete,
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.buffer, "est");

        Ok(())
    }

    #[test]
    fn test_handle_text_input_cursor_movement() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
        app.input.buffer = "test".to_string();
        app.input.cursor = 2;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Left,
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.cursor, 1);

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Right,
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.cursor, 2);

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Home,
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.cursor, 0);

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::End,
            KeyModifiers::NONE,
        );
        assert_eq!(app.input.cursor, 4);

        Ok(())
    }

    #[test]
    fn test_handle_text_input_escape() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
        app.input.buffer = "test".to_string();

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Esc,
            KeyModifiers::NONE,
        );
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_escape_reconnect_prompt_clears_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::ReconnectPrompt));
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

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::ReconnectPrompt,
            KeyCode::Esc,
            KeyModifiers::NONE,
        );

        assert!(app.spawn.worktree_conflict.is_none());
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_enter_empty_creating_exits() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
        app.input.buffer = String::new();

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Enter,
            KeyModifiers::NONE,
        );
        assert_eq!(app.mode, Mode::Normal);

        Ok(())
    }

    #[test]
    fn test_handle_alt_enter_inserts_newline() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;

        handle_text_input_mode(
            &mut app,
            Actions::new(),
            TextInputKind::Creating,
            KeyCode::Enter,
            KeyModifiers::ALT,
        );
        assert!(app.input.buffer.contains('\n'));

        Ok(())
    }
}
