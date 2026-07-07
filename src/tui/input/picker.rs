//! Picker mode key handling
//!
//! Handles key events for modes that involve picking/selecting:
//! - `ChildCount` (selecting number of child agents)
//! - `ReviewChildCount` (selecting number of review agents)
//! - `ReviewInfo` (info popup before review)
//! - `BranchSelector` (selecting a branch)
//! - `RebaseBranchSelector` (selecting a rebase target)
//! - `MergeBranchSelector` (selecting a merge source)
//! - `SwitchBranchSelector` (selecting a branch to switch to)

use crate::app::App;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `ChildCount` mode
pub fn handle_child_count_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_child_count_mode(app, code)
}

/// Handle key events in `ReviewChildCount` mode
pub fn handle_review_child_count_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_review_child_count_mode(app, code)
}

/// Handle key events in `ReviewInfo` mode (any key dismisses)
pub fn handle_review_info_mode(app: &mut App) -> Result<()> {
    crate::action::dispatch_review_info_mode(app)
}

/// Handle key events in `BranchSelector` mode
pub fn handle_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_branch_selector_mode(app, code)
}

/// Handle key events in `RebaseBranchSelector` mode
pub fn handle_rebase_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_rebase_branch_selector_mode(app, code)
}

/// Handle key events in `MergeBranchSelector` mode
pub fn handle_merge_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_merge_branch_selector_mode(app, code)
}

/// Handle key events in `SwitchBranchSelector` mode
pub fn handle_switch_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_switch_branch_selector_mode(app, code)
}

#[cfg(test)]
mod tests;
