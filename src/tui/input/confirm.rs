//! Confirmation mode key handling
//!
//! Handles key events for various confirmation dialogs:
//! - `ConfirmPush` (push branch to remote)
//! - `ConfirmPushForPR` (push and open PR)
//! - `RenameBranch` (rename agent/branch)
//! - `Confirming` (general yes/no confirmations)
//! - `UpdatePrompt` (self-update prompt on startup)

use crate::app::App;
use crate::state::ConfirmAction;
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `ConfirmPush` mode
pub fn handle_confirm_push_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_confirm_push_mode(app, code)
}

/// Handle key events in `ConfirmPushForPR` mode
pub fn handle_confirm_push_for_pr_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_confirm_push_for_pr_mode(app, code)
}

/// Handle key events in `RenameBranch` mode
pub fn handle_rename_branch_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_rename_branch_mode(app, code)
}

/// Handle key events in Confirming mode (general yes/no confirmations)
pub fn handle_confirming_mode(app: &mut App, action: ConfirmAction, code: KeyCode) -> Result<()> {
    crate::action::dispatch_confirming_mode(app, action, code)
}

/// Handle key events in `KeyboardRemapPrompt` mode
/// Asks user if they want to remap Ctrl+M to Ctrl+N due to terminal incompatibility
pub fn handle_keyboard_remap_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_keyboard_remap_prompt_mode(app, code)
}

