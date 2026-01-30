//! Modal/overlay action types (new architecture).

use super::{
    DismissAction, ScrollBottomAction, ScrollDownAction, ScrollTopAction, ScrollUpAction, ValidIn,
};
use crate::app::AppData;
use crate::config::{Action as KeyAction, ActionGroup};
use crate::state::{AppMode, ChangelogMode, ErrorModalMode, HelpMode, SuccessModalMode};
use anyhow::Result;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

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

/// Compute the maximum scroll offset for the changelog overlay.
///
/// This mirrors the sizing and wrapping logic in `src/tui/render/modals/changelog.rs`, but uses
/// the most recently known terminal dimensions stored in `data.ui.terminal_dimensions`.
#[must_use]
pub fn changelog_max_scroll(data: &AppData, state: &ChangelogMode) -> usize {
    let frame_area = terminal_frame_area(data);
    let total_lines = state.lines.len();

    let max_height = frame_area.height.saturating_sub(4);
    let min_height = 12u16.min(max_height);
    let desired_height = u16::try_from(total_lines)
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    let area = centered_rect_absolute(60, height, frame_area);

    let visible_height = usize::from(area.height.saturating_sub(2));
    let inner_width = area.width.saturating_sub(2);

    let mut wrapped_lines = wrapped_line_count(&state.lines, inner_width);
    if inner_width != 0 && wrapped_lines > visible_height {
        let reserved_width = inner_width.saturating_sub(1);
        if reserved_width != inner_width {
            wrapped_lines = wrapped_line_count(&state.lines, reserved_width);
        }
    }

    wrapped_lines.saturating_sub(visible_height)
}

fn clamp_help_scroll(app_data: &mut AppData) -> usize {
    let max_scroll = help_max_scroll(app_data);
    app_data.ui.help_scroll = app_data.ui.help_scroll.min(max_scroll);
    max_scroll
}

fn terminal_frame_area(data: &AppData) -> Rect {
    if let Some((width, height)) = data.ui.terminal_dimensions {
        return Rect::new(0, 0, width, height);
    }

    let (preview_width, preview_height) = data.ui.preview_dimensions.unwrap_or((80, 20));

    Rect::new(
        0,
        0,
        infer_frame_width_from_preview_width(preview_width),
        preview_height.saturating_add(4),
    )
}

fn infer_frame_width_from_preview_width(preview_width: u16) -> u16 {
    let content_width = u32::from(preview_width).saturating_add(2);
    let upper = content_width
        .saturating_mul(100)
        .saturating_add(99)
        .saturating_div(70);
    u16::try_from(upper).unwrap_or(u16::MAX)
}

fn centered_rect_absolute(percent_x: u16, height: u16, area: Rect) -> Rect {
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

fn wrapped_line_count(lines: &[String], width: u16) -> usize {
    if width == 0 {
        return 0;
    }

    let width = usize::from(width);
    let mut total = 0usize;

    for line in lines {
        if line.is_empty() {
            total = total.saturating_add(1);
            continue;
        }
        total = total.saturating_add(wrap_single_line(line, width).len());
    }

    total
}

struct WrapSpec<'a> {
    first_prefix: &'a str,
    subsequent_prefix: String,
    content: &'a str,
}

fn wrap_spec(line: &str) -> WrapSpec<'_> {
    let indent_len = line.as_bytes().iter().take_while(|&&b| b == b' ').count();
    let after_indent = &line[indent_len..];

    if after_indent.starts_with("- ")
        || after_indent.starts_with("* ")
        || after_indent.starts_with("+ ")
    {
        let prefix_len = indent_len.saturating_add(2);
        return WrapSpec {
            first_prefix: &line[..prefix_len],
            subsequent_prefix: " ".repeat(prefix_len),
            content: &line[prefix_len..],
        };
    }

    let after_bytes = after_indent.as_bytes();
    let mut digits_len = 0usize;
    while digits_len < after_bytes.len() && after_bytes[digits_len].is_ascii_digit() {
        digits_len = digits_len.saturating_add(1);
    }

    if digits_len != 0
        && after_bytes.get(digits_len) == Some(&b'.')
        && after_bytes.get(digits_len.saturating_add(1)) == Some(&b' ')
    {
        let prefix_len = indent_len.saturating_add(digits_len).saturating_add(2);
        return WrapSpec {
            first_prefix: &line[..prefix_len],
            subsequent_prefix: " ".repeat(prefix_len),
            content: &line[prefix_len..],
        };
    }

    WrapSpec {
        first_prefix: &line[..indent_len],
        subsequent_prefix: " ".repeat(indent_len),
        content: after_indent,
    }
}

