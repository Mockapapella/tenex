use super::*;
use crate::agent::{Agent, Storage};
use crate::app::Settings;
use crate::config::Config;
use crate::state::{DiffFocusedMode, ScrollingMode};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::NamedTempFile;

fn create_test_app() -> (App, NamedTempFile) {
    let temp_file = NamedTempFile::new().expect("temp file");
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("")
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<Vec<_>>()
        .join("")
}

fn cell_at(buffer: &ratatui::buffer::Buffer, x: u16, y: u16) -> &ratatui::buffer::Cell {
    buffer.cell((x, y)).expect("missing cell")
}

#[test]
fn test_normalize_preview_selection_points_orders_by_line_then_column() {
    let anchor = PreviewSelectionPoint { line: 2, column: 0 };
    let cursor = PreviewSelectionPoint {
        line: 1,
        column: 99,
    };
    let (start, end) = normalize_preview_selection_points(anchor, cursor);
    assert_eq!(start, cursor);
    assert_eq!(end, anchor);

    let anchor = PreviewSelectionPoint {
        line: 1,
        column: 10,
    };
    let cursor = PreviewSelectionPoint { line: 1, column: 9 };
    let (start, end) = normalize_preview_selection_points(anchor, cursor);
    assert_eq!(start, cursor);
    assert_eq!(end, anchor);
}

#[test]
fn test_normalize_preview_selection_points_keeps_anchor_when_anchor_precedes_cursor() {
    let anchor = PreviewSelectionPoint { line: 1, column: 0 };
    let cursor = PreviewSelectionPoint { line: 2, column: 0 };
    let (start, end) = normalize_preview_selection_points(anchor, cursor);
    assert_eq!(start, anchor);
    assert_eq!(end, cursor);

    let anchor = PreviewSelectionPoint { line: 1, column: 3 };
    let cursor = PreviewSelectionPoint { line: 1, column: 9 };
    let (start, end) = normalize_preview_selection_points(anchor, cursor);
    assert_eq!(start, anchor);
    assert_eq!(end, cursor);
}

#[test]
fn test_apply_preview_selection_to_line_highlights_entire_line_when_fully_selected() {
    let mut line: Line<'static> = Line::from(vec![
        Span::styled("hello ", Style::default().fg(colors::TEXT_PRIMARY)),
        Span::styled("world", Style::default().fg(colors::TEXT_MUTED)),
    ]);

    let start = PreviewSelectionPoint { line: 0, column: 0 };
    let end = PreviewSelectionPoint {
        line: 0,
        column: usize::MAX,
    };
    apply_preview_selection_to_line(0, start, end, &mut line);

    assert_eq!(line_text(&line), "hello world");
    for span in &line.spans {
        assert_eq!(span.style.bg, Some(colors::DIFF_SELECTION_BG));
    }
}

#[test]
fn test_apply_preview_selection_to_line_splits_spans_for_partial_selection() {
    let mut line: Line<'static> = Line::from(vec![
        Span::styled("hello ", Style::default().fg(colors::TEXT_PRIMARY)),
        Span::styled("world", Style::default().fg(colors::TEXT_PRIMARY)),
    ]);

    let start = PreviewSelectionPoint { line: 0, column: 6 };
    let end = PreviewSelectionPoint { line: 0, column: 8 };
    apply_preview_selection_to_line(0, start, end, &mut line);

    assert_eq!(line_text(&line), "hello world");
    assert_eq!(line.spans.len(), 3);
    assert_eq!(line.spans[0].content.as_ref(), "hello ");
    assert_eq!(line.spans[0].style.bg, None);
    assert_eq!(line.spans[1].content.as_ref(), "wor");
    assert_eq!(line.spans[1].style.bg, Some(colors::DIFF_SELECTION_BG));
    assert_eq!(line.spans[2].content.as_ref(), "ld");
    assert_eq!(line.spans[2].style.bg, None);
}

