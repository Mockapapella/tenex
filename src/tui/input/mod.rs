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

use crate::app::{Actions, App, BranchPickerKind, ConfirmKind, CountPickerKind, Mode, OverlayMode};
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
        Mode::Overlay(overlay) => match overlay {
            // Text input overlays
            OverlayMode::TextInput(kind) => {
                text_input::handle_text_input_mode(app, action_handler, *kind, code, modifiers);
            }

            // Count pickers
            OverlayMode::CountPicker(CountPickerKind::ChildCount) => {
                picker::handle_child_count_mode(app, code);
            }
            OverlayMode::CountPicker(CountPickerKind::ReviewChildCount) => {
                picker::handle_review_child_count_mode(app, code);
            }
            OverlayMode::ReviewInfo => {
                picker::handle_review_info_mode(app);
            }

            // Branch pickers
            OverlayMode::BranchPicker(BranchPickerKind::ReviewBaseBranch) => {
                picker::handle_branch_selector_mode(app, action_handler, code);
            }
            OverlayMode::BranchPicker(BranchPickerKind::RebaseTargetBranch) => {
                picker::handle_rebase_branch_selector_mode(app, code);
            }
            OverlayMode::BranchPicker(BranchPickerKind::MergeFromBranch) => {
                picker::handle_merge_branch_selector_mode(app, code);
            }

            // Confirmation overlays
            OverlayMode::Confirm(ConfirmKind::Push) => {
                confirm::handle_confirm_push_mode(app, code);
            }
            OverlayMode::Confirm(ConfirmKind::PushForPR) => {
                confirm::handle_confirm_push_for_pr_mode(app, code);
            }
            OverlayMode::Confirm(ConfirmKind::Action(action)) => {
                confirm::handle_confirming_mode(app, action_handler, *action, code)?;
            }
            OverlayMode::Confirm(ConfirmKind::KeyboardRemap) => {
                confirm::handle_keyboard_remap_mode(app, code);
            }
            OverlayMode::Confirm(ConfirmKind::UpdatePrompt(info)) => {
                confirm::handle_update_prompt_mode(app, info.clone(), code);
            }

            // Help, error, and success overlays
            OverlayMode::Help => {
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
            OverlayMode::Error(_) => {
                app.dismiss_error();
            }
            OverlayMode::Success(_) => {
                picker::handle_success_modal_mode(app);
            }

            // Slash commands
            OverlayMode::CommandPalette => {
                command::handle_command_palette_mode(app, code);
            }

            // Slash command modal/pickers
            OverlayMode::ModelSelector => {
                command::handle_model_selector_mode(app, code);
            }
        },

        // Update requested - ignore input while exiting
        Mode::UpdateRequested(_) => {}

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
mod tests;
