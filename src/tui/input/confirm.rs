//! Confirmation mode key handling
//!
//! Handles key events for various confirmation dialogs:
//! - `ConfirmPush` (push branch to remote)
//! - `ConfirmPushForPR` (push and open PR)
//! - `RenameBranch` (rename agent/branch)
//! - `Confirming` (general yes/no confirmations)
//! - `UpdatePrompt` (self-update prompt on startup)

use crate::app::{Actions, App, ConfirmAction};
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `ConfirmPush` mode
pub fn handle_confirm_push_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_confirm_push_mode(app, action_handler, code)
}

/// Handle key events in `ConfirmPushForPR` mode
pub fn handle_confirm_push_for_pr_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_confirm_push_for_pr_mode(app, action_handler, code)
}

/// Handle key events in `RenameBranch` mode
pub fn handle_rename_branch_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_rename_branch_mode(app, action_handler, code)
}

/// Handle key events in Confirming mode (general yes/no confirmations)
pub fn handle_confirming_mode(
    app: &mut App,
    action_handler: Actions,
    action: ConfirmAction,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_confirming_mode(app, action_handler, action, code)
}

/// Handle key events in `KeyboardRemapPrompt` mode
/// Asks user if they want to remap Ctrl+M to Ctrl+N due to terminal incompatibility
pub fn handle_keyboard_remap_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_keyboard_remap_prompt_mode(app, action_handler, code)
}