#[test]
fn test_apply_preview_selection_to_line_partial_selection_from_start_omits_prefix_span() {
    let mut line: Line<'static> = Line::from(vec![Span::styled(
        "world",
        Style::default().fg(colors::TEXT_PRIMARY),
    )]);

    let start = PreviewSelectionPoint { line: 0, column: 0 };
    let end = PreviewSelectionPoint { line: 0, column: 1 };
    apply_preview_selection_to_line(0, start, end, &mut line);

    assert_eq!(line_text(&line), "world");
    assert_eq!(line.spans.len(), 2);
    assert_eq!(line.spans[0].content.as_ref(), "wo");
    assert_eq!(line.spans[0].style.bg, Some(colors::DIFF_SELECTION_BG));
    assert_eq!(line.spans[1].content.as_ref(), "rld");
    assert_eq!(line.spans[1].style.bg, None);
}

#[test]
fn test_apply_preview_selection_noops_when_not_dragging_or_anchor_missing() {
    let (mut app, _temp) = create_test_app();
    app.data.ui.preview_selection_dragging = false;
    app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 0, column: 0 });
    app.data.ui.preview_selection_cursor = PreviewSelectionPoint { line: 0, column: 4 };

    let mut text: Text<'static> = Text::from(vec![
        Line::from(Span::styled(
            "hello",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            "world",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
    ]);

    apply_preview_selection(&app, 0, &mut text);
    for line in &text.lines {
        for span in &line.spans {
            assert_eq!(span.style.bg, None);
        }
    }

    app.data.ui.preview_selection_dragging = true;
    app.data.ui.preview_selection_anchor = None;
    apply_preview_selection(&app, 0, &mut text);
    for line in &text.lines {
        for span in &line.spans {
            assert_eq!(span.style.bg, None);
        }
    }
}

#[test]
fn test_apply_preview_selection_highlights_lines_in_range_and_skips_others() {
    let (mut app, _temp) = create_test_app();
    app.data.ui.preview_selection_dragging = true;
    app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 6, column: 0 });
    app.data.ui.preview_selection_cursor = PreviewSelectionPoint {
        line: 8,
        column: usize::MAX,
    };

    let mut text: Text<'static> = Text::from(vec![
        Line::from("zero"),
        Line::from("first"),
        Line::from("second"),
        Line::from("third"),
    ]);

    apply_preview_selection(&app, 5, &mut text);

    for (idx, line) in text.lines.iter().enumerate() {
        let global_idx = 5usize.saturating_add(idx);
        let selected = (6..=8).contains(&global_idx);
        for span in &line.spans {
            if selected {
                assert_eq!(span.style.bg, Some(colors::DIFF_SELECTION_BG));
            } else {
                assert_eq!(span.style.bg, None);
            }
        }
    }
}

#[test]
fn test_apply_preview_selection_skips_lines_after_selection_end() {
    let (mut app, _temp) = create_test_app();
    app.data.ui.preview_selection_dragging = true;
    app.data.ui.preview_selection_anchor = Some(PreviewSelectionPoint { line: 6, column: 0 });
    app.data.ui.preview_selection_cursor = PreviewSelectionPoint {
        line: 6,
        column: usize::MAX,
    };

    let mut text: Text<'static> = Text::from(vec![
        Line::from("zero"),
        Line::from("first"),
        Line::from("second"),
        Line::from("third"),
    ]);

    apply_preview_selection(&app, 5, &mut text);

    for (idx, line) in text.lines.iter().enumerate() {
        let global_idx = 5usize.saturating_add(idx);
        let selected = global_idx == 6;
        for span in &line.spans {
            if selected {
                assert_eq!(span.style.bg, Some(colors::DIFF_SELECTION_BG));
            } else {
                assert_eq!(span.style.bg, None);
            }
        }
    }
}

#[test]
fn test_apply_preview_selection_to_line_returns_early_for_empty_line() {
    let mut line: Line<'static> = Line::from("");
    let start = PreviewSelectionPoint { line: 0, column: 0 };
    let end = PreviewSelectionPoint { line: 0, column: 0 };
    apply_preview_selection_to_line(0, start, end, &mut line);
    assert_eq!(line_text(&line), "");
}

