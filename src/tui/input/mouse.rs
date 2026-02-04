//! Mouse input handling (click-to-select).

use crate::app::{App, Tab};
use crate::state::{AppMode, DiffFocusedMode, PreviewFocusedMode, ScrollingMode};
use anyhow::Result;
use ratatui::{
    crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
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
) -> Result<()> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            handle_left_click(app, mouse.column, mouse.row, frame_area)?;
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
    Ok(())
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
    if matches!(&app.mode, AppMode::Changelog(_)) {
        let Some(modal_area) = modal_rect(app, frame_area) else {
            return;
        };
        if !rect_contains(modal_area, x, y) {
            return;
        }

        let max_scroll = match &app.mode {
            AppMode::Changelog(state) => crate::action::changelog_max_scroll(&app.data, state),
            _ => return,
        };

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
    let local_x = x.saturating_sub(body.x).min(body.width.saturating_sub(1));
    let local_y = y.saturating_sub(body.y).min(body.height.saturating_sub(1));
    let col = local_x.saturating_add(1);
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

fn handle_left_click(app: &mut App, x: u16, y: u16, frame_area: Rect) -> Result<()> {
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
            crate::tui::input::handle_key_event(
                app,
                ratatui::crossterm::event::KeyCode::Esc,
                ratatui::crossterm::event::KeyModifiers::NONE,
                &mut unused_batched_keys,
            )?;
        }
        return Ok(());
    }

    let (agents_area, content_area) = main_panes(frame_area);

    if rect_contains(agents_area, x, y) {
        // Clicking anywhere in the agents pane should focus Tenex (i.e., detach from preview).
        app.apply_mode(AppMode::normal());
        handle_agent_list_click(app, x, y, agents_area);
        return Ok(());
    }

    if rect_contains(content_area, x, y) {
        handle_content_pane_click(app, x, y, content_area);
    }

    Ok(())
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
        handle_tab_bar_click(app, x, tab_bar_area);
        return;
    }

    // Click in the content body focuses the content pane.
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
    use crate::state::{AppMode, ChildCountMode, DiffFocusedMode, NormalMode};
    use ratatui::crossterm::event::KeyModifiers;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
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

    #[test]
    fn click_agent_row_selects_agent() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.selected, 1);
        Ok(())
    }

    #[test]
    fn click_agents_pane_detaches_preview_without_selecting_row() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        add_agent(&mut app, "a0");
        add_agent(&mut app, "a1");
        app.apply_mode(PreviewFocusedMode.into());

        let frame = Rect::new(0, 0, 100, 30);
        let mut batched_keys = Vec::new();
        let click = left_click(0, 0); // agents pane border
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::Normal(_)));
        Ok(())
    }

    #[test]
    fn click_diff_tab_selects_diff() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(matches!(&app.mode, AppMode::DiffFocused(_)));
        Ok(())
    }

    #[test]
    fn click_diff_tab_while_preview_focused_enters_diff_focused_mode() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(matches!(&app.mode, AppMode::DiffFocused(_)));
        Ok(())
    }

    #[test]
    fn click_diff_tab_with_unseen_dot_selects_diff() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let agent_id = app
            .selected_agent()
            .map(|agent| agent.id)
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(matches!(&app.mode, AppMode::DiffFocused(_)));
        Ok(())
    }

    #[test]
    fn click_commits_tab_selects_commits() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(matches!(&app.mode, AppMode::Scrolling(_)));
        Ok(())
    }

    #[test]
    fn click_commits_tab_with_unseen_dot_selects_commits() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

        let agent_id = app
            .selected_agent()
            .map(|agent| agent.id)
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(matches!(&app.mode, AppMode::Scrolling(_)));
        Ok(())
    }

    #[test]
    fn click_preview_tab_enters_preview_focused_mode() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        Ok(())
    }

    #[test]
    fn click_diff_body_enters_diff_focused_mode() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(matches!(&app.mode, AppMode::DiffFocused(_)));
        Ok(())
    }

    #[test]
    fn click_commits_body_enters_scrolling_mode() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(matches!(&app.mode, AppMode::Scrolling(_)));
        Ok(())
    }

    #[test]
    fn click_commits_tab_while_diff_focused_switches_tabs() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Commits);
        assert!(matches!(&app.mode, AppMode::Scrolling(_)));
        Ok(())
    }

    #[test]
    fn click_diff_tab_while_diff_focused_keeps_diff_focused() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert!(matches!(&app.mode, AppMode::DiffFocused(_)));
        Ok(())
    }

    #[test]
    fn click_preview_body_focuses_preview() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        Ok(())
    }

    #[test]
    fn click_outside_modal_cancels() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        app.apply_mode(ChildCountMode.into());
        assert!(!matches!(&app.mode, AppMode::Normal(_)));

        let frame = Rect::new(0, 0, 80, 24);
        let mut batched_keys = Vec::new();
        let click = left_click(0, 0);
        handle_mouse_event(&mut app, click, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::Normal(_)));
        Ok(())
    }

    #[test]
    fn scroll_wheel_over_content_scrolls_preview_and_enters_scrolling_mode() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, event, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::Scrolling(_)));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        assert!(batched_keys.is_empty());
        Ok(())
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_scrolls_without_detaching() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        assert!(batched_keys.is_empty());

        let down = scroll_down(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, down, frame, &mut batched_keys)?;
        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 27);
        assert!(batched_keys.is_empty());
        Ok(())
    }

    #[test]
    fn scroll_wheel_over_non_scrollable_preview_does_not_pause_follow() -> anyhow::Result<()> {
        // Regression: when the preview buffer can't scroll, wheel-up should not flip follow off.
        // Otherwise Tenex looks "paused" even though there's no scrollback to move through.
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)?;

        assert!(app.data.ui.preview_follow);
        assert!(batched_keys.is_empty());

        Ok(())
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_for_codex_scrolls_preview() -> anyhow::Result<()> {
        // Regression: some terminals report wheel events with ALT set. Codex preview scrolling
        // should keep working regardless of modifiers.
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        assert!(batched_keys.is_empty());

        let up_with_alt = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: content_area.x + 2,
            row: content_area.y + 2,
            modifiers: KeyModifiers::ALT,
        };
        handle_mouse_event(&mut app, up_with_alt, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 21);
        assert!(batched_keys.is_empty());

        Ok(())
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_for_codex_forwards_when_preview_isnt_scrollable()
    -> anyhow::Result<()> {
        // Some terminals don't report wheel modifiers reliably. If Tenex has no scrollback to
        // scroll anyway, forwarding is strictly better than entering a "paused" state.
        let (mut app, _tmp) = create_test_app()?;
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
        handle_mouse_event(&mut app, up, frame, &mut batched_keys)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, usize::MAX);
        assert_eq!(batched_keys, vec![String::from("\u{1b}[<64;2;1M")]);

        Ok(())
    }
}
