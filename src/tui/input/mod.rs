//! Mode-specific key handling
//!
//! This module provides a unified way to handle keyboard input based on the current mode.
//! Each mode has its own handler that implements the `ModeKeyHandler` trait.

mod confirm;
mod normal;
mod picker;
mod preview_focused;
mod text_input;

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use tenex::app::{Actions, App, Mode};

// Re-export for tests and internal use
#[cfg(test)]
pub use preview_focused::keycode_to_tmux_keys;

/// Handle a key event based on the current mode
///
/// Returns Ok(()) if the key was handled or ignored, or an error if something went wrong.
pub fn handle_key_event(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) -> Result<()> {
    match &app.mode {
        // Text input modes
        Mode::Creating
        | Mode::Prompting
        | Mode::ChildPrompt
        | Mode::Broadcasting
        | Mode::ReconnectPrompt
        | Mode::TerminalPrompt => {
            text_input::handle_text_input_mode(app, action_handler, code, modifiers);
        }

        // Count picker modes
        Mode::ChildCount => {
            picker::handle_child_count_mode(app, code);
        }
        Mode::ReviewChildCount => {
            picker::handle_review_child_count_mode(app, code);
        }
        Mode::ReviewInfo => {
            picker::handle_review_info_mode(app);
        }

        // Branch selector mode
        Mode::BranchSelector => {
            picker::handle_branch_selector_mode(app, action_handler, code);
        }

        // Git operation confirmation modes
        Mode::ConfirmPush => {
            confirm::handle_confirm_push_mode(app, code);
        }
        Mode::ConfirmPushForPR => {
            confirm::handle_confirm_push_for_pr_mode(app, code);
        }
        Mode::RenameBranch => {
            confirm::handle_rename_branch_mode(app, code);
        }

        // General confirmation mode
        Mode::Confirming(action) => {
            confirm::handle_confirming_mode(app, action_handler, *action, code)?;
        }

        // Rebase/Merge branch selector modes
        Mode::RebaseBranchSelector => {
            picker::handle_rebase_branch_selector_mode(app, code);
        }
        Mode::MergeBranchSelector => {
            picker::handle_merge_branch_selector_mode(app, code);
        }

        // Keyboard remap prompt
        Mode::KeyboardRemapPrompt => {
            confirm::handle_keyboard_remap_mode(app, code);
        }
        // Self-update prompt on startup
        Mode::UpdatePrompt(_) => {
            confirm::handle_update_prompt_mode(app, code);
        }
        // Update requested - ignore input while exiting
        Mode::UpdateRequested(_) => {}

        // Help, error, and success modes (dismiss on any key)
        Mode::Help => {
            app.exit_mode();
        }
        Mode::ErrorModal(_) => {
            app.dismiss_error();
        }
        Mode::SuccessModal(_) => {
            picker::handle_success_modal_mode(app);
        }

        // Preview focused mode (forwards keys to tmux)
        Mode::PreviewFocused => {
            preview_focused::handle_preview_focused_mode(app, code, modifiers, batched_keys);
        }

        // Normal and scrolling modes
        Mode::Normal | Mode::Scrolling => {
            normal::handle_normal_mode(app, action_handler, code, modifiers)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyCode;
    use semver::Version;
    use tempfile::NamedTempFile;
    use tenex::agent::Storage;
    use tenex::app::{ConfirmAction, Settings};
    use tenex::config::Config;
    use tenex::update::UpdateInfo;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    // ========== Mode routing integration tests ==========

    #[test]
    fn test_handle_key_event_help_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_error_modal_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ErrorModal("test error".to_string());
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_success_modal_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::SuccessModal("success!".to_string());
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char(' '),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPush;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ConfirmPushForPR;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RenameBranch;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.input.buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_keyboard_remap_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::KeyboardRemapPrompt;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert!(app.settings.merge_key_remapped);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_update_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdatePrompt(info);
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_update_requested_mode_ignores_input() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = Mode::UpdateRequested(info);
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        // Should remain in UpdateRequested mode - input is ignored
        assert!(matches!(app.mode, Mode::UpdateRequested(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Confirming(ConfirmAction::Quit);
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Creating;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('t'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Creating);
        assert_eq!(app.input.buffer, "t");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Prompting;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildCount;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewChildCount;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_info_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewInfo;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        // ReviewInfo mode exits on any key
        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        // Should exit to Normal mode
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rebase_branch_selector_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RebaseBranchSelector;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_merge_branch_selector_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::MergeBranchSelector;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Broadcasting;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('h'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Broadcasting);
        assert_eq!(app.input.buffer, "h");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_reconnect_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReconnectPrompt;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildPrompt;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::ChildPrompt);
        assert_eq!(app.input.buffer, "x");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_terminal_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::TerminalPrompt;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Scrolling;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Normal;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('?'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        Ok(())
    }
}
