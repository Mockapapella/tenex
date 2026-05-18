//! Mouse input handling (click-to-select).

use crate::app::{App, PreviewSelectionPoint, Tab};
use crate::state::{AppMode, DiffFocusedMode, PreviewFocusedMode, ScrollingMode};
use ratatui::{
    crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Direction, Layout, Rect},
};
const MOUSE_SCROLL_LINES: usize = 3;

/// Handle a mouse event.
///
/// Handles left-click selection (agents list, tabs, preview focus),
/// scroll wheel preview/diff scrolling, and "click outside modal to cancel".
pub fn handle_mouse_event(
    app: &mut App,
    mouse: MouseEvent,
    frame_area: Rect,
    batched_keys: &mut Vec<String>,
) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            handle_left_click(app, mouse.column, mouse.row, frame_area);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            handle_left_drag(app, mouse.column, mouse.row, frame_area);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            handle_left_up(app, mouse.column, mouse.row, frame_area);
        }
        MouseEventKind::ScrollUp => {
            handle_scroll_wheel(
                app,
                mouse.column,
                mouse.row,
                mouse.modifiers,
                ScrollDirection::Up,
                frame_area,
                batched_keys,
            );
        }
        MouseEventKind::ScrollDown => {
            handle_scroll_wheel(
                app,
                mouse.column,
                mouse.row,
                mouse.modifiers,
                ScrollDirection::Down,
                frame_area,
                batched_keys,
            );
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Copy)]
enum ScrollDirection {
    Up,
    Down,
}

fn handle_scroll_wheel(
    app: &mut App,
    x: u16,
    y: u16,
    modifiers: KeyModifiers,
    direction: ScrollDirection,
    frame_area: Rect,
    batched_keys: &mut Vec<String>,
) {
    // Allow scroll wheel when the changelog modal is open.
    if let AppMode::Changelog(state) = &app.mode {
        let modal_area = crate::tui::render::modals::changelog_modal_rect(state, frame_area);
        if !rect_contains(modal_area, x, y) {
            return;
        }

        let max_scroll = crate::action::changelog_max_scroll(&app.data, state);

        app.data.ui.changelog_scroll = app.data.ui.changelog_scroll.min(max_scroll);
        match direction {
            ScrollDirection::Up => {
                app.data.ui.changelog_scroll = app
                    .data
                    .ui
                    .changelog_scroll
                    .saturating_sub(MOUSE_SCROLL_LINES);
            }
            ScrollDirection::Down => {
                app.data.ui.changelog_scroll = app
                    .data
                    .ui
                    .changelog_scroll
                    .saturating_add(MOUSE_SCROLL_LINES)
                    .min(max_scroll);
            }
        }
        return;
    }

    // Ignore scroll wheel while a modal is open or text input is active.
    if !matches!(
        &app.mode,
        AppMode::Normal(_)
            | AppMode::Scrolling(_)
            | AppMode::PreviewFocused(_)
            | AppMode::DiffFocused(_)
    ) {
        return;
    }

    let (agents_area, content_area) = main_panes(frame_area);
    if rect_contains(content_area, x, y) {
        if app.data.active_tab == Tab::Preview && app.data.ui.preview_selection_anchor.is_some() {
            match direction {
                ScrollDirection::Up => app.data.scroll_up(MOUSE_SCROLL_LINES),
                ScrollDirection::Down => app.data.scroll_down(MOUSE_SCROLL_LINES),
            }

            if let Some(body_area) = content_body_area(content_area)
                && let Some(point) = preview_selection_point_for_xy(app, body_area, x, y)
            {
                app.data.ui.preview_selection_cursor = point;
                app.data.ui.preview_selection_dragging = true;
            }

            if matches!(&app.mode, AppMode::Normal(_) | AppMode::Scrolling(_)) {
                app.apply_mode(ScrollingMode.into());
            }
            return;
        }

        let preview_focused = matches!(&app.mode, AppMode::PreviewFocused(_));
        let preview_tab = app.data.active_tab == Tab::Preview;
        let preview_is_codex = app
            .data
            .selected_agent()
            .is_some_and(|agent| is_codex_program(&agent.program));

        // Some agent UIs (e.g. Codex) run full-screen. If Tenex has no scrollback to scroll,
        // forward wheel events to the agent so the wheel can still do something useful.
        if preview_focused && preview_tab && preview_is_codex {
            let visible_height = app
                .data
                .ui
                .preview_dimensions
                .map_or(20, |(_, h)| usize::from(h));
            let can_scroll_preview = app.data.ui.preview_text.lines.len() > visible_height;
            if !can_scroll_preview {
                if let Some(sequence) =
                    scroll_wheel_to_sgr_sequence(content_area, x, y, modifiers, direction)
                {
                    batched_keys.push(sequence);
                }
                return;
            }
        }

        match direction {
            ScrollDirection::Up => app.data.scroll_up(MOUSE_SCROLL_LINES),
            ScrollDirection::Down => app.data.scroll_down(MOUSE_SCROLL_LINES),
        }

        // Match keyboard scrolling behavior: when Tenex has focus, enter scrolling mode.
        // When preview/diff is focused, keep focus so keystrokes still go to the agent.
        if matches!(&app.mode, AppMode::Normal(_) | AppMode::Scrolling(_)) {
            app.apply_mode(ScrollingMode.into());
        }
        return;
    }

    // Reserved for future: scrolling the agents list.
    let _ = agents_area;
}

fn is_codex_program(program: &str) -> bool {
    program
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .any(|token| token == "codex")
}