#[test]
fn test_apply_preview_selection_to_line_supports_multi_line_ranges_and_empty_spans() {
    let selection_start = PreviewSelectionPoint { line: 0, column: 2 };
    let selection_end = PreviewSelectionPoint { line: 2, column: 1 };

    let mut start_line: Line<'static> = Line::from(vec![
        Span::raw(""),
        Span::styled("abcdef", Style::default().fg(colors::TEXT_PRIMARY)),
    ]);
    let mut middle_line: Line<'static> = Line::from(vec![Span::styled(
        "middle",
        Style::default().fg(colors::TEXT_PRIMARY),
    )]);
    let mut end_line: Line<'static> = Line::from(vec![Span::styled(
        "xyz",
        Style::default().fg(colors::TEXT_PRIMARY),
    )]);

    apply_preview_selection_to_line(0, selection_start, selection_end, &mut start_line);
    apply_preview_selection_to_line(1, selection_start, selection_end, &mut middle_line);
    apply_preview_selection_to_line(2, selection_start, selection_end, &mut end_line);

    assert_eq!(line_text(&start_line), "abcdef");
    assert!(
        start_line
            .spans
            .iter()
            .any(|span| span.content.as_ref().is_empty())
    );
    assert!(
        start_line
            .spans
            .iter()
            .any(|span| span.style.bg == Some(colors::DIFF_SELECTION_BG))
    );

    assert_eq!(line_text(&middle_line), "middle");
    for span in &middle_line.spans {
        assert_eq!(span.style.bg, Some(colors::DIFF_SELECTION_BG));
    }

    assert_eq!(line_text(&end_line), "xyz");
    assert!(
        end_line
            .spans
            .iter()
            .any(|span| span.style.bg == Some(colors::DIFF_SELECTION_BG))
    );
}

#[test]
fn test_apply_preview_selection_to_line_highlights_entire_span_when_selected() {
    let mut line: Line<'static> = Line::from(vec![
        Span::styled("foo", Style::default().fg(colors::TEXT_PRIMARY)),
        Span::styled("bar", Style::default().fg(colors::TEXT_PRIMARY)),
    ]);

    let start = PreviewSelectionPoint { line: 0, column: 0 };
    let end = PreviewSelectionPoint { line: 0, column: 2 };
    apply_preview_selection_to_line(0, start, end, &mut line);

    assert_eq!(line_text(&line), "foobar");
    assert_eq!(line.spans[0].content.as_ref(), "foo");
    assert_eq!(line.spans[0].style.bg, Some(colors::DIFF_SELECTION_BG));
    assert_eq!(line.spans[1].content.as_ref(), "bar");
    assert_eq!(line.spans[1].style.bg, None);
}

#[test]
fn test_apply_preview_selection_to_line_returns_early_when_selection_starts_at_eol() {
    let mut line: Line<'static> = Line::from(vec![Span::styled(
        "abc",
        Style::default().fg(colors::TEXT_PRIMARY),
    )]);

    let start = PreviewSelectionPoint { line: 0, column: 3 };
    let end = PreviewSelectionPoint {
        line: 0,
        column: usize::MAX,
    };
    apply_preview_selection_to_line(0, start, end, &mut line);

    assert_eq!(line_text(&line), "abc");
    assert_eq!(line.spans.len(), 1);
    assert_eq!(line.spans[0].style.bg, None);
}

#[test]
fn test_apply_preview_selection_to_line_partial_selection_to_end_of_span_uses_full_end_byte() {
    let mut line: Line<'static> = Line::from(vec![Span::styled(
        "world",
        Style::default().fg(colors::TEXT_PRIMARY),
    )]);

    let start = PreviewSelectionPoint { line: 0, column: 2 };
    let end = PreviewSelectionPoint { line: 0, column: 4 };
    apply_preview_selection_to_line(0, start, end, &mut line);

    assert_eq!(line_text(&line), "world");
    assert_eq!(line.spans.len(), 2);
    assert_eq!(line.spans[0].content.as_ref(), "wo");
    assert_eq!(line.spans[0].style.bg, None);
    assert_eq!(line.spans[1].content.as_ref(), "rld");
    assert_eq!(line.spans[1].style.bg, Some(colors::DIFF_SELECTION_BG));
}

