//! Modal/overlay action types (new architecture).

use super::{
    DismissAction, ScrollBottomAction, ScrollDownAction, ScrollTopAction, ScrollUpAction, ValidIn,
};
use crate::app::AppData;
use crate::config::{Action as KeyAction, ActionGroup};
use crate::state::{AppMode, ErrorModalMode, HelpMode, SuccessModalMode};
use anyhow::Result;

/// Help-mode action: page up (`PgUp`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PageUpAction;

/// Help-mode action: page down (`PgDn`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PageDownAction;

/// Help-mode action: half-page up (`Ctrl+u`).
#[derive(Debug, Clone, Copy, Default)]
pub struct HalfPageUpAction;

/// Help-mode action: half-page down (`Ctrl+d`).
#[derive(Debug, Clone, Copy, Default)]
pub struct HalfPageDownAction;

/// Compute the total number of lines in the help overlay content.
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
///
/// This mirrors the sizing logic used in `src/tui/render/modals/help.rs`, but uses the most
/// recently known preview height stored in `data.ui.preview_dimensions` since actions do not have
/// access to the render `Frame`.
#[must_use]
pub fn help_max_scroll(data: &AppData) -> usize {
    let total_lines = help_total_lines();

    // The help overlay uses `frame.area().height.saturating_sub(4)` as its max height.
    // `preview_dimensions` stores the preview inner height, which is also `frame_height - 4`.
    let max_height = usize::from(data.ui.preview_dimensions.map_or(20, |(_, h)| h));
    let min_height = 12usize.min(max_height);
    let desired_height = total_lines.saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    let visible_height = height.saturating_sub(2);
    total_lines.saturating_sub(visible_height)
}

fn clamp_help_scroll(app_data: &mut AppData) -> usize {
    let max_scroll = help_max_scroll(app_data);
    app_data.ui.help_scroll = app_data.ui.help_scroll.min(max_scroll);
    max_scroll
}

impl ValidIn<HelpMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = app_data.ui.help_scroll.saturating_sub(1).min(max_scroll);
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = app_data.ui.help_scroll.saturating_add(1).min(max_scroll);
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for PageUpAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = app_data.ui.help_scroll.saturating_sub(10).min(max_scroll);
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for PageDownAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = app_data.ui.help_scroll.saturating_add(10).min(max_scroll);
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for HalfPageUpAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = app_data.ui.help_scroll.saturating_sub(5).min(max_scroll);
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for HalfPageDownAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = app_data.ui.help_scroll.saturating_add(5).min(max_scroll);
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        clamp_help_scroll(app_data);
        app_data.ui.help_scroll = 0;
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let max_scroll = clamp_help_scroll(app_data);
        app_data.ui.help_scroll = max_scroll;
        Ok(HelpMode.into())
    }
}

impl ValidIn<HelpMode> for DismissAction {
    type NextState = AppMode;

    fn execute(self, _state: HelpMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<ErrorModalMode> for DismissAction {
    type NextState = AppMode;

    fn execute(self, _state: ErrorModalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.ui.clear_error();
        Ok(AppMode::normal())
    }
}

impl ValidIn<SuccessModalMode> for DismissAction {
    type NextState = AppMode;

    fn execute(self, _state: SuccessModalMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}