fn scroll_wheel_to_sgr_sequence(
    content_area: Rect,
    x: u16,
    y: u16,
    modifiers: KeyModifiers,
    direction: ScrollDirection,
) -> Option<String> {
    // Compute inner block area (inside borders), then the preview "body" area
    // (inner area excluding the 1-line tab bar).
    let inner = Rect {
        x: content_area.x.saturating_add(1),
        y: content_area.y.saturating_add(1),
        width: content_area.width.saturating_sub(2),
        height: content_area.height.saturating_sub(2),
    };

    let body = Rect {
        x: inner.x,
        y: inner.y.saturating_add(1),
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    if body.width == 0 || body.height == 0 {
        return None;
    }

    // Map terminal coordinates into 1-based cell coordinates for the agent PTY.
    let local_x = usize::from(x.saturating_sub(body.x).min(body.width.saturating_sub(1)));
    let local_y = y.saturating_sub(body.y).min(body.height.saturating_sub(1));
    let col = u16::try_from(local_x.saturating_add(1)).unwrap_or(u16::MAX);
    let row = local_y.saturating_add(1);

    // Xterm mouse protocol button codes.
    let base_button = match direction {
        ScrollDirection::Up => 64u8,
        ScrollDirection::Down => 65u8,
    };

    let mut button = base_button;
    if modifiers.contains(KeyModifiers::SHIFT) {
        button = button.saturating_add(4);
    }
    if modifiers.contains(KeyModifiers::ALT) {
        button = button.saturating_add(8);
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        button = button.saturating_add(16);
    }

    Some(format!("\x1b[<{button};{col};{row}M"))
}

fn handle_left_click(app: &mut App, x: u16, y: u16, frame_area: Rect) {
    // If a modal is open, only handle outside-click-to-cancel.
    if !matches!(
        &app.mode,
        AppMode::Normal(_)
            | AppMode::Scrolling(_)
            | AppMode::PreviewFocused(_)
            | AppMode::DiffFocused(_)
    ) {
        if let Some(modal_area) = modal_rect(app, frame_area)
            && !rect_contains(modal_area, x, y)
        {
            // Clicking off the modal is equivalent to pressing Escape/cancel.
            let mut unused_batched_keys = Vec::new();
            let _ = crate::tui::input::handle_key_event(
                app,
                KeyCode::Esc,
                KeyModifiers::NONE,
                &mut unused_batched_keys,
            );
        }
        return;
    }

    let (agents_area, content_area) = main_panes(frame_area);

    if rect_contains(agents_area, x, y) {
        // Clicking anywhere in the agents pane should focus Tenex (i.e., detach from preview).
        app.apply_mode(AppMode::normal());
        clear_preview_selection(app);
        handle_agent_list_click(app, x, y, agents_area);
        return;
    }

    if rect_contains(content_area, x, y) {
        handle_content_pane_click(app, x, y, content_area);
    }
}

fn main_panes(frame_area: Rect) -> (Rect, Rect) {
    let main_area = Rect {
        x: frame_area.x,
        y: frame_area.y,
        width: frame_area.width,
        height: frame_area.height.saturating_sub(1),
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(main_area);

    (chunks[0], chunks[1])
}

const fn clear_preview_selection(app: &mut App) {
    app.data.ui.preview_selection_anchor = None;
    app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
    app.data.ui.preview_selection_dragging = false;
}

fn handle_left_drag(app: &mut App, x: u16, y: u16, frame_area: Rect) {
    if app.data.active_tab != Tab::Preview {
        return;
    }

    let Some(_anchor) = app.data.ui.preview_selection_anchor else {
        return;
    };

    let (_agents_area, content_area) = main_panes(frame_area);
    let Some(body_area) = content_body_area(content_area) else {
        return;
    };

    let bottom_y = body_area
        .y
        .saturating_add(body_area.height.saturating_sub(1));

    if y < body_area.y {
        let delta = usize::from(body_area.y.saturating_sub(y)).max(1);
        app.data.scroll_up(delta);
    } else if y > bottom_y {
        let delta = usize::from(y.saturating_sub(bottom_y)).max(1);
        app.data.scroll_down(delta);
    }

    let Some(point) = preview_selection_point_for_xy(app, body_area, x, y) else {
        return;
    };

    app.data.ui.preview_selection_cursor = point;
    app.data.ui.preview_selection_dragging = true;
}

fn handle_left_up(app: &mut App, x: u16, y: u16, frame_area: Rect) {
    if app.data.active_tab != Tab::Preview {
        clear_preview_selection(app);
        return;
    }

    let Some(anchor) = app.data.ui.preview_selection_anchor else {
        return;
    };

    if !app.data.ui.preview_selection_dragging {
        clear_preview_selection(app);
        return;
    }

    let (_agents_area, content_area) = main_panes(frame_area);
    let Some(body_area) = content_body_area(content_area) else {
        clear_preview_selection(app);
        return;
    };

    let Some(cursor) = preview_selection_point_for_xy(app, body_area, x, y) else {
        clear_preview_selection(app);
        return;
    };

    let (start, end) = normalize_preview_selection_points(anchor, cursor);

    if let Some(text) = preview_selection_text(app, start, end) {
        app.data.ui.pending_clipboard = Some(text);
    }

    clear_preview_selection(app);
}

const fn content_body_area(content_area: Rect) -> Option<Rect> {
    // Compute inner block area (inside borders), then the content "body" area
    // (inner area excluding the 1-line tab bar).
    let inner = Rect {
        x: content_area.x.saturating_add(1),
        y: content_area.y.saturating_add(1),
        width: content_area.width.saturating_sub(2),
        height: content_area.height.saturating_sub(2),
    };

    if inner.width == 0 || inner.height < 2 {
        return None;
    }

    Some(Rect {
        x: inner.x,
        y: inner.y.saturating_add(1),
        width: inner.width,
        height: inner.height.saturating_sub(1),
    })
}

fn preview_column_for_x(_app: &App, body_area: Rect, x: u16) -> usize {
    if body_area.width == 0 {
        return 0;
    }

    let max_x = body_area.width.saturating_sub(1);
    let local_x = x.saturating_sub(body_area.x).min(max_x);
    usize::from(local_x)
}

fn preview_selection_point_for_xy(
    app: &App,
    body_area: Rect,
    x: u16,
    y: u16,
) -> Option<PreviewSelectionPoint> {
    let line = preview_line_index_for_y(app, body_area, y)?;
    let column = preview_column_for_x(app, body_area, x);
    Some(PreviewSelectionPoint { line, column })
}

const fn normalize_preview_selection_points(
    anchor: PreviewSelectionPoint,
    cursor: PreviewSelectionPoint,
) -> (PreviewSelectionPoint, PreviewSelectionPoint) {
    if anchor.line < cursor.line || (anchor.line == cursor.line && anchor.column <= cursor.column) {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    }
}

fn preview_line_index_for_y(app: &App, body_area: Rect, y: u16) -> Option<usize> {
    let line_count = app.data.ui.preview_text.lines.len();
    if line_count == 0 || body_area.height == 0 {
        return None;
    }

    let visible_height = usize::from(body_area.height);
    let max_scroll = line_count.saturating_sub(visible_height);
    let scroll = app.data.ui.preview_scroll.min(max_scroll);

    let local_row = if y < body_area.y {
        0usize
    } else {
        usize::from(y.saturating_sub(body_area.y)).min(visible_height.saturating_sub(1))
    };

    Some(
        scroll
            .saturating_add(local_row)
            .min(line_count.saturating_sub(1)),
    )
}

fn preview_selection_text(
    app: &App,
    start: PreviewSelectionPoint,
    end: PreviewSelectionPoint,
) -> Option<String> {
    let line_count = app.data.ui.preview_text.lines.len();
    let max_line = line_count.checked_sub(1)?;

    let clamped_start = PreviewSelectionPoint {
        line: start.line.min(max_line),
        column: start.column,
    };
    let clamped_end = PreviewSelectionPoint {
        line: end.line.min(max_line),
        column: end.column,
    };
    let start = clamped_start;
    let end = clamped_end;
    if start.line > end.line || (start.line == end.line && start.column > end.column) {
        return None;
    }

    let mut out = String::new();
    for (idx, line_idx) in (start.line..=end.line).enumerate() {
        if idx > 0 {
            out.push('\n');
        }

        let line = &app.data.ui.preview_text.lines[line_idx];
        let mut line_text = String::new();
        for span in &line.spans {
            line_text.push_str(span.content.as_ref());
        }

        let (slice_start, slice_end) = if start.line == end.line {
            (start.column, end.column)
        } else if line_idx == start.line {
            (start.column, usize::MAX)
        } else if line_idx == end.line {
            (0usize, end.column)
        } else {
            (0usize, usize::MAX)
        };

        if let Some((start_byte, end_byte)) =
            byte_range_for_char_range_inclusive(&line_text, slice_start, slice_end)
        {
            out.push_str(&line_text[start_byte..end_byte]);
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

fn byte_range_for_char_range_inclusive(
    text: &str,
    start_col: usize,
    end_col_inclusive: usize,
) -> Option<(usize, usize)> {
    let char_len = text.chars().count();
    if char_len == 0 {
        return Some((0, 0));
    }

    let start = start_col.min(char_len);
    let end_inclusive = end_col_inclusive.min(char_len.saturating_sub(1));
    if start > end_inclusive {
        return None;
    }

    let start_byte = byte_index_for_char(text, start);
    let end_byte = byte_index_for_char(text, end_inclusive.saturating_add(1));
    Some((start_byte, end_byte))
}

fn byte_index_for_char(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }

    match text.char_indices().nth(char_idx) {
        Some((byte_idx, _)) => byte_idx,
        None => text.len(),
    }
}

fn handle_agent_list_click(app: &mut App, x: u16, y: u16, area: Rect) {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    if !rect_contains(inner, x, y) {
        return;
    }

    let row = usize::from(y.saturating_sub(inner.y));
    let idx = app.data.ui.agent_list_scroll.saturating_add(row);
    let visible_count = app.data.sidebar_len();
    if idx >= visible_count {
        return;
    }

    app.data.selected = idx;
    app.data.ui.reset_scroll();
    app.data.ensure_agent_list_scroll();
}

fn handle_content_pane_click(app: &mut App, x: u16, y: u16, area: Rect) {
    // Compute inner block area (inside borders).
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    // Tab bar is the first line of the inner area.
    let tab_bar_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };

    if rect_contains(tab_bar_area, x, y) {
        clear_preview_selection(app);
        handle_tab_bar_click(app, x, tab_bar_area);
        return;
    }

    // Click in the content body focuses the content pane.
    if app.data.active_tab == Tab::Preview
        && let Some(body_area) = content_body_area(area)
        && rect_contains(body_area, x, y)
    {
        if let Some(point) = preview_selection_point_for_xy(app, body_area, x, y) {
            app.data.ui.preview_selection_anchor = Some(point);
            app.data.ui.preview_selection_cursor = point;
            app.data.ui.preview_selection_dragging = false;
        }
    } else {
        clear_preview_selection(app);
    }

    match app.data.active_tab {
        Tab::Preview => {
            if app.data.selected_agent().is_some() {
                app.apply_mode(PreviewFocusedMode.into());
            } else {
                app.apply_mode(ScrollingMode.into());
            }
        }
        Tab::Diff => {
            if app.data.selected_agent().is_some() {
                app.apply_mode(DiffFocusedMode.into());
            } else {
                app.apply_mode(ScrollingMode.into());
            }
        }
        Tab::Commits => {
            app.apply_mode(ScrollingMode.into());
        }
    }
}

fn handle_tab_bar_click(app: &mut App, x: u16, tab_bar_area: Rect) {
    let rel_x = x.saturating_sub(tab_bar_area.x);
    let Some(tab) = crate::tui::render::main_layout::tab_for_tab_bar_offset(app, rel_x) else {
        return;
    };

    let was_preview_focused = matches!(&app.mode, AppMode::PreviewFocused(_));
    if was_preview_focused && tab == Tab::Preview {
        return;
    }

    let was_diff_focused = matches!(&app.mode, AppMode::DiffFocused(_));
    if was_diff_focused && tab == Tab::Diff {
        return;
    }

    if app.data.active_tab != tab {
        app.data.active_tab = tab;
        app.data.ui.reset_scroll();
    }

    match tab {
        Tab::Preview => {
            if app.data.selected_agent().is_some() {
                app.apply_mode(PreviewFocusedMode.into());
            } else {
                app.apply_mode(ScrollingMode.into());
            }
        }
        Tab::Diff => {
            if app.data.selected_agent().is_some() {
                app.apply_mode(DiffFocusedMode.into());
            } else {
                app.apply_mode(ScrollingMode.into());
            }
        }
        Tab::Commits => {
            app.apply_mode(ScrollingMode.into());
        }
    }
}

const fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    let within_x = x >= rect.x && x < rect.x.saturating_add(rect.width);
    let within_y = y >= rect.y && y < rect.y.saturating_add(rect.height);
    within_x && within_y
}

fn modal_rect(app: &App, frame_area: Rect) -> Option<Rect> {
    crate::tui::render::modals::modal_rect_for_mode(app, frame_area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::{
        AppMode, ChildCountMode, DiffFocusedMode, NormalMode, PreviewFocusedMode, ScrollingMode,
        UpdateRequestedMode,
    };
    use crate::update::UpdateInfo;
    use anyhow::Result;
    use ratatui::crossterm::event::KeyModifiers;
    use semver::Version;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    thread_local! {
        static TEST_FORCE_HANDLE_MOUSE_EVENT_ERROR: std::cell::Cell<bool> = const {
            std::cell::Cell::new(false)
        };
    }

    fn force_handle_mouse_event_error_if_enabled_for_tests() -> Result<()> {
        if TEST_FORCE_HANDLE_MOUSE_EVENT_ERROR.with(std::cell::Cell::get) {
            anyhow::bail!("forced mouse handler error for test");
        }
        Ok(())
    }

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().expect("create test state file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    fn add_agent(app: &mut App, title: &str) {
        add_agent_with_program(app, title, "echo");
    }

    fn add_agent_with_program(app: &mut App, title: &str, program: &str) {
        let agent = Agent::new(
            title.to_string(),
            program.to_string(),
            format!("tenex/{title}"),
            PathBuf::from("/tmp"),
        );
        app.data.storage.add(agent);
    }

    fn handle_mouse_event(
        app: &mut App,
        mouse: MouseEvent,
        frame_area: Rect,
        batched_keys: &mut Vec<String>,
    ) -> Result<()> {
        super::handle_mouse_event(app, mouse, frame_area, batched_keys);
        force_handle_mouse_event_error_if_enabled_for_tests()?;
        Ok(())
    }

    #[inline(never)]
    fn is_normal_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::Normal(_))
    }

    #[inline(never)]
    fn is_diff_focused_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::DiffFocused(_))
    }

    #[inline(never)]
    fn is_scrolling_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::Scrolling(_))
    }

    #[inline(never)]
    fn is_preview_focused_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::PreviewFocused(_))
    }

    #[inline(never)]
    fn is_child_count_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::ChildCount(_))
    }

    #[inline(never)]
    fn is_update_requested_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::UpdateRequested(_))
    }

    #[test]
    fn test_app_mode_predicates_cover_both_outcomes() {
        let normal = NormalMode.into();
        let diff_focused = DiffFocusedMode.into();
        let scrolling = ScrollingMode.into();
        let preview_focused = PreviewFocusedMode.into();
        let child_count = ChildCountMode.into();
        let update_requested = UpdateRequestedMode {
            info: UpdateInfo {
                current_version: Version::new(1, 0, 0),
                latest_version: Version::new(1, 0, 1),
            },
        }
        .into();

        assert!(is_normal_mode(&normal));
        assert!(!is_normal_mode(&diff_focused));

        assert!(is_diff_focused_mode(&diff_focused));
        assert!(!is_diff_focused_mode(&normal));

        assert!(is_scrolling_mode(&scrolling));
        assert!(!is_scrolling_mode(&normal));

        assert!(is_preview_focused_mode(&preview_focused));
        assert!(!is_preview_focused_mode(&normal));

        assert!(is_child_count_mode(&child_count));
        assert!(!is_child_count_mode(&normal));

        assert!(is_update_requested_mode(&update_requested));
        assert!(!is_update_requested_mode(&normal));
    }

    fn left_click(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn scroll_up(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn scroll_down(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_drag(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_up(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn unhandled_mouse_event_kind_noops() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(NormalMode.into());

        let frame = Rect::new(0, 0, 80, 24);
        let mut batched_keys = Vec::new();
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse_event(&mut app, event, frame, &mut batched_keys)
            .expect("unhandled event should not fail");
        assert!(is_normal_mode(&app.mode));
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn handle_mouse_event_errors_when_forced_for_tests() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(NormalMode.into());

        let frame = Rect::new(0, 0, 80, 24);
        let mut batched_keys = Vec::new();
        let event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };

        let err = TEST_FORCE_HANDLE_MOUSE_EVENT_ERROR.with(|slot| {
            let previous = slot.replace(true);
            let result = handle_mouse_event(&mut app, event, frame, &mut batched_keys);
            slot.set(previous);
            result.expect_err("expected forced mouse handler error")
        });

        assert!(
            err.to_string()
                .contains("forced mouse handler error for test")
        );
    }

    #[test]
    fn click_agent_row_selects_agent() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        add_agent(&mut app, "a1");
        add_agent(&mut app, "a2");
        app.apply_mode(NormalMode.into());

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let agents_area = chunks[0];

        // Click on the second visible row (index 1).
        let inner_y = agents_area.y + 1;
        let click = left_click(agents_area.x + 2, inner_y + 1);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("agent row click should not fail");

        assert_eq!(app.data.selected, 1);
    }

    #[test]
    fn click_agent_row_beyond_visible_count_noops() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.selected = 0;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let agents_area = chunks[0];

        let inner_y = agents_area.y + 1;
        let click = left_click(agents_area.x + 2, inner_y + 10);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("click past visible agents should not fail");

        assert_eq!(app.data.selected, 0);
    }

    #[test]
    fn click_agents_pane_detaches_preview_without_selecting_row() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        add_agent(&mut app, "a1");
        app.apply_mode(PreviewFocusedMode.into());

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let click = left_click(0, 0); // agents pane border
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("agents pane click should not fail");

        assert!(is_normal_mode(&app.mode));
    }

    #[test]
    fn click_outside_main_panes_noops() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());

        let selected_before = app.data.selected;
        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let click = left_click(0, frame.height.saturating_sub(1));
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("click should not fail");

        assert!(is_normal_mode(&app.mode));
        assert_eq!(app.data.selected, selected_before);
    }

    #[test]
    fn click_tab_bar_offset_past_end_noops() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        let click = left_click(
            tab_bar.x.saturating_add(tab_bar.width.saturating_sub(1)),
            tab_bar.y,
        );
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("tab bar click should not fail");
        assert_eq!(app.data.active_tab, Tab::Preview);
    }

    #[test]
    fn click_diff_tab_selects_diff() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the " Diff " label (after " Preview ").
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let click = left_click(tab_bar.x + preview_w + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_diff_focused_mode(&app.mode));
    }

    #[test]
    fn click_diff_tab_while_preview_focused_enters_diff_focused_mode() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the " Diff " label (after " Preview ").
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let click = left_click(tab_bar.x + preview_w + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_diff_focused_mode(&app.mode));
    }

    #[test]
    fn click_diff_tab_with_unseen_dot_selects_diff() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let agent_id = app.selected_agent().expect("Expected selected agent").id;
        app.data.ui.diff_hash = 123;
        app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 0);

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the last character of "Diff" when the unseen dot is shown.
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let diff_w = u16::try_from(" ◐ Diff ".chars().count()).unwrap_or(0);
        let click = left_click(
            tab_bar
                .x
                .saturating_add(preview_w)
                .saturating_add(diff_w.saturating_sub(2)),
            tab_bar.y,
        );
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_diff_focused_mode(&app.mode));
    }

    #[test]
    fn click_commits_tab_selects_commits() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the " Commits " label (after " Preview " + " Diff ").
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let diff_w = u16::try_from(" Diff ".chars().count()).unwrap_or(0);
        let click = left_click(tab_bar.x + preview_w + diff_w + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("commits tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn click_commits_tab_with_unseen_dot_selects_commits() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let agent_id = app.selected_agent().expect("Expected selected agent").id;
        app.data.ui.diff_hash = 0;
        app.data.ui.commits_hash = 123;
        app.data
            .ui
            .set_commits_last_seen_hash_for_agent(agent_id, 0);

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the last character of "Commits" when the unseen dot is shown.
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let diff_w = u16::try_from(" Diff ".chars().count()).unwrap_or(0);
        let commits_w = u16::try_from(" ◐ Commits ".chars().count()).unwrap_or(0);
        let click = left_click(
            tab_bar
                .x
                .saturating_add(preview_w)
                .saturating_add(diff_w)
                .saturating_add(commits_w.saturating_sub(2)),
            tab_bar.y,
        );
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("commits tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn click_preview_tab_enters_preview_focused_mode() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the " Preview " label (first tab).
        let click = left_click(tab_bar.x + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("preview tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert!(is_preview_focused_mode(&app.mode));
    }

    #[test]
    fn click_preview_tab_while_already_preview_focused_noops() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        let click = left_click(tab_bar.x + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("preview tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert!(is_preview_focused_mode(&app.mode));
    }

    #[test]
    fn click_preview_tab_without_agent_enters_scrolling_mode() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        let click = left_click(tab_bar.x + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("preview tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn click_diff_tab_without_agent_enters_scrolling_mode() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let click = left_click(tab_bar.x + preview_w + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn click_diff_body_enters_diff_focused_mode() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];

        // Click below the tab bar, inside the diff content body.
        let click = left_click(content_area.x + 2, content_area.y + 3);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff body click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_diff_focused_mode(&app.mode));
    }

    #[test]
    fn click_diff_body_without_agent_enters_scrolling_mode() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];

        let click = left_click(content_area.x + 2, content_area.y + 3);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff body click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn drag_select_preview_sets_pending_clipboard() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("line1\nline2\nline3");

        let frame = Rect::new(0, 0, 100, 30);
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");

        let mut batched_keys = Vec::new();
        let end_x = body_area
            .x
            .saturating_add(body_area.width.saturating_sub(1));
        handle_mouse_event(
            &mut app,
            left_click(body_area.x, body_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected left click to succeed");
        handle_mouse_event(
            &mut app,
            left_drag(end_x, body_area.y + 2),
            frame,
            &mut batched_keys,
        )
        .expect("Expected left drag to succeed");
        handle_mouse_event(
            &mut app,
            left_up(end_x, body_area.y + 2),
            frame,
            &mut batched_keys,
        )
        .expect("Expected left up to succeed");

        assert_eq!(
            app.data.ui.pending_clipboard.as_deref(),
            Some("line1\nline2\nline3")
        );
    }

    #[test]
    fn drag_ignores_events_when_not_on_preview_tab() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_drag(5, 5), frame, &mut batched_keys)
            .expect("drag should not fail");

        assert!(!app.data.ui.preview_selection_dragging);
    }

    #[test]
    fn drag_returns_early_when_preview_has_no_lines() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("");
        app.data.ui.preview_text.lines.clear();
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_drag(5, 5), frame, &mut batched_keys)
            .expect("drag should not fail");

        assert!(!app.data.ui.preview_selection_dragging);
    }

    #[test]
    fn drag_select_preview_single_line_selects_by_column() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("0123456789");

        let frame = Rect::new(0, 0, 100, 8);
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 7,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");
        app.set_preview_dimensions(body_area.width, body_area.height);

        let start = (body_area.x.saturating_add(2), body_area.y);
        let end = (body_area.x.saturating_add(5), body_area.y);

        let mut batched_keys = Vec::new();
        handle_mouse_event(
            &mut app,
            left_click(start.0, start.1),
            frame,
            &mut batched_keys,
        )
        .expect("Expected click to succeed");
        handle_mouse_event(&mut app, left_drag(end.0, end.1), frame, &mut batched_keys)
            .expect("Expected drag to succeed");
        handle_mouse_event(&mut app, left_up(end.0, end.1), frame, &mut batched_keys)
            .expect("Expected up to succeed");

        assert_eq!(app.data.ui.pending_clipboard.as_deref(), Some("2345"));
    }

    #[test]
    fn drag_beyond_preview_bottom_autoscrolls_and_extends_selection() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = false;
        app.data.ui.set_preview_content(
            (0..20)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let frame = Rect::new(0, 0, 100, 8);
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 7,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");
        app.set_preview_dimensions(body_area.width, body_area.height);

        let end_x = body_area
            .x
            .saturating_add(body_area.width.saturating_sub(1));
        let start_y = body_area.y;
        let end_y = body_area
            .y
            .saturating_add(body_area.height.saturating_sub(1))
            .saturating_add(1);

        let mut batched_keys = Vec::new();
        handle_mouse_event(
            &mut app,
            left_click(body_area.x, start_y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected click to succeed");
        handle_mouse_event(&mut app, left_drag(end_x, end_y), frame, &mut batched_keys)
            .expect("Expected drag to succeed");
        handle_mouse_event(&mut app, left_up(end_x, end_y), frame, &mut batched_keys)
            .expect("Expected up to succeed");

        let expected = (0..5)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            app.data.ui.pending_clipboard.as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn scroll_wheel_while_selecting_extends_preview_selection() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = false;
        app.data.ui.set_preview_content(
            (0..20)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let frame = Rect::new(0, 0, 100, 8);
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 7,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");
        app.set_preview_dimensions(body_area.width, body_area.height);

        let end_x = body_area
            .x
            .saturating_add(body_area.width.saturating_sub(1));
        let end_y = body_area
            .y
            .saturating_add(body_area.height.saturating_sub(1));

        let mut batched_keys = Vec::new();
        handle_mouse_event(
            &mut app,
            left_click(body_area.x, body_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected click to succeed");
        handle_mouse_event(&mut app, left_drag(end_x, end_y), frame, &mut batched_keys)
            .expect("Expected drag to succeed");
        handle_mouse_event(
            &mut app,
            scroll_down(end_x, end_y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected scroll down to succeed");
        handle_mouse_event(&mut app, left_up(end_x, end_y), frame, &mut batched_keys)
            .expect("Expected up to succeed");

        let expected = (0..7)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            app.data.ui.pending_clipboard.as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn click_preview_does_not_set_pending_clipboard() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("line1\nline2\nline3");

        let frame = Rect::new(0, 0, 100, 30);
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");

        let mut batched_keys = Vec::new();
        handle_mouse_event(
            &mut app,
            left_click(body_area.x + 1, body_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected click to succeed");
        handle_mouse_event(
            &mut app,
            left_up(body_area.x + 1, body_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected up to succeed");

        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn drag_preview_without_anchor_noops() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("line1\nline2\nline3");

        let frame = Rect::new(0, 0, 100, 30);
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");

        let mut batched_keys = Vec::new();
        handle_mouse_event(
            &mut app,
            left_drag(body_area.x + 1, body_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("Expected drag to succeed");

        assert!(app.data.ui.preview_selection_anchor.is_none());
        assert!(!app.data.ui.preview_selection_dragging);
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn left_up_outside_preview_clears_preview_selection() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 2, column: 0 };
        app.data.ui.preview_selection_dragging = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_up(10, 10), frame, &mut batched_keys)
            .expect("mouse up should not fail");

        assert!(app.data.ui.preview_selection_anchor.is_none());
        assert_eq!(
            app.data.ui.preview_selection_cursor,
            PreviewSelectionPoint { line: 0, column: 0 }
        );
        assert!(!app.data.ui.preview_selection_dragging);
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn content_body_area_returns_none_when_too_small() {
        assert!(content_body_area(Rect::new(0, 0, 0, 0)).is_none());
        assert!(content_body_area(Rect::new(0, 0, 10, 3)).is_none());
    }

    #[test]
    fn preview_selection_text_returns_none_for_empty_or_inverted_range() {
        let (mut app, _tmp) = create_test_app();
        assert!(
            preview_selection_text(
                &app,
                PreviewSelectionPoint { line: 0, column: 0 },
                PreviewSelectionPoint { line: 0, column: 0 }
            )
            .is_none()
        );

        app.data.ui.set_preview_content("line1\nline2");
        assert!(
            preview_selection_text(
                &app,
                PreviewSelectionPoint { line: 1, column: 0 },
                PreviewSelectionPoint { line: 0, column: 0 }
            )
            .is_none()
        );
    }

    #[test]
    fn selection_up_clears_when_body_area_missing() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("line1\nline2");
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 1, column: 0 };
        app.data.ui.preview_selection_dragging = true;

        let frame = Rect::new(0, 0, 100, 3);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_up(10, 1), frame, &mut batched_keys)
            .expect("mouse up should not fail");

        assert!(app.data.ui.preview_selection_anchor.is_none());
        assert_eq!(
            app.data.ui.preview_selection_cursor,
            PreviewSelectionPoint { line: 0, column: 0 }
        );
        assert!(!app.data.ui.preview_selection_dragging);
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn click_commits_body_enters_scrolling_mode() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Commits;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];

        // Click below the tab bar, inside the commits content body.
        let click = left_click(content_area.x + 2, content_area.y + 3);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("commits body click should not fail");

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn click_commits_tab_while_diff_focused_switches_tabs() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(DiffFocusedMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the " Commits " label (after " Preview " + " Diff ").
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let diff_w = u16::try_from(" Diff ".chars().count()).unwrap_or(0);
        let click = left_click(tab_bar.x + preview_w + diff_w + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("commits tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn click_diff_tab_while_diff_focused_keeps_diff_focused() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(DiffFocusedMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        // Click inside the " Diff " label (after " Preview ").
        let preview_w = u16::try_from(" Preview ".chars().count()).unwrap_or(0);
        let click = left_click(tab_bar.x + preview_w + 1, tab_bar.y);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("diff tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(is_diff_focused_mode(&app.mode));
    }

    #[test]
    fn click_preview_body_focuses_preview() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let main = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 29,
        };
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(main);
        let content_area = chunks[1];

        // Click below the tab bar, inside the preview content body.
        let click = left_click(content_area.x + 2, content_area.y + 3);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("preview body click should not fail");

        assert!(is_preview_focused_mode(&app.mode));
    }

    #[test]
    fn click_outside_modal_cancels() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(ChildCountMode.into());
        assert!(!matches!(&app.mode, AppMode::Normal(_)));

        let frame = Rect::new(0, 0, 80, 24);
        let mut batched_keys = Vec::new();
        let click = left_click(0, 0);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)
            .expect("outside modal click should not fail");

        assert!(is_normal_mode(&app.mode));
    }

    #[test]
    fn click_inside_modal_does_not_cancel() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(ChildCountMode.into());
        assert!(!matches!(&app.mode, AppMode::Normal(_)));

        let frame = Rect::new(0, 0, 80, 24);
        let modal_area = modal_rect(&app, frame).expect("Expected modal to have a rect");
        let inside = left_click(modal_area.x + 1, modal_area.y + 1);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, inside, frame, &mut batched_keys)
            .expect("inside modal click should not fail");

        assert!(is_child_count_mode(&app.mode));
    }

    #[test]
    fn scroll_wheel_over_content_scrolls_preview_and_enters_scrolling_mode() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.set_preview_content(
            (0..30)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (agents_area, content_area) = main_panes(frame);
        let _ = agents_area;

        // Scroll up inside the preview body.
        let event = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, event, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_scrolling_mode(&app.mode));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_over_agents_area_noops() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (agents_area, _content_area) = main_panes(frame);

        let event = scroll_up(agents_area.x + 1, agents_area.y + 1);
        handle_mouse_event(&mut app, event, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_normal_mode(&app.mode));
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_returns_early_when_mode_is_modal() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(ChildCountMode.into());

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, scroll_up(10, 10), frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_child_count_mode(&app.mode));
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_over_content_when_active_tab_is_diff_enters_scrolling_mode() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        let event = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, event, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_scrolling_mode(&app.mode));
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_with_selection_anchor_skips_cursor_update_when_body_area_missing() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = false;
        app.data.ui.set_preview_content("line0\nline1\nline2");
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 4);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        handle_mouse_event(
            &mut app,
            scroll_down(content_area.x + 1, content_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("scroll wheel should not fail");

        assert!(is_scrolling_mode(&app.mode));
        assert!(!app.data.ui.preview_selection_dragging);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_with_selection_anchor_skips_cursor_update_when_preview_point_missing() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = false;
        app.data.ui.set_preview_content("");
        app.data.ui.preview_text.lines.clear();
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        handle_mouse_event(
            &mut app,
            scroll_down(content_area.x + 2, content_area.y + 2),
            frame,
            &mut batched_keys,
        )
        .expect("scroll wheel should not fail");

        assert!(is_scrolling_mode(&app.mode));
        assert!(!app.data.ui.preview_selection_dragging);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_with_selection_anchor_scrolls_and_updates_cursor() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.set_preview_content(
            (0..30)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_scroll = 0;
        app.data.ui.preview_follow = false;
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        let event = scroll_down(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, event, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_scrolling_mode(&app.mode));
        assert!(app.data.ui.preview_selection_dragging);
    }

    #[test]
    fn scroll_wheel_with_selection_anchor_scrolls_up() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.preview_follow = false;
        app.data.ui.set_preview_content(
            (0..30)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_scroll = MOUSE_SCROLL_LINES.saturating_add(1);
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        let before = app.data.ui.preview_scroll;
        handle_mouse_event(
            &mut app,
            scroll_up(content_area.x + 2, content_area.y + 2),
            frame,
            &mut batched_keys,
        )
        .expect("Expected mouse scroll to succeed");
        assert!(app.data.ui.preview_scroll < before);
        assert!(is_scrolling_mode(&app.mode));
    }

    #[test]
    fn drag_returns_early_when_body_area_missing() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("line0\nline1\nline2");
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = false;

        let frame = Rect::new(0, 0, 100, 2);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_drag(1, 1), frame, &mut batched_keys)
            .expect("mouse drag should not fail");
        assert!(!app.data.ui.preview_selection_dragging);
    }

    #[test]
    fn drag_above_body_area_autoscrolls_up() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = false;
        app.data.ui.set_preview_content(
            (0..20)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let frame = Rect::new(0, 0, 100, 30);
        let (_agents_area, content_area) = main_panes(frame);
        let body_area = content_body_area(content_area)
            .expect("Expected preview body to be renderable for selection");
        app.set_preview_dimensions(body_area.width, body_area.height);

        app.data.ui.preview_scroll = 10;
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_dragging = false;

        let before = app.data.ui.preview_scroll;
        let mut batched_keys = Vec::new();
        handle_mouse_event(
            &mut app,
            left_drag(body_area.x, body_area.y.saturating_sub(1)),
            frame,
            &mut batched_keys,
        )
        .expect("Expected mouse drag to succeed");
        assert!(app.data.ui.preview_scroll < before);
    }

    #[test]
    fn left_up_returns_early_when_selection_anchor_missing() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_selection_anchor = None;
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 2, column: 3 };
        app.data.ui.preview_selection_dragging = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_up(5, 5), frame, &mut batched_keys)
            .expect("mouse up should not fail");
        assert_eq!(
            app.data.ui.preview_selection_cursor,
            PreviewSelectionPoint { line: 2, column: 3 }
        );
        assert!(app.data.ui.preview_selection_dragging);
    }

    #[test]
    fn left_up_clears_when_preview_line_lookup_fails() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("");
        app.data.ui.preview_text.lines.clear();
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_up(10, 10), frame, &mut batched_keys)
            .expect("mouse up should not fail");

        assert!(app.data.ui.preview_selection_anchor.is_none());
        assert!(!app.data.ui.preview_selection_dragging);
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_scrolls_without_detaching() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.set_preview_content(
            (0..30)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);
        let up = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_preview_focused_mode(&app.mode));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        assert!(batched_keys.is_empty());

        let down = scroll_down(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, down, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");
        assert!(is_preview_focused_mode(&app.mode));
        assert!(app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 27);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_over_non_scrollable_preview_does_not_pause_follow() {
        // Regression: when the preview buffer can't scroll, wheel-up should not flip follow off.
        // Otherwise Tenex looks "paused" even though there's no scrollback to move through.
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.set_preview_content("line0\nline1\nline2");
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);
        let up = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(app.data.ui.preview_follow);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_for_codex_scrolls_preview() {
        // Regression: some terminals report wheel events with ALT set. Codex preview scrolling
        // should keep working regardless of modifiers.
        let (mut app, _tmp) = create_test_app();
        add_agent_with_program(&mut app, "a0", "codex");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.set_preview_content(
            (0..30)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);
        let up = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_preview_focused_mode(&app.mode));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        assert!(batched_keys.is_empty());

        let up_with_alt = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: content_area.x + 2,
            row: content_area.y + 2,
            modifiers: KeyModifiers::ALT,
        };
        handle_mouse_event(&mut app, up_with_alt, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_preview_focused_mode(&app.mode));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 21);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_for_codex_forwards_when_preview_isnt_scrollable() {
        // Some terminals don't report wheel modifiers reliably. If Tenex has no scrollback to
        // scroll anyway, forwarding is strictly better than entering a "paused" state.
        let (mut app, _tmp) = create_test_app();
        add_agent_with_program(&mut app, "a0", "codex");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.set_preview_content("line0\nline1\nline2");
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);
        let up = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)
            .expect("scroll wheel should not fail");

        assert!(is_preview_focused_mode(&app.mode));
        assert!(app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, usize::MAX);
        assert_eq!(batched_keys, vec![String::from("\u{1b}[<64;2;1M")]);
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_for_codex_does_not_push_sequence_when_content_area_is_too_small()
     {
        let (mut app, _tmp) = create_test_app();
        add_agent_with_program(&mut app, "a0", "codex");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("line0\nline1\nline2");
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 20, 4);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        handle_mouse_event(
            &mut app,
            scroll_up(content_area.x + 1, content_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("scroll wheel should not fail");

        assert!(is_preview_focused_mode(&app.mode));
        assert!(app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, usize::MAX);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_when_active_tab_is_diff_does_not_forward_codex() {
        let (mut app, _tmp) = create_test_app();
        add_agent_with_program(&mut app, "a0", "codex");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Diff;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);

        handle_mouse_event(
            &mut app,
            scroll_up(content_area.x + 2, content_area.y + 2),
            frame,
            &mut batched_keys,
        )
        .expect("scroll wheel should not fail");

        assert!(is_preview_focused_mode(&app.mode));
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_in_changelog_modal_scrolls_when_inside_modal() {
        let (mut app, _tmp) = create_test_app();
        app.set_terminal_dimensions(100, 30);
        app.apply_mode(AppMode::Changelog(crate::state::ChangelogMode {
            title: String::from("What's New"),
            lines: (0..200).map(|i| format!("line{i}")).collect(),
            mark_seen_version: None,
        }));

        let frame = Rect::new(0, 0, 100, 30);
        let modal_area = modal_rect(&app, frame).expect("Expected changelog modal to have a rect");

        let mut batched_keys = Vec::new();
        let inside = (modal_area.x + 1, modal_area.y + 1);
        handle_mouse_event(
            &mut app,
            scroll_down(inside.0, inside.1),
            frame,
            &mut batched_keys,
        )
        .expect("Expected scroll down to succeed");
        assert_eq!(app.data.ui.changelog_scroll, MOUSE_SCROLL_LINES);
        assert!(batched_keys.is_empty());

        handle_mouse_event(
            &mut app,
            scroll_up(inside.0, inside.1),
            frame,
            &mut batched_keys,
        )
        .expect("Expected scroll up to succeed");
        assert_eq!(app.data.ui.changelog_scroll, 0);
        assert!(batched_keys.is_empty());

        handle_mouse_event(&mut app, scroll_down(0, 0), frame, &mut batched_keys)
            .expect("Expected scroll down outside modal to succeed");
        assert_eq!(app.data.ui.changelog_scroll, 0);
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn scroll_wheel_to_sgr_sequence_encodes_direction_and_modifiers() {
        let content_area = Rect::new(0, 0, 100, 30);
        let modifiers = KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL;
        let seq = scroll_wheel_to_sgr_sequence(
            content_area,
            content_area.x + 2,
            content_area.y + 2,
            modifiers,
            ScrollDirection::Down,
        )
        .expect("Expected SGR sequence for non-empty content area");
        assert_eq!(seq, String::from("\u{1b}[<93;2;1M"));

        let seq_up = scroll_wheel_to_sgr_sequence(
            content_area,
            content_area.x + 2,
            content_area.y + 2,
            modifiers,
            ScrollDirection::Up,
        )
        .expect("Expected SGR sequence for non-empty content area");
        assert_eq!(seq_up, String::from("\u{1b}[<92;2;1M"));

        assert!(
            scroll_wheel_to_sgr_sequence(
                Rect::new(0, 0, 2, 2),
                0,
                0,
                KeyModifiers::NONE,
                ScrollDirection::Up
            )
            .is_none()
        );

        assert!(
            scroll_wheel_to_sgr_sequence(
                Rect::new(0, 0, 4, 3),
                0,
                0,
                KeyModifiers::NONE,
                ScrollDirection::Up
            )
            .is_none()
        );
    }

    #[test]
    fn preview_helpers_cover_edge_cases() {
        let mut app = App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        app.data.ui.set_preview_content("line0\nline1");
        app.data.ui.preview_scroll = 0;

        assert_eq!(preview_column_for_x(&app, Rect::new(0, 0, 0, 5), 0), 0);

        assert_eq!(
            normalize_preview_selection_points(
                PreviewSelectionPoint { line: 9, column: 0 },
                PreviewSelectionPoint { line: 1, column: 0 }
            ),
            (
                PreviewSelectionPoint { line: 1, column: 0 },
                PreviewSelectionPoint { line: 9, column: 0 }
            )
        );

        assert_eq!(
            normalize_preview_selection_points(
                PreviewSelectionPoint { line: 1, column: 5 },
                PreviewSelectionPoint { line: 1, column: 2 }
            ),
            (
                PreviewSelectionPoint { line: 1, column: 2 },
                PreviewSelectionPoint { line: 1, column: 5 }
            )
        );

        assert_eq!(
            preview_line_index_for_y(&app, Rect::new(10, 10, 5, 3), 9),
            Some(0)
        );

        assert_eq!(
            preview_line_index_for_y(&app, Rect::new(0, 0, 10, 0), 0),
            None
        );

        assert_eq!(byte_range_for_char_range_inclusive("", 0, 0), Some((0, 0)));
        assert_eq!(byte_range_for_char_range_inclusive("abc", 2, 0), None);
    }

    #[test]
    fn is_codex_program_recognizes_tokens_across_separators() {
        assert!(is_codex_program("codex"));
        assert!(is_codex_program("python -m codex"));
        assert!(is_codex_program("/usr/bin/codex"));
        assert!(!is_codex_program("mycodex"));
        assert!(!is_codex_program("code-x"));
        assert!(!is_codex_program("code_x"));
    }

    #[test]
    fn click_while_update_requested_does_not_cancel() {
        let (mut app, _tmp) = create_test_app();
        app.apply_mode(
            UpdateRequestedMode {
                info: UpdateInfo {
                    current_version: Version::new(1, 0, 0),
                    latest_version: Version::new(1, 0, 1),
                },
            }
            .into(),
        );

        let frame = Rect::new(0, 0, 80, 24);
        let mut batched_keys = Vec::new();
        handle_mouse_event(&mut app, left_click(0, 0), frame, &mut batched_keys)
            .expect("click should not fail");

        assert!(is_update_requested_mode(&app.mode));
    }

    #[test]
    fn handle_content_pane_click_clears_selection_when_body_area_missing() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });

        let content_area = Rect::new(0, 0, 10, 3);
        handle_content_pane_click(&mut app, 2, 2, content_area);

        assert!(app.data.ui.preview_selection_anchor.is_none());
    }

    #[test]
    fn handle_content_pane_click_clears_selection_when_click_outside_body_area() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });

        let content_area = Rect::new(0, 0, 100, 30);
        handle_content_pane_click(&mut app, content_area.x, content_area.y, content_area);

        assert!(app.data.ui.preview_selection_anchor.is_none());
    }

    #[test]
    fn click_active_preview_tab_in_normal_mode_focuses_preview() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);
        let inner = Rect {
            x: content_area.x + 1,
            y: content_area.y + 1,
            width: content_area.width.saturating_sub(2),
            height: content_area.height.saturating_sub(2),
        };
        let tab_bar = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };

        handle_mouse_event(
            &mut app,
            left_click(tab_bar.x + 1, tab_bar.y),
            frame,
            &mut batched_keys,
        )
        .expect("tab click should not fail");

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert!(is_preview_focused_mode(&app.mode));
    }

    #[test]
    fn left_up_does_not_set_pending_clipboard_when_preview_text_is_empty() {
        let (mut app, _tmp) = create_test_app();
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;
        app.data.ui.set_preview_content("");
        app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
        app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 0 };
        app.data.ui.preview_selection_dragging = true;

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let (_agents_area, content_area) = main_panes(frame);
        let body_area = content_body_area(content_area).expect("body area should exist");

        handle_mouse_event(
            &mut app,
            left_up(body_area.x, body_area.y),
            frame,
            &mut batched_keys,
        )
        .expect("left up should not fail");

        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn preview_selection_text_returns_none_for_inverted_columns_on_same_line() {
        let (mut app, _tmp) = create_test_app();
        app.data.ui.set_preview_content("line1\nline2");
        assert!(
            preview_selection_text(
                &app,
                PreviewSelectionPoint { line: 0, column: 4 },
                PreviewSelectionPoint { line: 0, column: 1 }
            )
            .is_none()
        );
    }

    #[test]
    fn preview_selection_text_skips_ranges_that_start_past_line_end() {
        let (mut app, _tmp) = create_test_app();
        app.data.ui.set_preview_content("abc\ndef");
        assert_eq!(
            preview_selection_text(
                &app,
                PreviewSelectionPoint {
                    line: 0,
                    column: 99
                },
                PreviewSelectionPoint { line: 1, column: 0 }
            )
            .as_deref(),
            Some("\nd")
        );
    }
}