#[test]
fn test_tab_bar_renders_unseen_diff_dot() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.selected = 1;
    app.data.active_tab = Tab::Preview;

    app.data.ui.diff_hash = 123;
    app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 0);

    let line = tab_bar_line(&app);
    assert!(line_text(&line).contains("◐ Diff"));
}

#[test]
fn test_tab_bar_hides_unseen_diff_dot_when_viewing_diff_tab() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.selected = 1;
    app.data.active_tab = Tab::Diff;

    app.data.ui.diff_hash = 123;
    app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 0);

    let line = tab_bar_line(&app);
    assert!(!line_text(&line).contains('◐'));
}

#[test]
fn test_tab_bar_hides_unseen_diff_dot_when_hash_seen() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.selected = 1;
    app.data.active_tab = Tab::Preview;

    app.data.ui.diff_hash = 123;
    app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 123);

    let line = tab_bar_line(&app);
    assert!(!line_text(&line).contains('◐'));
}

#[test]
fn test_no_agent_selected_placeholder_has_consistent_color_across_tabs() {
    let (mut app, _temp) = create_test_app();

    app.data.ui.set_preview_content("(No agent selected)");
    app.data.ui.set_diff_content("(No agent selected)");
    app.data.ui.set_commits_content("(No agent selected)");

    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            app.data.active_tab = Tab::Preview;
            render_preview(frame, &app, frame.area());
        })
        .expect("draw preview");
    let preview_fg = {
        let buffer = terminal.backend().buffer();
        let cell = cell_at(buffer, 1, 2);
        assert_eq!(cell.symbol(), "(");
        cell.fg
    };
    assert_eq!(preview_fg, colors::TEXT_MUTED);

    terminal
        .draw(|frame| {
            app.data.active_tab = Tab::Diff;
            render_diff(frame, &app, frame.area());
        })
        .expect("draw diff");
    let diff_fg = {
        let buffer = terminal.backend().buffer();
        let cell = cell_at(buffer, 1, 2);
        assert_eq!(cell.symbol(), "(");
        cell.fg
    };
    assert_eq!(diff_fg, colors::TEXT_MUTED);

    terminal
        .draw(|frame| {
            app.data.active_tab = Tab::Commits;
            render_commits(frame, &app, frame.area());
        })
        .expect("draw commits");
    let commits_fg = {
        let buffer = terminal.backend().buffer();
        let cell = cell_at(buffer, 1, 2);
        assert_eq!(cell.symbol(), "(");
        cell.fg
    };
    assert_eq!(commits_fg, colors::TEXT_MUTED);
}

#[test]
fn test_tab_bar_renders_unseen_commits_dot() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.selected = 1;
    app.data.active_tab = Tab::Preview;

    app.data.ui.diff_hash = 0;
    app.data.ui.commits_hash = 123;
    app.data
        .ui
        .set_commits_last_seen_hash_for_agent(agent_id, 0);

    let line = tab_bar_line(&app);
    assert!(line_text(&line).contains("◐ Commits"));
}

#[test]
fn test_tab_bar_hides_unseen_commits_dot_when_viewing_commits_tab() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.selected = 1;
    app.data.active_tab = Tab::Commits;

    app.data.ui.diff_hash = 0;
    app.data.ui.commits_hash = 123;
    app.data
        .ui
        .set_commits_last_seen_hash_for_agent(agent_id, 0);

    let line = tab_bar_line(&app);
    assert!(!line_text(&line).contains('◐'));
}

#[test]
fn test_tab_bar_hides_unseen_commits_dot_when_hash_seen() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.selected = 1;
    app.data.active_tab = Tab::Preview;

    app.data.ui.diff_hash = 0;
    app.data.ui.commits_hash = 123;
    app.data
        .ui
        .set_commits_last_seen_hash_for_agent(agent_id, 123);

    let line = tab_bar_line(&app);
    assert!(!line_text(&line).contains('◐'));
}

