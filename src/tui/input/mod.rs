//! Mode-specific key handling
//!
//! This module provides a unified way to handle keyboard input based on the current mode.
//! Each mode has its own handler that implements the `ModeKeyHandler` trait.

mod command;
mod confirm;
mod normal;
mod picker;
mod text_input;

use crate::app::App;
use crate::state::AppMode;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Handle a key event based on the current mode
///
/// Returns Ok(()) if the key was handled or ignored, or an error if something went wrong.
pub fn handle_key_event(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) -> Result<()> {
    match &app.mode {
        // Text input modes
        AppMode::Creating(_)
        | AppMode::Prompting(_)
        | AppMode::ChildPrompt(_)
        | AppMode::Broadcasting(_)
        | AppMode::ReconnectPrompt(_)
        | AppMode::TerminalPrompt(_)
        | AppMode::CustomAgentCommand(_) => {
            text_input::handle_text_input_mode(app, code, modifiers)?;
        }

        // Count picker modes
        AppMode::ChildCount(_) => {
            picker::handle_child_count_mode(app, code)?;
        }
        AppMode::ReviewChildCount(_) => {
            picker::handle_review_child_count_mode(app, code)?;
        }
        AppMode::ReviewInfo(_) => {
            picker::handle_review_info_mode(app)?;
        }

        // Branch selector mode
        AppMode::BranchSelector(_) => {
            picker::handle_branch_selector_mode(app, code)?;
        }

        // Git operation confirmation modes
        AppMode::ConfirmPush(_) => {
            confirm::handle_confirm_push_mode(app, code)?;
        }
        AppMode::ConfirmPushForPR(_) => {
            confirm::handle_confirm_push_for_pr_mode(app, code)?;
        }
        AppMode::RenameBranch(_) => {
            confirm::handle_rename_branch_mode(app, code)?;
        }

        // General confirmation mode
        AppMode::Confirming(state) => {
            confirm::handle_confirming_mode(app, state.action, code)?;
        }

        // Rebase/Merge branch selector modes
        AppMode::RebaseBranchSelector(_) => {
            picker::handle_rebase_branch_selector_mode(app, code)?;
        }
        AppMode::MergeBranchSelector(_) => {
            picker::handle_merge_branch_selector_mode(app, code)?;
        }

        // Keyboard remap prompt
        AppMode::KeyboardRemapPrompt(_) => {
            confirm::handle_keyboard_remap_mode(app, code)?;
        }
        // Self-update prompt on startup
        AppMode::UpdatePrompt(state) => {
            let info = state.info.clone();
            confirm::handle_update_prompt_mode(app, &info, code)?;
        }
        // Update requested - ignore input while exiting
        AppMode::UpdateRequested(_) => {}

        // Help, error, and success modes
        AppMode::Help(_) => {
            crate::action::dispatch_help_mode(app, code, modifiers)?;
        }
        AppMode::ErrorModal(state) => {
            crate::action::dispatch_error_modal_mode(app, state.message.clone())?;
        }
        AppMode::SuccessModal(state) => {
            crate::action::dispatch_success_modal_mode(app, state.message.clone())?;
        }

        // Slash commands
        AppMode::CommandPalette(_) => {
            command::handle_command_palette_mode(app, code)?;
        }

        // Slash command modal/pickers
        AppMode::ModelSelector(_) => {
            command::handle_model_selector_mode(app, code)?;
        }

        // Preview focused mode (forwards keys to the mux backend)
        AppMode::PreviewFocused(_) => {
            crate::action::dispatch_preview_focused_mode(app, code, modifiers, batched_keys)?;
        }

        // Normal and scrolling modes
        AppMode::Normal(_) | AppMode::Scrolling(_) => {
            normal::handle_normal_mode(app, code, modifiers)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::*;
    use crate::update::UpdateInfo;
    use ratatui::crossterm::event::KeyCode;
    use semver::Version;
    use tempfile::NamedTempFile;

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
        app.mode = AppMode::Help(HelpMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_error_modal_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.set_error("test error");
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_success_modal_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.show_success("success!");
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char(' '),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ConfirmPush(ConfirmPushMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ConfirmPushForPR(ConfirmPushForPRMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::RenameBranch(RenameBranchMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
        assert_eq!(app.data.input.buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_keyboard_remap_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::KeyboardRemapPrompt(KeyboardRemapPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert!(app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_update_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = AppMode::UpdatePrompt(UpdatePromptMode { info });
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_update_requested_mode_ignores_input() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = AppMode::UpdateRequested(UpdateRequestedMode { info });
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        // Should remain in UpdateRequested mode - input is ignored
        assert!(matches!(&app.mode, AppMode::UpdateRequested(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Confirming(ConfirmingMode {
            action: ConfirmAction::Quit,
        });
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Creating(CreatingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('t'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        assert_eq!(app.data.input.buffer, "t");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Prompting(PromptingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ChildCount(ChildCountMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ReviewChildCount(ReviewChildCountMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_info_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ReviewInfo(ReviewInfoMode);
        let mut batched_keys = Vec::new();

        // ReviewInfo mode exits on any key
        handle_key_event(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        // Should exit to Normal mode
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::BranchSelector(BranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rebase_branch_selector_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::RebaseBranchSelector(RebaseBranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_merge_branch_selector_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::MergeBranchSelector(MergeBranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Broadcasting(BroadcastingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('h'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
        assert_eq!(app.data.input.buffer, "h");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_reconnect_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ReconnectPrompt(ReconnectPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::ChildPrompt(ChildPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::ChildPrompt(ChildPromptMode));
        assert_eq!(app.data.input.buffer, "x");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_terminal_prompt_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::TerminalPrompt(TerminalPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Scrolling(ScrollingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::normal();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('?'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_scroll_does_not_exit() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Down,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 1);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_scroll_up_from_bottom_is_immediate() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = usize::MAX;

        let max_scroll = crate::action::help_max_scroll(&app.data);
        assert_ne!(max_scroll, 0, "help should be scrollable for this test");

        let mut batched_keys = Vec::new();

        handle_key_event(&mut app, KeyCode::Up, KeyModifiers::NONE, &mut batched_keys)?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, max_scroll.saturating_sub(1));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_page_down() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::PageDown,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert!(app.data.ui.help_scroll > 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_page_up() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::PageUp,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_ctrl_d() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert!(app.data.ui.help_scroll > 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_ctrl_u() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 5);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_go_to_top() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('g'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_go_to_bottom() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('G'),
            KeyModifiers::SHIFT,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        let max_scroll = crate::action::help_max_scroll(&app.data);
        assert_eq!(app.data.ui.help_scroll, max_scroll);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_home_key() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Home,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_end_key() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::End,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        let max_scroll = crate::action::help_max_scroll(&app.data);
        assert_eq!(app.data.ui.help_scroll, max_scroll);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_any_other_key_exits() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }
}
