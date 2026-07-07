//! Modal rendering utilities and implementations
//!
//! This module provides a unified way to render modal dialogs, reducing
//! duplication across the various overlay rendering functions.

mod branch;
mod changelog;
mod command_palette;
mod confirm;
mod error;
mod help;
mod input;
mod models;
mod picker;
mod progress;
mod settings_menu;

pub use branch::render_branch_selector_overlay;
pub use changelog::render_changelog_overlay;
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
pub use progress::render_preparing_docker_modal;
pub use settings_menu::render_settings_menu_overlay;

use crate::app::App;
use crate::config::Action;
use crate::state::{AppMode, ConfirmAction};
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

/// Compute the on-screen rectangle used by the currently active modal/overlay (if any).
///
/// This mirrors the sizing logic in the corresponding render functions so input handlers can
/// implement click-to-dismiss behavior without storing layout state.
#[must_use]
pub fn modal_rect_for_mode(app: &App, frame_area: Rect) -> Option<Rect> {
    match &app.mode {
        AppMode::Changelog(state) => Some(changelog_rect(state, frame_area)),
        AppMode::Help(_) => Some(help_rect(app, frame_area)),
        AppMode::CommandPalette(_) => Some(command_palette_rect(app, frame_area)),
        AppMode::Creating(_)
        | AppMode::Prompting(_)
        | AppMode::ChildPrompt(_)
        | AppMode::Broadcasting(_)
        | AppMode::ReconnectPrompt(_)
        | AppMode::TerminalPrompt(_)
        | AppMode::CustomAgentCommand(_)
        | AppMode::SynthesisPrompt(_) => Some(text_input_rect(app, frame_area)),
        AppMode::ChildCount(_) | AppMode::ReviewChildCount(_) => {
            Some(centered_rect_absolute(40, 12, frame_area))
        }
        AppMode::ReviewInfo(_) => Some(centered_rect_absolute(50, 9, frame_area)),
        AppMode::BranchSelector(_)
        | AppMode::RebaseBranchSelector(_)
        | AppMode::MergeBranchSelector(_)
        | AppMode::SwitchBranchSelector(_) => Some(centered_rect_absolute(60, 20, frame_area)),
        AppMode::ModelSelector(_) => Some(centered_rect_absolute(55, 12, frame_area)),
        AppMode::SettingsMenu(_) => Some(centered_rect_absolute(60, 9, frame_area)),
        AppMode::ConfirmPush(_) => Some(confirm_push_rect(app, frame_area)),
        AppMode::RenameBranch(_) => Some(centered_rect_absolute(55, 9, frame_area)),
        AppMode::ConfirmPushForPR(_) | AppMode::UpdatePrompt(_) => {
            Some(centered_rect_absolute(55, 11, frame_area))
        }
        AppMode::KeyboardRemapPrompt(_) => Some(centered_rect_absolute(55, 16, frame_area)),
        AppMode::PreparingDocker(state) => Some(success_modal_rect(&state.message, frame_area)),
        AppMode::ErrorModal(state) => Some(error_modal_rect(&state.message, frame_area)),
        AppMode::SuccessModal(state) => Some(success_modal_rect(&state.message, frame_area)),
        AppMode::Confirming(state) => Some(confirming_rect(app, state.action, frame_area)),
        _ => None,
    }
}

pub(in crate::tui) fn changelog_modal_rect(
    state: &crate::state::ChangelogMode,
    frame_area: Rect,
) -> Rect {
    changelog_rect(state, frame_area)
}

fn changelog_rect(state: &crate::state::ChangelogMode, frame_area: Rect) -> Rect {
    let total_lines = state.lines.len();

    let max_height = frame_area.height.saturating_sub(4);
    let min_height = 12u16.min(max_height);
    let desired_height = u16::try_from(total_lines)
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    centered_rect_absolute(60, height, frame_area)
}

fn help_rect(app: &App, frame_area: Rect) -> Rect {
    // Mirror `render_help_overlay`'s line-count and sizing logic.
    let _merge_key_remapped = app.is_merge_key_remapped();

    let mut total_lines: usize = 2; // "Keybindings" + blank
    let mut current_group = None;
    for &action in Action::ALL_FOR_HELP {
        let group = action.group();
        if current_group != Some(group) {
            if current_group.is_some() {
                total_lines = total_lines.saturating_add(1); // blank between groups
            }
            total_lines = total_lines.saturating_add(1); // group title line
            current_group = Some(group);
        }

        // One line per action entry. The actual text varies, but line count does not.
        total_lines = total_lines.saturating_add(1);
    }

    // Footer: blank + 2 hint lines
    total_lines = total_lines.saturating_add(3);

    let max_height = frame_area.height.saturating_sub(4);
    let min_height = 12u16.min(max_height);
    let desired_height = u16::try_from(total_lines)
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    centered_rect_absolute(50, height, frame_area)
}

fn command_palette_rect(app: &App, frame_area: Rect) -> Rect {
    let filtered = app.filtered_slash_commands();
    let total_count = filtered.len();

    let max_visible: usize = 8;
    let visible_count = total_count.min(max_visible).max(1);
    let visible_count_u16 = u16::try_from(visible_count).unwrap_or(0);

    // Header + blank + list + blank + help
    let content_height = 1u16 + 1u16 + visible_count_u16 + 1u16 + 1u16;
    let total_height = content_height.saturating_add(2); // borders

    centered_rect_absolute(60, total_height, frame_area)
}