#[test]
fn test_tab_for_tab_bar_offset_selects_commits_and_none_after_end() {
    let (app, _temp) = create_test_app();

    let preview_w = tab_bar_tab_width("Preview", false);
    let diff_w = tab_bar_tab_width("Diff", false);
    let commits_w = tab_bar_tab_width("Commits", false);

    let diff_start = preview_w;
    let commits_start = diff_start.saturating_add(diff_w);
    let commits_end = commits_start.saturating_add(commits_w);

    assert_eq!(
        tab_for_tab_bar_offset(&app, commits_start),
        Some(Tab::Commits)
    );
    assert_eq!(tab_for_tab_bar_offset(&app, commits_end), None);
}

#[test]
fn test_tab_bar_tab_has_unseen_changes_preview_is_false() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    app.data.storage.add(agent);
    app.data.selected = 1;

    assert!(!tab_bar_tab_has_unseen_changes(&app, Tab::Preview));
}

#[test]
fn test_render_content_pane_renders_commits() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Commits;
    app.data
        .ui
        .set_commits_content("Branch: main\nCommits: main..HEAD (0 shown)");

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_content_pane(frame, &app, frame.area()))
        .expect("draw content pane");

    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("Git Commits"));
}

#[test]
fn test_render_preview_cursor_returns_on_invalid_state() {
    let (mut app, _temp) = create_test_app();

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 10, 10);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, true));
    app.data.ui.preview_pane_size = Some((40, 10));
    terminal
        .draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 10, 10);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, false));
    app.data.ui.preview_pane_size = None;
    terminal
        .draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 10, 10);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, false));
    app.data.ui.preview_pane_size = Some((40, 10));
    terminal
        .draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 10, 0);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, false));
    app.data.ui.preview_pane_size = Some((40, 5));
    terminal
        .draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 10, 1);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, false));
    app.data.ui.preview_pane_size = Some((40, 5));
    terminal
        .draw(|frame| {
            let mut area = frame.area();
            area.width = 0;
            render_preview_cursor(frame, &app, area, 0, 5, 5);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, false));
    app.data.ui.preview_pane_size = Some((40, 0));
    terminal
        .draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 10, 1);
        })
        .expect("draw preview cursor");

    app.data.ui.preview_cursor_position = Some((0, 0, false));
    app.data.ui.preview_pane_size = Some((40, 5));
    terminal
        .draw(|frame| {
            let mut area = frame.area();
            area.height = 0;
            render_preview_cursor(frame, &app, area, 0, 5, 5);
        })
        .expect("draw preview cursor");
}

#[test]
fn test_render_diff_focused_applies_styles_and_selection() {
    let (mut app, _temp) = create_test_app();

    let agent = Agent::new(
        "a".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    app.data.storage.add(agent);
    app.data.selected = 1;

    app.enter_mode(DiffFocusedMode.into());
    app.data.active_tab = Tab::Diff;
    let model = crate::git::DiffModel {
        files: vec![crate::git::DiffFile {
            path: std::path::PathBuf::from("file.txt"),
            status: crate::git::FileStatus::Modified,
            meta: Vec::new(),
            hunks: vec![crate::git::DiffHunk {
                header: "@@ -1,1 +1,2 @@".to_string(),
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 2,
                lines: vec![
                    crate::git::DiffHunkLine {
                        origin: '+',
                        content: "added".to_string(),
                        old_lineno: None,
                        new_lineno: Some(1),
                    },
                    crate::git::DiffHunkLine {
                        origin: '-',
                        content: "removed".to_string(),
                        old_lineno: Some(1),
                        new_lineno: None,
                    },
                    crate::git::DiffHunkLine {
                        origin: ' ',
                        content: "@@ inline".to_string(),
                        old_lineno: Some(2),
                        new_lineno: Some(2),
                    },
                    crate::git::DiffHunkLine {
                        origin: ' ',
                        content: "context".to_string(),
                        old_lineno: Some(3),
                        new_lineno: Some(3),
                    },
                ],
            }],
            additions: 1,
            deletions: 1,
        }],
        summary: crate::git::DiffSummary {
            files_changed: 1,
            additions: 1,
            deletions: 1,
        },
        hash: 1,
    };

    let (content, meta) = app.data.ui.build_diff_view(&model);
    app.data.ui.set_diff_view(content, meta);
    app.data.ui.diff_visual_anchor = Some(3);
    app.data.ui.diff_cursor = 4;

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_diff(frame, &app, frame.area()))
        .expect("draw diff");

    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("INTERACTIVE"));
    assert!(text.contains("+added"));
    assert!(text.contains("-removed"));
}

