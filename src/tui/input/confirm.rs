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
