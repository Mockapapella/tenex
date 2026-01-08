//! Mouse input handling (click-to-select).

use crate::app::{App, Tab};
use crate::state::{AppMode, PreviewFocusedMode, ScrollingMode};
use anyhow::Result;
use ratatui::{
    crossterm::event::{MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Direction, Layout, Rect},
};

const MOUSE_SCROLL_LINES: usize = 3;

/// Handle a mouse event.
///
/// Handles left-click selection (agents list, tabs, preview focus),
/// scroll wheel preview/diff scrolling, and "click outside modal to cancel".
pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent, frame_area: Rect) -> Result<()> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            handle_left_click(app, mouse.column, mouse.row, frame_area)?;
        }
        MouseEventKind::ScrollUp => {
            handle_scroll_wheel(
                app,
                mouse.column,
                mouse.row,
                ScrollDirection::Up,
                frame_area,
            );
        }
        MouseEventKind::ScrollDown => {
            handle_scroll_wheel(
                app,
                mouse.column,
                mouse.row,
                ScrollDirection::Down,
                frame_area,
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
    direction: ScrollDirection,
    frame_area: Rect,
) {
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

fn handle_left_click(app: &mut App, x: u16, y: u16, frame_area: Rect) -> Result<()> {
    // If a modal is open, only handle outside-click-to-cancel.
    if !matches!(
        &app.mode,
        AppMode::Normal(_) | AppMode::Scrolling(_) | AppMode::PreviewFocused(_)
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
    let visible_count = app.data.storage.visible_count();
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

    // Click anywhere else in the preview pane focuses it (attaches).
    if app.data.active_tab == Tab::Preview && app.data.selected_agent().is_some() {
        app.data.active_tab = Tab::Preview;
        app.apply_mode(PreviewFocusedMode.into());
    }
}

fn handle_tab_bar_click(app: &mut App, x: u16, tab_bar_area: Rect) {
    // Keep these in sync with `render::main_layout::tab_bar_line`.
    const PREVIEW_LABEL: &str = " Preview ";
    const DIFF_LABEL: &str = " Diff ";

    let rel_x = x.saturating_sub(tab_bar_area.x);
    let preview_w = u16::try_from(PREVIEW_LABEL.chars().count()).unwrap_or(0);
    let diff_w = u16::try_from(DIFF_LABEL.chars().count()).unwrap_or(0);

    if rel_x < preview_w {
        if app.data.active_tab != Tab::Preview {
            app.data.active_tab = Tab::Preview;
            app.data.ui.reset_scroll();
        }
        return;
    }

    if rel_x < preview_w.saturating_add(diff_w) {
        if app.data.active_tab != Tab::Diff {
            app.data.active_tab = Tab::Diff;
            app.data.ui.reset_scroll();
        }
        // Diff view is non-interactive; ensure we aren't "attached" to preview.
        if matches!(&app.mode, AppMode::PreviewFocused(_)) {
            app.apply_mode(AppMode::normal());
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
    use crate::state::{AppMode, ChildCountMode, NormalMode};
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
        let agent = Agent::new(
            title.to_string(),
            "echo".to_string(),
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
        handle_mouse_event(&mut app, click, frame)?;

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
        let click = left_click(0, 0); // agents pane border
        handle_mouse_event(&mut app, click, frame)?;

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
        handle_mouse_event(&mut app, click, frame)?;

        assert_eq!(app.data.active_tab, Tab::Diff);
        Ok(())
    }

    #[test]
    fn click_preview_body_focuses_preview() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        add_agent(&mut app, "a0");
        app.apply_mode(NormalMode.into());
        app.data.active_tab = Tab::Preview;

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

        // Click below the tab bar, inside the preview content body.
        let click = left_click(content_area.x + 2, content_area.y + 3);
        handle_mouse_event(&mut app, click, frame)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        Ok(())
    }

    #[test]
    fn click_outside_modal_cancels() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        app.apply_mode(ChildCountMode.into());
        assert!(!matches!(&app.mode, AppMode::Normal(_)));

        let frame = Rect::new(0, 0, 80, 24);
        let click = left_click(0, 0);
        handle_mouse_event(&mut app, click, frame)?;

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
        app.data.ui.preview_content = (0..30)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let (agents_area, content_area) = main_panes(frame);
        let _ = agents_area;

        // Scroll up inside the preview body.
        let event = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, event, frame)?;

        assert!(matches!(&app.mode, AppMode::Scrolling(_)));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);
        Ok(())
    }

    #[test]
    fn scroll_wheel_in_preview_focused_mode_scrolls_without_detaching() -> anyhow::Result<()> {
        let (mut app, _tmp) = create_test_app()?;
        add_agent(&mut app, "a0");
        app.apply_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Preview;
        app.set_preview_dimensions(80, 3);
        app.data.ui.preview_content = (0..30)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.ui.preview_scroll = usize::MAX;
        app.data.ui.preview_follow = true;

        let frame = Rect::new(0, 0, 100, 30);
        let (_agents_area, content_area) = main_panes(frame);
        let up = scroll_up(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, up, frame)?;

        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(!app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 24);

        let down = scroll_down(content_area.x + 2, content_area.y + 2);
        handle_mouse_event(&mut app, down, frame)?;
        assert!(matches!(&app.mode, AppMode::PreviewFocused(_)));
        assert!(app.data.ui.preview_follow);
        assert_eq!(app.data.ui.preview_scroll, 27);
        Ok(())
    }
}
