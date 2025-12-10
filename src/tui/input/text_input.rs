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
        KeyCode::Left => app.input_cursor_left(),
        KeyCode::Right => app.input_cursor_right(),
        KeyCode::Up => app.input_cursor_up(),
        KeyCode::Down => app.input_cursor_down(),
        KeyCode::Home => app.input_cursor_home(),
        KeyCode::End => app.input_cursor_end(),
        _ => {}
    }
}

/// Handle Enter key in text input modes
fn handle_enter(app: &mut App, action_handler: Actions) {
    let input = app.input_buffer.clone();

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
                if let Some(ref mut conflict) = app.worktree_conflict {
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
        app.worktree_conflict = None;
    }
    app.exit_mode();
}