#[test]
fn test_render_commits_shows_selected_border_in_scrolling_mode() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Commits;
    app.data
        .ui
        .set_commits_content("Branch: main\nCommits: main..HEAD (0 shown)");

    app.enter_mode(ScrollingMode.into());

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_commits(frame, &app, frame.area()))
        .expect("draw commits");

    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("Git Commits"));
}

#[test]
fn test_scrollbars_return_when_scrollbar_area_is_empty() {
    let (_app, _temp) = create_test_app();

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            let area = frame.area();
            let content_area = Rect { height: 0, ..area };

            render_commits_scrollbar(frame, area, content_area, 10, 1, 9, 0);
            render_diff_scrollbar(frame, area, content_area, 10, 1, 9, 0);

            let mut zero_width_area = area;
            zero_width_area.width = 0;
            render_commits_scrollbar(frame, zero_width_area, zero_width_area, 10, 1, 9, 0);
            render_diff_scrollbar(frame, zero_width_area, zero_width_area, 10, 1, 9, 0);

            render_commits_scrollbar(frame, area, area, 1, 10, 0, 0);
            render_diff_scrollbar(frame, area, area, 1, 10, 0, 0);
        })
        .expect("draw scrollbars");
}

#[test]
fn test_render_commits_renders_subject_meta_and_body() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Commits;
    app.data.ui.set_commits_content(
        [
            "Branch: tenex/test",
            "Commits: main..HEAD (1 shown)",
            "abcdef1  Add thing",
            "  2026-01-11 12:34 • Test Author • (HEAD -> tenex/test)",
            "    This is the body.",
            "abc123  Too short (should not parse as hash)",
            "1234567890abcde  Too long (should not parse as hash)",
            "abcdef1 Add thing without delimiter",
            "zzzzzzz  Not hex (should not parse as hash)",
        ]
        .join("\n"),
    );

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_commits(frame, &app, frame.area()))
        .expect("draw commits");

    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("abcdef1"));
    assert!(text.contains("Add thing"));
    assert!(text.contains("Test Author"));
    assert!(text.contains("This is the body."));
    assert!(text.contains("Too short"));
    assert!(text.contains("Too long"));
    assert!(text.contains("without delimiter"));
    assert!(text.contains("Not hex"));
}

#[test]
fn test_render_commits_treats_branch_and_commits_markers_as_headers_even_when_late() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Commits;
    app.data.ui.set_commits_content(
        [
            "Intro line",
            "Another line",
            "Branch: tenex/test",
            "Commits: main..HEAD (0 shown)",
            "(No commits)",
            "abcdef1  Subject",
        ]
        .join("\n"),
    );

    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_commits(frame, &app, frame.area()))
        .expect("draw commits");

    let buffer = terminal.backend().buffer();
    let content_x = 1u16;
    let content_y = 2u16;
    for (offset, symbol) in [
        (0u16, "I"),
        (1u16, "A"),
        (2u16, "B"),
        (3u16, "C"),
        (4u16, "("),
    ] {
        let cell = cell_at(buffer, content_x, content_y.saturating_add(offset));
        assert_eq!(cell.symbol(), symbol);
        assert_eq!(cell.fg, colors::TEXT_MUTED);
    }

    let subject_cell = cell_at(buffer, content_x, content_y.saturating_add(5u16));
    assert_eq!(subject_cell.symbol(), "a");
    assert_eq!(subject_cell.fg, colors::DIFF_HUNK);
}

