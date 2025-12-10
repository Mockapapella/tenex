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

        // Help and error modes (dismiss on any key)
        Mode::Help => {
            app.exit_mode();
        }
        Mode::ErrorModal(_) => {
            app.dismiss_error();
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