fn text_input_rect(app: &App, frame_area: Rect) -> Rect {
    // Mirror `render_input_overlay`'s dynamic sizing logic; title/prompt do not affect layout.
    let input = app.data.input.buffer.as_str();
    let cursor_pos = app.data.input.cursor;
    let text_with_cursor = if cursor_pos >= input.len() {
        format!("{input}│")
    } else {
        let before = &input[..cursor_pos];
        let after = &input[cursor_pos..];
        format!("{before}│{after}")
    };

    let min_input_height = 3_usize;
    let max_input_height = 20_usize;
    let modal_width = centered_rect_absolute(60, 1, frame_area).width;
    let mut inner_width = modal_width.saturating_sub(2).max(1);

    let mut input_lines =
        input::wrap_input_with_cursor(&text_with_cursor, usize::from(inner_width)).0;
    let mut input_area_height = input_lines.len().clamp(min_input_height, max_input_height);
    let mut needs_scrollbar = input_lines.len() > input_area_height;

    if needs_scrollbar {
        inner_width = modal_width.saturating_sub(3).max(1);
        input_lines = input::wrap_input_with_cursor(&text_with_cursor, usize::from(inner_width)).0;
        input_area_height = input_lines.len().clamp(min_input_height, max_input_height);
        needs_scrollbar = input_lines.len() > input_area_height;
    }

    // Total height: borders(2) + prompt(1) + empty(1) + input area + empty(1) + help(1)
    let total_height = u16::try_from(6 + input_area_height).unwrap_or(u16::MAX);
    let _ = needs_scrollbar; // Keep parity with render logic for future changes.

    centered_rect_absolute(60, total_height, frame_area)
}

fn confirm_push_rect(app: &App, frame_area: Rect) -> Rect {
    let agent_present = app
        .data
        .git_op
        .agent_id
        .and_then(|id| app.data.storage.get(id))
        .is_some();

    let lines = if agent_present { 6 } else { 5 };
    let height = u16::try_from(lines + 2).unwrap_or(u16::MAX);
    centered_rect_absolute(50, height, frame_area)
}

fn error_modal_rect(message: &str, frame_area: Rect) -> Rect {
    let wrapped = word_wrap_line_count(message, 44);
    let lines = wrapped.saturating_add(4);
    let height = u16::try_from(lines + 2).unwrap_or(u16::MAX).max(7);
    centered_rect_absolute(50, height, frame_area)
}

fn success_modal_rect(message: &str, frame_area: Rect) -> Rect {
    let wrapped = word_wrap_line_count(message, 44);
    let lines = wrapped.saturating_add(4);
    let height = u16::try_from(lines + 2).unwrap_or(u16::MAX).max(7);
    centered_rect_absolute(50, height, frame_area)
}

fn word_wrap_line_count(message: &str, max_line_width: usize) -> usize {
    let mut line_count = 0usize;
    let mut current_len = 0usize;

    for word in message.split_whitespace() {
        let word_len = word.len();
        if current_len == 0 {
            current_len = word_len;
            line_count = line_count.saturating_add(1);
        } else if current_len.saturating_add(1).saturating_add(word_len) <= max_line_width {
            current_len = current_len.saturating_add(1).saturating_add(word_len);
        } else {
            current_len = word_len;
            line_count = line_count.saturating_add(1);
        }
    }

    line_count
}

fn confirming_rect(app: &App, action: ConfirmAction, frame_area: Rect) -> Rect {
    match action {
        ConfirmAction::WorktreeConflict => {
            let conflict = app.data.spawn.worktree_conflict.as_ref();
            let existing_branch = conflict.and_then(|c| c.existing_branch.as_ref()).is_some();
            let existing_commit = conflict.and_then(|c| c.existing_commit.as_ref()).is_some();
            let base_lines = 16usize
                .saturating_add(usize::from(existing_branch))
                .saturating_add(usize::from(existing_commit));
            let height = u16::try_from(base_lines + 2).unwrap_or(u16::MAX);
            centered_rect_absolute(60, height, frame_area)
        }
        ConfirmAction::Kill | ConfirmAction::InterruptAgent => {
            let lines = if app.data.selected_agent().is_some() {
                7
            } else {
                1
            };
            confirm_overlay_rect(lines, frame_area)
        }
        ConfirmAction::Reset | ConfirmAction::Quit => confirm_overlay_rect(1, frame_area),
        ConfirmAction::RestartMuxDaemon => {
            let lines = app
                .data
                .ui
                .muxd_version_mismatch
                .as_ref()
                .map_or(1, |info| {
                    7usize.saturating_add(usize::from(info.env_mux_socket.is_some()))
                });
            confirm_overlay_rect(lines, frame_area)
        }
        ConfirmAction::Synthesize => {
            let lines = if app.data.selected_agent().is_some() {
                6
            } else {
                1
            };
            confirm_overlay_rect(lines, frame_area)
        }
        ConfirmAction::SwitchBranch => confirm_overlay_rect(7, frame_area),
    }
}

fn confirm_overlay_rect(base_lines: usize, frame_area: Rect) -> Rect {
    // `render_confirm_overlay` appends 2 lines for the prompt, then adds 2 border lines.
    let lines = base_lines.saturating_add(2);
    let height = u16::try_from(lines + 2).unwrap_or(u16::MAX);
    centered_rect_absolute(50, height, frame_area)
}

#[cfg(test)]
mod tests;