#[test]
fn test_render_commits_renders_scrollbar_when_overflowing() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Commits;

    let mut lines = Vec::new();
    lines.push("Branch: tenex/test".to_string());
    lines.push("Commits: main..HEAD (100 shown)".to_string());
    for idx in 0..100 {
        lines.push(format!("{idx:07x}  Commit {idx}"));
    }

    app.data.ui.set_commits_content(lines.join("\n"));

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_commits(frame, &app, frame.area()))
        .expect("draw commits");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(has_light | has_dark);
}

#[test]
fn test_render_preview_hides_scrollbar_when_following() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Preview;

    app.data.ui.set_preview_content(
        (0..50)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.preview_follow = true;
    app.data.ui.preview_scroll = usize::MAX;

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_preview(frame, &app, frame.area()))
        .expect("draw preview");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(!(has_light | has_dark));
}

#[test]
fn test_render_preview_shows_scrollbar_when_paused() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Preview;

    app.data.ui.set_preview_content(
        (0..50)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.preview_follow = false;
    app.data.ui.preview_scroll = 0;

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_preview(frame, &app, frame.area()))
        .expect("draw preview");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(has_light | has_dark);
}

#[test]
fn test_render_preview_handles_zero_width() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Preview;
    app.data.ui.set_preview_content(
        (0..50)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.preview_follow = false;
    app.data.ui.preview_scroll = 0;

    let backend = TestBackend::new(10, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            let mut area = frame.area();
            area.width = 0;
            render_preview(frame, &app, area);
        })
        .expect("draw preview");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(!(has_light | has_dark));
}

#[test]
fn test_render_preview_skips_scrollbar_when_inner_height_zero() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Preview;
    app.data.ui.set_preview_content("hello");
    app.data.ui.preview_follow = false;
    app.data.ui.preview_scroll = 0;

    let backend = TestBackend::new(20, 3);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_preview(frame, &app, frame.area()))
        .expect("draw preview");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(!(has_light | has_dark));
}

#[test]
fn test_diff_selection_range_returns_none_when_anchor_missing() {
    let (mut app, _temp) = create_test_app();
    app.data.ui.diff_visual_anchor = None;
    app.data.ui.diff_cursor = 5;

    assert_eq!(diff_selection_range(&app), None);
}

#[test]
fn test_render_diff_does_not_style_file_header_markers_as_add_remove_lines() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = Tab::Diff;

    app.data.storage.add(Agent::new(
        "a".to_string(),
        "echo".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    ));
    app.data.selected = 1;

    let content = ["+++ b/file.txt", "--- a/file.txt"].join("\n");
    app.data.ui.set_diff_view(
        content,
        vec![
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 0,
            },
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 1,
            },
        ],
    );

    let backend = TestBackend::new(60, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_diff(frame, &app, frame.area()))
        .expect("draw diff");

    let buffer = terminal.backend().buffer();
    let first_plus = cell_at(buffer, 1, 2);
    assert_eq!(first_plus.symbol(), "+");
    assert_eq!(first_plus.fg, colors::TEXT_PRIMARY);

    let first_dash = cell_at(buffer, 1, 3);
    assert_eq!(first_dash.symbol(), "-");
    assert_eq!(first_dash.fg, colors::TEXT_PRIMARY);
}

#[test]
fn test_render_agent_list_skips_scrollbar_when_not_overflowing() {
    let (mut app, _temp) = create_test_app();
    app.data.storage.add(Agent::new(
        "a".to_string(),
        "echo".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    ));
    app.data.selected = 1;

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_agent_list(frame, &app, frame.area()))
        .expect("draw agent list");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(!(has_light | has_dark));
}

#[test]
fn test_render_agent_list_renders_scrollbar_when_overflowing() {
    let (mut app, _temp) = create_test_app();
    for idx in 0..4 {
        app.data.storage.add(Agent::new(
            format!("agent-{idx}"),
            "echo".to_string(),
            format!("branch-{idx}"),
            std::path::PathBuf::from("/tmp"),
        ));
    }
    app.data.selected = 1;

    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_agent_list(frame, &app, frame.area()))
        .expect("draw agent list");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(has_light | has_dark);
}