/// Handle key events in `UpdatePrompt` mode.
///
/// If the user accepts, switch to `UpdateRequested` so the TUI can exit
/// and the binary can run the updater.
pub fn handle_update_prompt_mode(
    app: &mut App,
    action_handler: Actions,
    info: UpdateInfo,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_update_prompt_mode(app, action_handler, info, code)
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
    fn test_handle_confirm_push_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, Actions::new(), KeyCode::Char('n'))?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_uppercase_n() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, Actions::new(), KeyCode::Char('N'))?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_yes_sets_error_without_git()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, Actions::new(), KeyCode::Char('y'))?;
        // Should set an error since there's no git repo
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_uppercase_y() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, Actions::new(), KeyCode::Char('Y'))?;
        // Should set an error since there's no git repo
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        handle_confirm_push_mode(&mut app, Actions::new(), KeyCode::Char('x'))?;
        assert_eq!(app.mode, Mode::ConfirmPush);
        Ok(())
    }

    // ========== ConfirmPushForPR mode tests ==========

    #[test]
    fn test_handle_confirm_push_for_pr_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, Actions::new(), KeyCode::Char('n'))?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_uppercase_n() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, Actions::new(), KeyCode::Char('N'))?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_yes_sets_error_without_git()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, Actions::new(), KeyCode::Char('y'))?;
        // Should set an error since there's no git repo
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_uppercase_y() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, Actions::new(), KeyCode::Char('Y'))?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        handle_confirm_push_for_pr_mode(&mut app, Actions::new(), KeyCode::Char('x'))?;
        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        Ok(())
    }

    // ========== RenameBranch mode tests ==========

    #[test]
    fn test_handle_rename_branch_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        handle_rename_branch_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_char() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        handle_rename_branch_mode(&mut app, Actions::new(), KeyCode::Char('a'))?;
        assert_eq!(app.input.buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        app.input.buffer = "test".to_string();
        app.input.cursor = 4;
        handle_rename_branch_mode(&mut app, Actions::new(), KeyCode::Backspace)?;
        assert_eq!(app.input.buffer, "tes");
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_enter_empty_stays_in_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        app.input.buffer.clear();
        handle_rename_branch_mode(&mut app, Actions::new(), KeyCode::Enter)?;
        // Should stay in RenameBranch mode because name is empty
        assert_eq!(app.mode, Mode::RenameBranch);
        Ok(())
    }

    #[test]
    fn test_handle_rename_branch_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        handle_rename_branch_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.mode, Mode::RenameBranch);
        assert!(app.input.buffer.is_empty());
        Ok(())
    }

    // ========== KeyboardRemap mode tests ==========

    #[test]
    fn test_handle_keyboard_remap_mode_yes() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, Actions::new(), KeyCode::Char('y'))?;
        assert!(app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_uppercase_yes() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, Actions::new(), KeyCode::Char('Y'))?;
        assert!(app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, Actions::new(), KeyCode::Char('n'))?;
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_uppercase_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, Actions::new(), KeyCode::Char('N'))?;
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_keyboard_remap_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        handle_keyboard_remap_mode(&mut app, Actions::new(), KeyCode::Char('x'))?;
        assert!(!app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::KeyboardRemapPrompt);
        Ok(())
    }

    // ========== UpdatePrompt mode tests ==========

    #[test]
    fn test_handle_update_prompt_mode_yes() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, Actions::new(), info.clone(), KeyCode::Char('y'))?;
        assert!(matches!(app.mode, Mode::UpdateRequested(_)));
        if let Mode::UpdateRequested(req_info) = &app.mode {
            assert_eq!(req_info.current_version, info.current_version);
            assert_eq!(req_info.latest_version, info.latest_version);
        }
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_uppercase_yes() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, Actions::new(), info, KeyCode::Char('Y'))?;
        assert!(matches!(app.mode, Mode::UpdateRequested(_)));
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, Actions::new(), info, KeyCode::Char('n'))?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_uppercase_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, Actions::new(), info, KeyCode::Char('N'))?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, Actions::new(), info, KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_update_prompt_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info.clone());
        handle_update_prompt_mode(&mut app, Actions::new(), info, KeyCode::Char('x'))?;
        assert!(matches!(app.mode, Mode::UpdatePrompt(_)));
        Ok(())
    }

    // ========== WorktreeConflict mode tests ==========

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('r'),
        )?;

        assert_eq!(app.mode, Mode::ReconnectPrompt);
        assert_eq!(app.input.buffer, "test prompt");
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect_uppercase()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('R'),
        )?;

        assert_eq!(app.mode, Mode::ReconnectPrompt);
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect_no_prompt()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let mut conflict = create_worktree_conflict_info();
        conflict.prompt = None;
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('r'),
        )?;

        assert_eq!(app.mode, Mode::ReconnectPrompt);
        assert!(app.input.buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::WorktreeConflict,
            KeyCode::Esc,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.spawn.worktree_conflict.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_worktree_conflict_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('x'),
        )?;

        assert!(matches!(
            app.mode,
            Mode::Confirming(ConfirmAction::WorktreeConflict)
        ));
        Ok(())
    }

    // ========== GeneralConfirm mode tests ==========

    #[test]
    fn test_handle_general_confirm_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::Quit,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_general_confirm_mode_uppercase_no() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::Quit,
            KeyCode::Char('N'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_general_confirm_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        handle_confirming_mode(&mut app, Actions::new(), ConfirmAction::Quit, KeyCode::Esc)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_general_confirm_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::Quit,
            KeyCode::Char('x'),
        )?;

        assert!(matches!(app.mode, Mode::Confirming(ConfirmAction::Quit)));
        Ok(())
    }

    // ========== Confirming mode routing tests ==========

    #[test]
    fn test_handle_confirming_mode_routes_to_worktree_conflict()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        let conflict = create_worktree_conflict_info();
        app.spawn.worktree_conflict = Some(conflict);
        app.mode = Mode::Confirming(ConfirmAction::WorktreeConflict);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::WorktreeConflict,
            KeyCode::Esc,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_quit() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::Quit,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_kill() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Kill);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::Kill,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_reset()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Reset);

        handle_confirming_mode(
            &mut app,
            Actions::new(),
            ConfirmAction::Reset,
            KeyCode::Char('n'),
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }
}