/// Handle key events in `UpdatePrompt` mode.
///
/// If the user accepts, switch to `UpdateRequested` so the TUI can exit
/// and the binary can run the updater.
pub fn handle_update_prompt_mode(app: &mut App, info: &UpdateInfo, code: KeyCode) -> Result<()> {
    crate::action::dispatch_update_prompt_mode(app, info, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{Settings, WorktreeConflictInfo};
    use crate::config::Config;
    use crate::state::{
        AppMode, ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode, KeyboardRemapPromptMode,
        ReconnectPromptMode, RenameBranchMode, UpdatePromptMode,
    };
    use crate::update::UpdateInfo;
    use ratatui::crossterm::event::KeyCode;
    use semver::Version;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn assert_mode_variant(mode: &AppMode, expected: &AppMode) {
        assert_eq!(
            std::mem::discriminant(mode),
            std::mem::discriminant(expected)
        );
    }

    fn update_requested_state(mode: &AppMode) -> Option<&crate::state::UpdateRequestedMode> {
        match mode {
            AppMode::UpdateRequested(state) => Some(state),
            _ => None,
        }
    }

    fn confirming_action(mode: &AppMode) -> Option<ConfirmAction> {
        match mode {
            AppMode::Confirming(state) => Some(state.action),
            _ => None,
        }
    }

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    fn create_worktree_conflict_info() -> WorktreeConflictInfo {
        WorktreeConflictInfo {
            title: "test-agent".to_string(),
            prompt: Some("test prompt".to_string()),
            branch: "test-branch".to_string(),
            worktree_path: PathBuf::from("/tmp/test-worktree"),
            repo_root: PathBuf::from("/tmp"),
            existing_branch: Some("main".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        }
    }

    // ========== ConfirmPush mode tests ==========

    #[test]
    fn test_handle_confirm_push_mode_no() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushMode.into());
        handle_confirm_push_mode(&mut app, KeyCode::Char('n')).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirm_push_mode_uppercase_n() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushMode.into());
        handle_confirm_push_mode(&mut app, KeyCode::Char('N')).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirm_push_mode_esc() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushMode.into());
        handle_confirm_push_mode(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirm_push_mode_yes_sets_error_without_git() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushMode.into());
        handle_confirm_push_mode(&mut app, KeyCode::Char('y')).unwrap();
        assert_mode_variant(
            &app.mode,
            &AppMode::ErrorModal(crate::state::ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_handle_confirm_push_mode_uppercase_y() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushMode.into());
        handle_confirm_push_mode(&mut app, KeyCode::Char('Y')).unwrap();
        assert_mode_variant(
            &app.mode,
            &AppMode::ErrorModal(crate::state::ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_handle_confirm_push_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushMode.into());
        handle_confirm_push_mode(&mut app, KeyCode::Char('x')).unwrap();
        assert_eq!(app.mode, ConfirmPushMode.into());
    }

    // ========== ConfirmPushForPR mode tests ==========

    #[test]
    fn test_handle_confirm_push_for_pr_mode_no() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushForPRMode.into());
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('n')).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_uppercase_n() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushForPRMode.into());
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('N')).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_esc() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushForPRMode.into());
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_yes_sets_error_without_git() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushForPRMode.into());
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('y')).unwrap();
        assert_mode_variant(
            &app.mode,
            &AppMode::ErrorModal(crate::state::ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_uppercase_y() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushForPRMode.into());
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('Y')).unwrap();
        assert_mode_variant(
            &app.mode,
            &AppMode::ErrorModal(crate::state::ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_handle_confirm_push_for_pr_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ConfirmPushForPRMode.into());
        handle_confirm_push_for_pr_mode(&mut app, KeyCode::Char('x')).unwrap();
        assert_eq!(app.mode, ConfirmPushForPRMode.into());
    }

    // ========== RenameBranch mode tests ==========

    #[test]
    fn test_handle_rename_branch_mode_esc() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(RenameBranchMode.into());
        handle_rename_branch_mode(&mut app, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_rename_branch_mode_char() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(RenameBranchMode.into());
        handle_rename_branch_mode(&mut app, KeyCode::Char('a')).unwrap();
        assert_eq!(app.data.input.buffer, "a");
    }

    #[test]
    fn test_handle_rename_branch_mode_backspace() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(RenameBranchMode.into());
        app.data.input.buffer = "test".to_string();
        app.data.input.cursor = 4;
        handle_rename_branch_mode(&mut app, KeyCode::Backspace).unwrap();
        assert_eq!(app.data.input.buffer, "tes");
    }

    #[test]
    fn test_handle_rename_branch_mode_enter_empty_stays_in_mode() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(RenameBranchMode.into());
        app.data.input.buffer.clear();
        handle_rename_branch_mode(&mut app, KeyCode::Enter).unwrap();
        // Should stay in RenameBranch mode because name is empty
        assert_eq!(app.mode, RenameBranchMode.into());
    }

    #[test]
    fn test_handle_rename_branch_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(RenameBranchMode.into());
        handle_rename_branch_mode(&mut app, KeyCode::Tab).unwrap();
        assert_eq!(app.mode, RenameBranchMode.into());
        assert!(app.data.input.buffer.is_empty());
    }

    // ========== KeyboardRemap mode tests ==========

    #[test]
    fn test_handle_keyboard_remap_mode_yes() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(KeyboardRemapPromptMode.into());
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('y')).unwrap();
        assert!(app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_keyboard_remap_mode_uppercase_yes() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(KeyboardRemapPromptMode.into());
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('Y')).unwrap();
        assert!(app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_keyboard_remap_mode_no() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(KeyboardRemapPromptMode.into());
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('n')).unwrap();
        assert!(!app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_keyboard_remap_mode_uppercase_no() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(KeyboardRemapPromptMode.into());
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('N')).unwrap();
        assert!(!app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_keyboard_remap_mode_esc() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(KeyboardRemapPromptMode.into());
        handle_keyboard_remap_mode(&mut app, KeyCode::Esc).unwrap();
        assert!(!app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_keyboard_remap_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(KeyboardRemapPromptMode.into());
        handle_keyboard_remap_mode(&mut app, KeyCode::Char('x')).unwrap();
        assert!(!app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, KeyboardRemapPromptMode.into());
    }

    // ========== UpdatePrompt mode tests ==========

    #[test]
    fn test_handle_update_prompt_mode_yes() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.apply_mode(UpdatePromptMode { info: info.clone() }.into());
        handle_update_prompt_mode(&mut app, &info, KeyCode::Char('y'))
            .expect("Expected update prompt to handle y");
        let state = update_requested_state(&app.mode).expect("Expected UpdateRequested mode");
        assert_eq!(state.info.current_version, info.current_version);
        assert_eq!(state.info.latest_version, info.latest_version);

        assert!(update_requested_state(&AppMode::normal()).is_none());
    }

    #[test]
    fn test_handle_update_prompt_mode_uppercase_yes() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.apply_mode(UpdatePromptMode { info: info.clone() }.into());
        handle_update_prompt_mode(&mut app, &info, KeyCode::Char('Y')).unwrap();
        let _ = update_requested_state(&app.mode).expect("Expected UpdateRequested mode");
    }

    #[test]
    fn test_handle_update_prompt_mode_no() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.apply_mode(UpdatePromptMode { info: info.clone() }.into());
        handle_update_prompt_mode(&mut app, &info, KeyCode::Char('n')).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_update_prompt_mode_uppercase_no() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.apply_mode(UpdatePromptMode { info: info.clone() }.into());
        handle_update_prompt_mode(&mut app, &info, KeyCode::Char('N')).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_update_prompt_mode_esc() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.apply_mode(UpdatePromptMode { info: info.clone() }.into());
        handle_update_prompt_mode(&mut app, &info, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_update_prompt_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.apply_mode(UpdatePromptMode { info: info.clone() }.into());
        handle_update_prompt_mode(&mut app, &info, KeyCode::Char('x')).unwrap();
        assert_mode_variant(&app.mode, &AppMode::UpdatePrompt(UpdatePromptMode { info }));
    }

    // ========== WorktreeConflict mode tests ==========

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect() {
        let (mut app, _temp) = create_test_app();
        let conflict = create_worktree_conflict_info();
        app.data.spawn.worktree_conflict = Some(conflict);
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        handle_confirming_mode(
            &mut app,
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('r'),
        )
        .expect("Expected reconnect to be handled");

        assert_eq!(app.mode, ReconnectPromptMode.into());
        assert_eq!(app.data.input.buffer, "test prompt");
    }

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect_uppercase() {
        let (mut app, _temp) = create_test_app();
        let conflict = create_worktree_conflict_info();
        app.data.spawn.worktree_conflict = Some(conflict);
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        handle_confirming_mode(
            &mut app,
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('R'),
        )
        .expect("Expected reconnect to be handled");

        assert_eq!(app.mode, ReconnectPromptMode.into());
    }

    #[test]
    fn test_handle_worktree_conflict_mode_reconnect_no_prompt() {
        let (mut app, _temp) = create_test_app();
        let mut conflict = create_worktree_conflict_info();
        conflict.prompt = None;
        app.data.spawn.worktree_conflict = Some(conflict);
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        handle_confirming_mode(
            &mut app,
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('r'),
        )
        .expect("Expected reconnect to be handled");

        assert_eq!(app.mode, ReconnectPromptMode.into());
        assert!(app.data.input.buffer.is_empty());
    }

    #[test]
    fn test_handle_worktree_conflict_mode_esc() {
        let (mut app, _temp) = create_test_app();
        let conflict = create_worktree_conflict_info();
        app.data.spawn.worktree_conflict = Some(conflict);
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::WorktreeConflict, KeyCode::Esc).unwrap();

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.spawn.worktree_conflict.is_none());
    }

    #[test]
    fn test_handle_worktree_conflict_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        let conflict = create_worktree_conflict_info();
        app.data.spawn.worktree_conflict = Some(conflict);
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        handle_confirming_mode(
            &mut app,
            ConfirmAction::WorktreeConflict,
            KeyCode::Char('x'),
        )
        .expect("Expected conflict mode to ignore other key");

        assert_eq!(
            confirming_action(&app.mode).expect("Expected Confirming mode"),
            ConfirmAction::WorktreeConflict
        );
        assert!(confirming_action(&AppMode::normal()).is_none());
    }

    // ========== GeneralConfirm mode tests ==========

    #[test]
    fn test_handle_general_confirm_mode_no() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Quit, KeyCode::Char('n')).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_general_confirm_mode_uppercase_no() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Quit, KeyCode::Char('N')).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_general_confirm_mode_esc() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Quit, KeyCode::Esc).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_general_confirm_mode_other_key_ignored() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Quit, KeyCode::Char('x')).unwrap();

        assert_eq!(
            confirming_action(&app.mode).expect("Expected Confirming mode"),
            ConfirmAction::Quit
        );
    }

    // ========== Confirming mode routing tests ==========

    #[test]
    fn test_handle_confirming_mode_routes_to_worktree_conflict() {
        let (mut app, _temp) = create_test_app();
        let conflict = create_worktree_conflict_info();
        app.data.spawn.worktree_conflict = Some(conflict);
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::WorktreeConflict, KeyCode::Esc).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_quit() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Quit, KeyCode::Char('n')).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_kill() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Kill, KeyCode::Char('n')).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_confirming_mode_routes_to_general_reset() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::Reset,
            }
            .into(),
        );

        handle_confirming_mode(&mut app, ConfirmAction::Reset, KeyCode::Char('n')).unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }
}
