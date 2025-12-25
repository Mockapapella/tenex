//! Confirmation mode key handling
//!
//! Handles key events for various confirmation dialogs:
//! - `ConfirmPush` (push branch to remote)
//! - `ConfirmPushForPR` (push and open PR)
//! - `RenameBranch` (rename agent/branch)
//! - `Confirming` (general yes/no confirmations)
//! - `UpdatePrompt` (self-update prompt on startup)

use crate::app::{Actions, App, ConfirmAction, Mode};
use crate::config::Action;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

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

/// Handle key events in `UpdatePrompt` mode.
///
/// If the user accepts, switch to `UpdateRequested` so the TUI can exit
/// and the binary can run the updater.
pub fn handle_update_prompt_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('y' | 'Y') => {
            if let Mode::UpdatePrompt(info) = app.mode.clone() {
                app.mode = Mode::UpdateRequested(info);
            }
        }
        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
            app.exit_mode();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{Mode, Settings, WorktreeConflictInfo};
    use crate::config::Config;
    use crate::update::UpdateInfo;
    use ratatui::crossterm::event::KeyCode;
    use semver::Version;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    fn create_worktree_conflict_info() -> WorktreeConflictInfo {
        WorktreeConflictInfo {
            title: "test-agent".to_string(),
            prompt: Some("test prompt".to_string()),
            branch: "test-branch".to_string(),
            worktree_path: PathBuf::from("/tmp/test-worktree"),
            existing_branch: Some("main".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        }
    }

    // ========== ConfirmPush mode tests ==========

    #[test]
    fn test_handle_confirm_push_mode_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Char('n'));
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_uppercase_n() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Char('N'));
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
    fn test_handle_confirm_push_mode_yes_sets_error_without_git() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Char('y'));
        // Should set an error since there's no git repo
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_uppercase_y() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Char('Y'));
        // Should set an error since there's no git repo
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, KeyCode::Char('x'));
        assert_eq!(app.mode, Mode::ConfirmPush);
        Ok(())
    }

    // ========== ConfirmPushForPR mode tests ==========

    #[test]
    fn test_handle_confirm_push_for_pr_mode_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('n'));
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_uppercase_n() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('N'));
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
    fn test_handle_confirm_push_for_pr_mode_yes_sets_error_without_git()
    -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('y'));
        // Should set an error since there's no git repo
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_uppercase_y() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('Y'));
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('x'));
        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        Ok(())
    }

    // ========== RenameBranch mode tests ==========

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
    fn test_handle_rename_branch_mode_enter_empty_stays_in_mode() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        app.input.buffer.clear();
        handle_rename_branch_mode(&mut app, KeyCode::Enter);
        // Should stay in RenameBranch mode because name is empty
        assert_eq!(app.mode, Mode::RenameBranch);
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        handle_rename_branch_mode(&mut app, KeyCode::Tab);
        assert_eq!(app.mode, Mode::RenameBranch);
        assert!(app.input.buffer.is_empty());
        Ok(())
    }

    // ========== KeyboardRemap mode tests ==========

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
    fn test_handle_keyboard_remap_mode_uppercase_yes() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('Y'));
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
    fn test_handle_keyboard_remap_mode_uppercase_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('N'));
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

    #[test]
    fn test_handle_keyboard_remap_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('x'));
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::KeyboardRemapPrompt);
        Ok(())
    }

    // ========== UpdatePrompt mode tests ==========

    #[test]
    fn test_handle_update_prompt_mode_yes() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, KeyCode::Char('y'));
        assert!(matches!(app.mode, Mode::UpdateRequested(_)));
        if let Mode::UpdateRequested(req_info) = &app.mode {
            assert_eq!(req_info.current_version, info.current_version);
            assert_eq!(req_info.latest_version, info.latest_version);
        }
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_uppercase_yes() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info);
        handle_update_prompt_mode(&mut app, KeyCode::Char('Y'));
        assert!(matches!(app.mode, Mode::UpdateRequested(_)));
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info);
        handle_update_prompt_mode(&mut app, KeyCode::Char('n'));
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_uppercase_no() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info);
        handle_update_prompt_mode(&mut app, KeyCode::Char('N'));
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info);
        handle_update_prompt_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info);
        handle_update_prompt_mode(&mut app, KeyCode::Char('x'));
        assert!(matches!(app.mode, Mode::UpdatePrompt(_)));
        Ok(())
    }

    // ========== WorktreeConflict mode tests ==========

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        let action_handler = Actions::new();
        handle_worktree_conflict_mode(&mut app, action_handler, KeyCode::Char('r'))?;

        assert_eq!(app.mode, Mode::ReconnectPrompt);
        assert_eq!(app.input.buffer, "test prompt");
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect_uppercase() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        let action_handler = Actions::new();
        handle_worktree_conflict_mode(&mut app, action_handler, KeyCode::Char('R'))?;

        assert_eq!(app.mode, Mode::ReconnectPrompt);
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect_no_prompt() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        let mut conflict = create_worktree_conflict_info();
        conflict.prompt = None;
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        let action_handler = Actions::new();
        handle_worktree_conflict_mode(&mut app, action_handler, KeyCode::Char('r'))?;

        assert_eq!(app.mode, Mode::ReconnectPrompt);
        assert!(app.input.buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_esc() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        let action_handler = Actions::new();
        handle_worktree_conflict_mode(&mut app, action_handler, KeyCode::Esc)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.spawn.worktree_conflict.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_other_key_ignored() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        let action_handler = Actions::new();
        handle_worktree_conflict_mode(&mut app, action_handler, KeyCode::Char('x'))?;

        assert!(matches!(
            app.mode,
            Mode::Confirming(ConfirmAction::WorktreeConflict)
        ));
        Ok(())
    }

    // ========== GeneralConfirm mode tests ==========

    #[test]
    fn test_handle_general_confirm_mode_no() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        let action_handler = Actions::new();
        handle_general_confirm_mode(&mut app, action_handler, KeyCode::Char('n'))?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_general_confirm_mode_uppercase_no() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        let action_handler = Actions::new();
        handle_general_confirm_mode(&mut app, action_handler, KeyCode::Char('N'))?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_general_confirm_mode_esc() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        let action_handler = Actions::new();
        handle_general_confirm_mode(&mut app, action_handler, KeyCode::Esc)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_general_confirm_mode_other_key_ignored() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        let action_handler = Actions::new();
        handle_general_confirm_mode(&mut app, action_handler, KeyCode::Char('x'))?;

        assert!(matches!(app.mode, Mode::Confirming(ConfirmAction::Quit)));
        Ok(())
    }

    // ========== Confirming mode routing tests ==========

    #[test]
    fn test_handle_confirming_mode_routes_to_worktree_conflict() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        let action_handler = Actions::new();
        handle_confirming_mode(
            &mut app,
            action_handler,
            ConfirmAction::WorktreeConflict,
            KeyCode::Esc,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_quit() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        let action_handler = Actions::new();
        handle_confirming_mode(
            &mut app,
            action_handler,
            ConfirmAction::Quit,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_kill() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Kill);

        let action_handler = Actions::new();
        handle_confirming_mode(
            &mut app,
            action_handler,
            ConfirmAction::Kill,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_reset() -> Result<(), anyhow::Error> {
        let (mut app, _temp) = create_test_app().map_err(anyhow::Error::from)?;
        app.mode = Mode::Confirming(ConfirmAction::Reset);

        let action_handler = Actions::new();
        handle_confirming_mode(
            &mut app,
            action_handler,
            ConfirmAction::Reset,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }
}
