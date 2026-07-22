//! Mode-specific key handling
//!
//! This module provides a unified way to handle keyboard input based on the current mode.
//! Each mode has its own handler that implements the `ModeKeyHandler` trait.

mod command;
mod confirm;
mod mouse;
mod normal;
mod picker;
mod text_input;

use crate::app::App;
use crate::state::AppMode;
use anyhow::Result;
use ratatui::{
    crossterm::event::{KeyCode, KeyModifiers, MouseEvent},
    layout::Rect,
};

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
        | AppMode::CustomAgentCommand(_)
        | AppMode::SynthesisPrompt(_) => {
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
        AppMode::SwitchBranchSelector(_) => {
            picker::handle_switch_branch_selector_mode(app, code)?;
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
        // Ignore input while the app is busy with a blocking background step.
        AppMode::UpdateRequested(_) | AppMode::PreparingDocker(_) => {}

        // Help, error, and success modes
        AppMode::Changelog(state) => {
            let mark_seen_version = state.mark_seen_version.clone();
            let max_scroll = crate::action::changelog_max_scroll(&app.data, state);
            crate::action::dispatch_changelog_mode(
                app,
                mark_seen_version,
                max_scroll,
                code,
                modifiers,
            )?;
        }
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
        AppMode::SettingsMenu(_) => {
            command::handle_settings_menu_mode(app, code)?;
        }

        // Preview focused mode (forwards keys to the mux backend)
        AppMode::PreviewFocused(_) => {
            crate::action::dispatch_preview_focused_mode(app, code, modifiers, batched_keys)?;
        }
        // Diff focused mode (interactive diff view)
        AppMode::DiffFocused(_) => {
            crate::action::dispatch_diff_focused_mode(app, code, modifiers)?;
        }

        // Normal and scrolling modes
        AppMode::Normal(_) | AppMode::Scrolling(_) => {
            normal::handle_normal_mode(app, code, modifiers)?;
        }
    }
    Ok(())
}

/// Handle a mouse event based on the current mode and layout.
///
/// `frame_area` should be the terminal viewport (`Rect::new(0, 0, width, height)`).
pub fn handle_mouse_event(
    app: &mut App,
    mouse: MouseEvent,
    frame_area: Rect,
    batched_keys: &mut Vec<String>,
) {
    mouse::handle_mouse_event(app, mouse, frame_area, batched_keys);
}
