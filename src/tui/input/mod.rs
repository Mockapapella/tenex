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

#[derive(Clone, Copy)]
struct KeyEventHandlers {
    text_input: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
    child_count: fn(&mut App, KeyCode) -> Result<()>,
    review_child_count: fn(&mut App, KeyCode) -> Result<()>,
    review_info: fn(&mut App) -> Result<()>,
    branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    confirm_push: fn(&mut App, KeyCode) -> Result<()>,
    confirm_push_for_pr: fn(&mut App, KeyCode) -> Result<()>,
    rename_branch: fn(&mut App, KeyCode) -> Result<()>,
    confirming: fn(&mut App, crate::state::ConfirmAction, KeyCode) -> Result<()>,
    rebase_branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    merge_branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    switch_branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    keyboard_remap: fn(&mut App, KeyCode) -> Result<()>,
    update_prompt: fn(&mut App, &crate::update::UpdateInfo, KeyCode) -> Result<()>,
    dispatch_changelog:
        fn(&mut App, Option<semver::Version>, usize, KeyCode, KeyModifiers) -> Result<()>,
    dispatch_help: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
    dispatch_error_modal: fn(&mut App, String) -> Result<()>,
    dispatch_success_modal: fn(&mut App, String) -> Result<()>,
    command_palette: fn(&mut App, KeyCode) -> Result<()>,
    model_selector: fn(&mut App, KeyCode) -> Result<()>,
    settings_menu: fn(&mut App, KeyCode) -> Result<()>,
    dispatch_preview_focused: fn(&mut App, KeyCode, KeyModifiers, &mut Vec<String>) -> Result<()>,
    dispatch_diff_focused: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
    normal: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
}

const KEY_EVENT_HANDLERS: KeyEventHandlers = KeyEventHandlers {
    text_input: text_input::handle_text_input_mode,
    child_count: picker::handle_child_count_mode,
    review_child_count: picker::handle_review_child_count_mode,
    review_info: picker::handle_review_info_mode,
    branch_selector: picker::handle_branch_selector_mode,
    confirm_push: confirm::handle_confirm_push_mode,
    confirm_push_for_pr: confirm::handle_confirm_push_for_pr_mode,
    rename_branch: confirm::handle_rename_branch_mode,
    confirming: confirm::handle_confirming_mode,
    rebase_branch_selector: picker::handle_rebase_branch_selector_mode,
    merge_branch_selector: picker::handle_merge_branch_selector_mode,
    switch_branch_selector: picker::handle_switch_branch_selector_mode,
    keyboard_remap: confirm::handle_keyboard_remap_mode,
    update_prompt: confirm::handle_update_prompt_mode,
    dispatch_changelog: crate::action::dispatch_changelog_mode,
    dispatch_help: crate::action::dispatch_help_mode,
    dispatch_error_modal: crate::action::dispatch_error_modal_mode,
    dispatch_success_modal: crate::action::dispatch_success_modal_mode,
    command_palette: command::handle_command_palette_mode,
    model_selector: command::handle_model_selector_mode,
    settings_menu: command::handle_settings_menu_mode,
    dispatch_preview_focused: crate::action::dispatch_preview_focused_mode,
    dispatch_diff_focused: crate::action::dispatch_diff_focused_mode,
    normal: normal::handle_normal_mode,
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
    handle_key_event_with_handlers(app, code, modifiers, batched_keys, KEY_EVENT_HANDLERS)
}

fn handle_key_event_with_handlers(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
    handlers: KeyEventHandlers,
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
            (handlers.text_input)(app, code, modifiers)?;
        }

        // Count picker modes
        AppMode::ChildCount(_) => {
            (handlers.child_count)(app, code)?;
        }
        AppMode::ReviewChildCount(_) => {
            (handlers.review_child_count)(app, code)?;
        }
        AppMode::ReviewInfo(_) => {
            (handlers.review_info)(app)?;
        }

        // Branch selector mode
        AppMode::BranchSelector(_) => {
            (handlers.branch_selector)(app, code)?;
        }

        // Git operation confirmation modes
        AppMode::ConfirmPush(_) => {
            (handlers.confirm_push)(app, code)?;
        }
        AppMode::ConfirmPushForPR(_) => {
            (handlers.confirm_push_for_pr)(app, code)?;
        }
        AppMode::RenameBranch(_) => {
            (handlers.rename_branch)(app, code)?;
        }

        // General confirmation mode
        AppMode::Confirming(state) => {
            (handlers.confirming)(app, state.action, code)?;
        }

        // Rebase/Merge branch selector modes
        AppMode::RebaseBranchSelector(_) => {
            (handlers.rebase_branch_selector)(app, code)?;
        }
        AppMode::MergeBranchSelector(_) => {
            (handlers.merge_branch_selector)(app, code)?;
        }
        AppMode::SwitchBranchSelector(_) => {
            (handlers.switch_branch_selector)(app, code)?;
        }

        // Keyboard remap prompt
        AppMode::KeyboardRemapPrompt(_) => {
            (handlers.keyboard_remap)(app, code)?;
        }
        // Self-update prompt on startup
        AppMode::UpdatePrompt(state) => {
            let info = state.info.clone();
            (handlers.update_prompt)(app, &info, code)?;
        }
        // Ignore input while the app is busy with a blocking background step.
        AppMode::UpdateRequested(_) | AppMode::PreparingDocker(_) => {}

        // Help, error, and success modes
        AppMode::Changelog(state) => {
            let mark_seen_version = state.mark_seen_version.clone();
            let max_scroll = crate::action::changelog_max_scroll(&app.data, state);
            (handlers.dispatch_changelog)(app, mark_seen_version, max_scroll, code, modifiers)?;
        }
        AppMode::Help(_) => {
            (handlers.dispatch_help)(app, code, modifiers)?;
        }
        AppMode::ErrorModal(state) => {
            (handlers.dispatch_error_modal)(app, state.message.clone())?;
        }
        AppMode::SuccessModal(state) => {
            (handlers.dispatch_success_modal)(app, state.message.clone())?;
        }

        // Slash commands
        AppMode::CommandPalette(_) => {
            (handlers.command_palette)(app, code)?;
        }

        // Slash command modal/pickers
        AppMode::ModelSelector(_) => {
            (handlers.model_selector)(app, code)?;
        }
        AppMode::SettingsMenu(_) => {
            (handlers.settings_menu)(app, code)?;
        }

        // Preview focused mode (forwards keys to the mux backend)
        AppMode::PreviewFocused(_) => {
            (handlers.dispatch_preview_focused)(app, code, modifiers, batched_keys)?;
        }
        // Diff focused mode (interactive diff view)
        AppMode::DiffFocused(_) => {
            (handlers.dispatch_diff_focused)(app, code, modifiers)?;
        }

        // Normal and scrolling modes
        AppMode::Normal(_) | AppMode::Scrolling(_) => {
            (handlers.normal)(app, code, modifiers)?;
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

#[cfg(test)]
mod tests;
