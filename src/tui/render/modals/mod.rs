//! Modal rendering utilities and implementations
//!
//! This module provides a unified way to render modal dialogs, reducing
//! duplication across the various overlay rendering functions.

mod branch;
mod command_palette;
mod confirm;
mod error;
mod help;
mod input;
mod models;
mod picker;

pub use branch::render_branch_selector_overlay;
pub use command_palette::render_command_palette_overlay;
pub use confirm::{
    render_confirm_overlay, render_confirm_push_for_pr_overlay, render_confirm_push_overlay,
    render_keyboard_remap_overlay, render_update_prompt_overlay, render_worktree_conflict_overlay,
};
pub use error::{render_error_modal, render_success_modal};
pub use help::render_help_overlay;
pub use input::{render_input_overlay, render_rename_overlay};
pub use models::render_model_selector_overlay;
pub use picker::{
    render_count_picker_overlay, render_review_count_picker_overlay, render_review_info_overlay,
};

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Create a centered rect with percentage width and absolute height
pub fn centered_rect_absolute(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical_padding = area.height.saturating_sub(height) / 2;
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(vertical_padding),
            Constraint::Length(height),
            Constraint::Length(vertical_padding),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
