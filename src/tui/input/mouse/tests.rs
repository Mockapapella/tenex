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
    handle_mouse_event(&mut app, click, frame, &mut batched_keys).expect("click should not fail");

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
