//! Confirmation mode key handling
//!
//! Handles key events for various confirmation dialogs:
//! - `ConfirmPush` (push branch to remote)
//! - `ConfirmPushForPR` (push and open PR)
//! - `RenameBranch` (rename agent/branch)
//! - `Confirming` (general yes/no confirmations)

use anyhow::Result;
use ratatui::crossterm::event::KeyCode;
use tenex::app::{Actions, App, ConfirmAction, Mode};
use tenex::config::Action;

/// Handle key events in `ConfirmPush` mode
pub fn handle_confirm_push_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y' | 'Y') => {
            if let Err(e) = Actions::execute_push(app) {
                app.set_error(format!("Push failed: {e:#}"));
            }
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.clear_git_op_state();
            app.exit_mode();
        }
        _ => {}
    }
}

/// Handle key events in `ConfirmPushForPR` mode
pub fn handle_confirm_push_for_pr_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y' | 'Y') => {
            if let Err(e) = Actions::execute_push_and_open_pr(app) {
                app.set_error(format!("Failed to push and open PR: {e:#}"));
            }
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.clear_git_op_state();
            app.exit_mode();
        }
        _ => {}
    }
}

/// Handle key events in `RenameBranch` mode
pub fn handle_rename_branch_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            if app.confirm_rename_branch()
                && let Err(e) = Actions::execute_rename(app)
            {
                app.set_error(format!("Rename failed: {e:#}"));
            }
            // If rename failed (empty name), stay in mode
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

/// Handle key events in Confirming mode (general yes/no confirmations)
pub fn handle_confirming_mode(
    app: &mut App,
    action_handler: Actions,
    action: ConfirmAction,
    code: KeyCode,
) -> Result<()> {
    match action {
        ConfirmAction::WorktreeConflict => {
            handle_worktree_conflict_mode(app, action_handler, code)?;
        }
        _ => {
            handle_general_confirm_mode(app, action_handler, code)?;
        }
    }
    Ok(())
}

/// Handle key events for worktree conflict confirmation
fn handle_worktree_conflict_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    match code {
        KeyCode::Char('r' | 'R') => {
            // Transition to ReconnectPrompt mode to allow editing the prompt
            // Pre-fill input buffer with existing prompt if available
            if let Some(ref conflict) = app.spawn.worktree_conflict {
                app.input.buffer = conflict.prompt.clone().unwrap_or_default();
                app.input.cursor = app.input.buffer.len();
            }
            app.enter_mode(Mode::ReconnectPrompt);
        }
        KeyCode::Char('d' | 'D') => {
            app.exit_mode();
            action_handler.recreate_worktree(app)?;
        }
        KeyCode::Esc => {
            app.spawn.worktree_conflict = None;
            app.exit_mode();
        }
        _ => {}
    }
    Ok(())
}

/// Handle key events for general yes/no confirmations (Kill, Reset, Quit, Synthesize)
fn handle_general_confirm_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    match code {
        KeyCode::Char('y' | 'Y') => {
            action_handler.handle_action(app, Action::Confirm)?;
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.exit_mode();
        }
        _ => {}
    }
    Ok(())
}

/// Handle key events in `KeyboardRemapPrompt` mode
/// Asks user if they want to remap Ctrl+M to Ctrl+N due to terminal incompatibility
pub fn handle_keyboard_remap_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y' | 'Y') => {
            app.accept_keyboard_remap();
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.decline_keyboard_remap();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyCode;
    use tempfile::NamedTempFile;
    use tenex::agent::Storage;
    use tenex::app::{Mode, Settings};
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
    fn test_handle_confirm_push_mode_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Char('n'));
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('n'));
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        handle_rename_branch_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_char() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        handle_rename_branch_mode(&mut app, KeyCode::Char('a'));
        assert_eq!(app.input.buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_backspace() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;
        handle_rename_branch_mode(&mut app, KeyCode::Backspace);
        assert_eq!(app.input.buffer, "tes");
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_yes() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('y'));
        assert!(app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('n'));
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, KeyCode::Esc);
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }
}