fn wrap_single_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    if line.len() <= width {
        return vec![line.to_string()];
    }

    let spec = wrap_spec(line);
    if width <= spec.first_prefix.len() {
        return chunk_into_width(line, width);
    }

    if spec.content.trim().is_empty() {
        return vec![line.to_string()];
    }

    let first_available = width.saturating_sub(spec.first_prefix.len());
    let subsequent_available = width.saturating_sub(spec.subsequent_prefix.len());
    if first_available == 0 || subsequent_available == 0 {
        return chunk_into_width(line, width);
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut is_first_line = true;

    for word in spec.content.split_whitespace() {
        let mut remaining = word;

        while !remaining.is_empty() {
            let available = if is_first_line {
                first_available
            } else {
                subsequent_available
            };

            if current.is_empty() {
                if remaining.len() <= available {
                    current.push_str(remaining);
                    break;
                }

                let (chunk, rest) = split_at_char_boundary(remaining, available);
                current.push_str(chunk);

                let prefix = if is_first_line {
                    spec.first_prefix
                } else {
                    &spec.subsequent_prefix
                };
                out.push(prefixed(prefix, &current));
                current.clear();
                is_first_line = false;
                remaining = rest;
                continue;
            }

            if current
                .len()
                .saturating_add(1)
                .saturating_add(remaining.len())
                <= available
            {
                current.push(' ');
                current.push_str(remaining);
                break;
            }

            let prefix = if is_first_line {
                spec.first_prefix
            } else {
                &spec.subsequent_prefix
            };
            out.push(prefixed(prefix, &current));
            current.clear();
            is_first_line = false;
        }
    }

    if !current.is_empty() {
        let prefix = if is_first_line {
            spec.first_prefix
        } else {
            &spec.subsequent_prefix
        };
        out.push(prefixed(prefix, &current));
    }

    out
}

fn prefixed(prefix: &str, content: &str) -> String {
    let mut out = String::with_capacity(prefix.len().saturating_add(content.len()));
    out.push_str(prefix);
    out.push_str(content);
    out
}

fn split_at_char_boundary(s: &str, max_bytes: usize) -> (&str, &str) {
    if s.len() <= max_bytes {
        return (s, "");
    }

    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut = cut.saturating_sub(1);
    }

    if cut == 0 {
        let Some(first_char) = s.chars().next() else {
            return ("", "");
        };
        let cut = first_char.len_utf8();
        return s.split_at(cut);
    }

    s.split_at(cut)
}

fn chunk_into_width(mut s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();

    while !s.is_empty() {
        let (chunk, rest) = split_at_char_boundary(s, width);
        if chunk.is_empty() {
            break;
        }

        out.push(chunk.to_string());
        s = rest;
    }

    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use semver::Version;

    fn app_data() -> AppData {
        AppData::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        )
    }

    #[test]
    fn test_changelog_max_scroll_returns_zero_for_short_content() {
        let mut data = app_data();
        data.ui.terminal_dimensions = Some((80, 24));

        let state = ChangelogMode {
            title: "Changelog".to_string(),
            lines: vec!["One".to_string(), "Two".to_string(), "Three".to_string()],
            mark_seen_version: Some(Version::new(1, 2, 3)),
        };

        assert_eq!(changelog_max_scroll(&data, &state), 0);
    }

    #[test]
    fn test_changelog_max_scroll_accounts_for_wrapping() {
        let mut data = app_data();
        data.ui.terminal_dimensions = Some((80, 24));

        let mut lines = Vec::new();
        for _ in 0..50 {
            lines.push(
                "- This bullet is long enough that it should wrap within the modal width."
                    .to_string(),
            );
        }

        let state = ChangelogMode {
            title: "Changelog".to_string(),
            lines,
            mark_seen_version: None,
        };

        assert!(changelog_max_scroll(&data, &state) > 0);
    }

    #[test]
    fn test_terminal_frame_area_uses_preview_dimensions_when_missing_terminal_size() {
        let mut data = app_data();
        data.ui.terminal_dimensions = None;
        data.ui.preview_dimensions = Some((70, 20));

        let state = ChangelogMode {
            title: "Changelog".to_string(),
            lines: (0..40).map(|idx| format!("Line {idx}")).collect(),
            mark_seen_version: None,
        };

        assert!(changelog_max_scroll(&data, &state) > 0);
    }

    #[test]
    fn test_wrap_single_line_chunks_long_word() {
        let wrapped = wrap_single_line("- abcdefghijklmnop", 8);
        assert_eq!(
            wrapped,
            vec![
                "- abcdef".to_string(),
                "  ghijkl".to_string(),
                "  mnop".to_string()
            ]
        );
    }

    #[test]
    fn test_infer_frame_width_from_preview_width_ceils() {
        assert_eq!(infer_frame_width_from_preview_width(70), 104);
    }

    #[test]
    fn test_wrap_single_line_returns_line_when_content_is_empty() {
        let wrapped = wrap_single_line("  -   ", 12);
        assert_eq!(wrapped, vec!["  -   ".to_string()]);
    }

    #[test]
    fn test_wrap_single_line_chunks_when_width_smaller_than_prefix() {
        let wrapped = wrap_single_line("    - abc", 4);
        assert_eq!(
            wrapped,
            vec!["    ".to_string(), "- ab".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn test_wrap_single_line_numbered_list_indent() {
        let wrapped = wrap_single_line("  12. one two three four", 14);
        assert_eq!(
            wrapped,
            vec![
                "  12. one two".to_string(),
                "      three".to_string(),
                "      four".to_string()
            ]
        );
    }

    #[test]
    fn test_wrapped_line_count_returns_zero_when_width_zero() {
        assert_eq!(wrapped_line_count(&[String::from("line")], 0), 0);
    }

    #[test]
    fn test_split_at_char_boundary_splits_ascii() {
        let (left, right) = split_at_char_boundary("abcdef", 3);
        assert_eq!(left, "abc");
        assert_eq!(right, "def");
    }

    #[test]
    fn test_chunk_into_width_returns_empty_when_width_zero() {
        assert!(chunk_into_width("abcdef", 0).is_empty());
    }
}