#[test]
fn test_render_agent_list_handles_zero_width() {
    let (mut app, _temp) = create_test_app();
    app.data.storage.add(Agent::new(
        "a".to_string(),
        "echo".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    ));
    app.data.selected = 1;

    let backend = TestBackend::new(10, 2);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            let area = Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 2,
            };
            render_agent_list(frame, &app, area);
        })
        .expect("draw agent list");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(!(has_light | has_dark));
}

#[test]
fn test_render_agent_list_skips_scrollbar_when_inner_height_zero() {
    let (mut app, _temp) = create_test_app();
    app.data.storage.add(Agent::new(
        "a".to_string(),
        "echo".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    ));
    app.data.selected = 1;

    let backend = TestBackend::new(10, 2);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| render_agent_list(frame, &app, frame.area()))
        .expect("draw agent list");

    let text = buffer_text(terminal.backend().buffer());
    let has_light = text.contains('░');
    let has_dark = text.contains('█');
    assert!(!(has_light | has_dark));
}

#[test]
fn test_agent_list_item_renders_docker_badge_and_plain_dir_suffix() {
    let (app, _temp) = create_test_app();
    let mut agent = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        "branch".to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    agent.runtime = AgentRuntime::Docker;
    agent.workspace_kind = WorkspaceKind::PlainDir;
    agent.status = Status::Running;

    let info = crate::agent::VisibleAgentInfo {
        agent: &agent,
        depth: 0,
        has_children: false,
        child_count: 0,
    };

    let item = agent_list_item(&app, 0, &info);
    let list = List::new(vec![item]);
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).expect("Expected test terminal");
    terminal
        .draw(|frame| frame.render_widget(list, frame.area()))
        .expect("Expected render");

    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("[D]"));
    assert!(text.contains("(no-git)"));
}

#[test]
fn test_project_list_item_uses_selected_style_when_index_matches_selection() {
    let (mut app, _temp) = create_test_app();
    app.data.selected = 0;

    let project = SidebarProject {
        root: std::path::PathBuf::from("/tmp/project"),
        label: "project".to_string(),
        collapsed: false,
        agent_count: 1,
    };

    let selected = project_list_item(&app, 0, &project);
    let not_selected = project_list_item(&app, 1, &project);
    let list = List::new(vec![selected, not_selected]);
    let backend = TestBackend::new(40, 2);
    let mut terminal = Terminal::new(backend).expect("Expected test terminal");
    terminal
        .draw(|frame| frame.render_widget(list, frame.area()))
        .expect("Expected render");

    let buffer = terminal.backend().buffer();
    let selected_cell = buffer.cell((2, 0)).expect("Expected selected project cell");
    assert_eq!(selected_cell.symbol(), "p");
    assert_eq!(selected_cell.bg, colors::SURFACE_HIGHLIGHT);
    assert_eq!(selected_cell.fg, colors::TEXT_PRIMARY);

    let not_selected_cell = buffer
        .cell((2, 1))
        .expect("Expected unselected project cell");
    assert_eq!(not_selected_cell.symbol(), "p");
    assert_eq!(not_selected_cell.bg, colors::SURFACE);
    assert_eq!(not_selected_cell.fg, colors::TEXT_DIM);
}

#[test]
fn test_project_list_item_renders_collapsed_indicator_when_collapsed() {
    let (mut app, _temp) = create_test_app();
    app.data.selected = 0;

    let project = SidebarProject {
        root: std::path::PathBuf::from("/tmp/project"),
        label: "project".to_string(),
        collapsed: true,
        agent_count: 1,
    };

    let item = project_list_item(&app, 0, &project);
    let list = List::new(vec![item]);
    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(list, frame.area()))
        .expect("draw project list");

    let cell = cell_at(terminal.backend().buffer(), 0, 0);
    assert_eq!(cell.symbol(), "▶");
}
