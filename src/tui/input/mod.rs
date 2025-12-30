//! Mode-specific key handling
//!
//! This module provides a unified way to handle keyboard input based on the current mode.
//! Each mode has its own handler that implements the `ModeKeyHandler` trait.

mod command;
mod confirm;
mod normal;
mod picker;
mod preview_focused;
mod text_input;

use crate::app::{Actions, App, Mode};
use crate::config::{Action as KeyAction, ActionGroup};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

// Re-export for tests and internal use
#[cfg(test)]
pub use preview_focused::keycode_to_input_sequence;

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
        | Mode::TerminalPrompt
        | Mode::CustomAgentCommand => {
            text_input::handle_text_input_mode(app, action_handler, code, modifiers)?;
        }

        // Count picker modes
        Mode::ChildCount => {
            picker::handle_child_count_mode(app, action_handler, code)?;
        }
        Mode::ReviewChildCount => {
            picker::handle_review_child_count_mode(app, action_handler, code)?;
        }
        Mode::ReviewInfo => {
            picker::handle_review_info_mode(app, action_handler)?;
        }

        // Branch selector mode
        Mode::BranchSelector => {
            picker::handle_branch_selector_mode(app, action_handler, code)?;
        }

        // Git operation confirmation modes
        Mode::ConfirmPush => {
            confirm::handle_confirm_push_mode(app, action_handler, code)?;
        }
        Mode::ConfirmPushForPR => {
            confirm::handle_confirm_push_for_pr_mode(app, action_handler, code)?;
        }
        Mode::RenameBranch => {
            confirm::handle_rename_branch_mode(app, action_handler, code)?;
        }

        // General confirmation mode
        Mode::Confirming(action) => {
            confirm::handle_confirming_mode(app, action_handler, *action, code)?;
        }

        // Rebase/Merge branch selector modes
        Mode::RebaseBranchSelector => {
            picker::handle_rebase_branch_selector_mode(app, action_handler, code)?;
        }
        Mode::MergeBranchSelector => {
            picker::handle_merge_branch_selector_mode(app, action_handler, code)?;
        }

        // Keyboard remap prompt
        Mode::KeyboardRemapPrompt => {
            confirm::handle_keyboard_remap_mode(app, action_handler, code)?;
        }
        // Self-update prompt on startup
        Mode::UpdatePrompt(info) => {
            confirm::handle_update_prompt_mode(app, action_handler, info.clone(), code)?;
        }
        // Update requested - ignore input while exiting
        Mode::UpdateRequested(_) => {}

        // Help, error, and success modes
        Mode::Help => {
            let max_scroll = help_max_scroll(app);
            // Clamp any out-of-range scroll immediately (fixes "dead zone" when scroll is usize::MAX).
            app.ui.help_scroll = app.ui.help_scroll.min(max_scroll);
            match (code, modifiers) {
                // Scroll help content (does not close help)
                (KeyCode::Up, _) => {
                    app.ui.help_scroll = app.ui.help_scroll.saturating_sub(1).min(max_scroll);
                }
                (KeyCode::Down, _) => {
                    app.ui.help_scroll = app.ui.help_scroll.saturating_add(1).min(max_scroll);
                }
                (KeyCode::PageUp, _) => {
                    app.ui.help_scroll = app.ui.help_scroll.saturating_sub(10).min(max_scroll);
                }
                (KeyCode::PageDown, _) => {
                    app.ui.help_scroll = app.ui.help_scroll.saturating_add(10).min(max_scroll);
                }
                (KeyCode::Char('u'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                    app.ui.help_scroll = app.ui.help_scroll.saturating_sub(5).min(max_scroll);
                }
                (KeyCode::Char('d'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                    app.ui.help_scroll = app.ui.help_scroll.saturating_add(5).min(max_scroll);
                }
                (KeyCode::Char('g') | KeyCode::Home, _) => {
                    app.ui.help_scroll = 0;
                }
                (KeyCode::Char('G') | KeyCode::End, _) => {
                    app.ui.help_scroll = max_scroll;
                }

                // Any non-scroll key (Esc, q, ?, Enter, etc.) closes help
                _ => app.exit_mode(),
            }
        }
        Mode::ErrorModal(_) => {
            app.dismiss_error();
        }
        Mode::SuccessModal(_) => {
            picker::handle_success_modal_mode(app);
        }

        // Slash commands
        Mode::CommandPalette => {
            command::handle_command_palette_mode(app, action_handler, code)?;
        }

        // Slash command modal/pickers
        Mode::ModelSelector => {
            command::handle_model_selector_mode(app, action_handler, code)?;
        }

        // Preview focused mode (forwards keys to the mux backend)
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

/// Compute the total number of lines in the help overlay content.
///
/// This is used for scroll normalization in Help mode.
fn help_total_lines() -> usize {
    let mut group_count = 0usize;
    let mut last_group: Option<ActionGroup> = None;
    for &action in KeyAction::ALL_FOR_HELP {
        let group = action.group();
        if Some(group) != last_group {
            group_count = group_count.saturating_add(1);
            last_group = Some(group);
        }
    }

    // Content structure:
    // - Header: 2 lines ("Keybindings" + blank)
    // - Groups: each group adds a header line, and each transition adds an extra blank line
    // - Actions: 1 line per action
    // - Footer: blank line + 2 footer lines
    KeyAction::ALL_FOR_HELP
        .len()
        .saturating_add(group_count.saturating_mul(2))
        .saturating_add(4)
}

/// Compute the maximum scroll offset for the help overlay based on terminal height.
fn help_max_scroll(app: &App) -> usize {
    let total_lines = help_total_lines();

    // The help overlay uses `frame.area().height.saturating_sub(4)` as its max height.
    // `preview_dimensions` stores the preview inner height, which is also `frame_height - 4`.
    let max_height = usize::from(app.ui.preview_dimensions.map_or(20, |(_, h)| h));
    let min_height = 12usize.min(max_height);
    let desired_height = total_lines.saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    let visible_height = height.saturating_sub(2);
    total_lines.saturating_sub(visible_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{ConfirmAction, Settings};
    use crate::config::Config;
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

    #[test]
    fn test_handle_key_event_help_mode_scroll_does_not_exit() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 0;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Down,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.ui.help_scroll, 1);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_scroll_up_from_bottom_is_immediate() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = usize::MAX;

        let max_scroll = help_max_scroll(&app);
        assert_ne!(max_scroll, 0, "help should be scrollable for this test");

        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Up,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.ui.help_scroll, max_scroll.saturating_sub(1));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_page_down() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 0;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::PageDown,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert!(app.ui.help_scroll > 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_page_up() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 10;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::PageUp,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_ctrl_d() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 0;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert!(app.ui.help_scroll > 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_ctrl_u() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 10;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.ui.help_scroll, 5);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_go_to_top() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 10;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('g'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_go_to_bottom() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 0;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Char('G'),
            KeyModifiers::SHIFT,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        let max_scroll = help_max_scroll(&app);
        assert_eq!(app.ui.help_scroll, max_scroll);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_home_key() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 10;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::Home,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.ui.help_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_end_key() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 0;
        let action_handler = Actions::new();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            action_handler,
            KeyCode::End,
            KeyModifiers::NONE,
            &mut batched_keys,
        )?;

        assert_eq!(app.mode, Mode::Help);
        let max_scroll = help_max_scroll(&app);
        assert_eq!(app.ui.help_scroll, max_scroll);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_any_other_key_exits() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Help;
        app.ui.help_scroll = 0;
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
}
