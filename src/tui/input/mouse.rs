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
mod tests;
