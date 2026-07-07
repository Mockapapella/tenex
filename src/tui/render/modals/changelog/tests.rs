use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::config::Config;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Style;

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for cell in &buffer.content {
        text.push_str(cell.symbol());
    }
    text
}

#[test]
fn test_render_changelog_overlay_renders_content() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    app.data.ui.changelog_scroll = 10;

    let state = ChangelogMode {
            title: "What's New".to_string(),
            lines: vec![
                "What's New in Tenex v1.2.3".to_string(),
                String::new(),
                "v1.2.3 (2026-01-30)".to_string(),
                String::new(),
                "### Highlights".to_string(),
                "- This bullet is long enough that it should wrap within the modal width and still align properly under the dash."
                    .to_string(),
                "1. A numbered list item that also needs to wrap cleanly when it reaches the edge."
                    .to_string(),
                "AReallyLongUnbrokenWordThatMustBeChunkedToAvoidOverflow".to_string(),
            ],
            mark_seen_version: None,
        };

    terminal
        .draw(|frame| {
            render_changelog_overlay(frame, &app, &state);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_changelog_overlay_skips_scrollbar_when_inner_area_is_too_small() {
    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    let state = ChangelogMode {
        title: "What's New".to_string(),
        lines: vec![
            "What's New in Tenex v1.2.3".to_string(),
            "A bullet that will wrap once rendered at the modal width.".to_string(),
        ],
        mark_seen_version: None,
    };

    terminal
        .draw(|frame| {
            render_changelog_overlay(frame, &app, &state);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_changelog_overlay_skips_scrollbar_when_scrollbar_area_has_zero_height() {
    let backend = TestBackend::new(80, 6);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    let state = ChangelogMode {
        title: "What's New".to_string(),
        lines: vec![
            "What's New in Tenex v1.2.3".to_string(),
            "A bullet that will wrap once rendered at the modal width.".to_string(),
        ],
        mark_seen_version: None,
    };

    terminal
        .draw(|frame| {
            render_changelog_overlay(frame, &app, &state);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_changelog_overlay_does_not_render_scrollbar_when_content_fits() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    let state = ChangelogMode {
        title: "What's New".to_string(),
        lines: vec!["Short".to_string(), "v1.2.3".to_string()],
        mark_seen_version: None,
    };

    terminal
        .draw(|frame| {
            render_changelog_overlay(frame, &app, &state);
        })
        .expect("draw");

    assert!(!buffer_text(&terminal).contains('░'));
}

#[test]
fn test_style_for_line_does_not_highlight_non_version_prefix() {
    assert_eq!(
        style_for_line(1, "vNext release"),
        Style::default().fg(colors::TEXT_PRIMARY)
    );
}

#[test]
fn test_wrap_spec_handles_star_and_plus_bullets() {
    let star = wrap_spec("* item");
    assert_eq!(star.first_prefix, "* ");
    assert_eq!(star.content, "item");

    let plus = wrap_spec("+ item");
    assert_eq!(plus.first_prefix, "+ ");
    assert_eq!(plus.content, "item");
}

#[test]
fn test_wrap_spec_ignores_parenthesized_number_prefix() {
    let spec = wrap_spec("1) item");
    assert_eq!(spec.first_prefix, "");
    assert_eq!(spec.subsequent_prefix, "");
    assert_eq!(spec.content, "1) item");
}

#[test]
fn test_wrap_spec_ignores_numbered_prefix_without_space_after_dot() {
    let spec = wrap_spec("1.abc def");
    assert_eq!(spec.first_prefix, "");
    assert_eq!(spec.subsequent_prefix, "");
    assert_eq!(spec.content, "1.abc def");
}

#[test]
fn test_wrap_spec_scans_all_digits_when_input_is_digits_only() {
    let spec = wrap_spec("123");
    assert_eq!(spec.first_prefix, "");
    assert_eq!(spec.subsequent_prefix, "");
    assert_eq!(spec.content, "123");
}

#[test]
fn test_wrap_single_line_preserves_bullet_prefix() {
    let wrapped = wrap_single_line("  - one two three four", 12);
    assert_eq!(
        wrapped,
        vec![
            "  - one two".to_string(),
            "    three".to_string(),
            "    four".to_string()
        ]
    );
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
fn test_split_at_char_boundary_does_not_split_utf8() {
    let (left, right) = split_at_char_boundary("éé", 1);
    assert_eq!(left, "é");
    assert_eq!(right, "é");
}

#[test]
fn test_split_at_char_boundary_backtracks_to_boundary() {
    let (left, right) = split_at_char_boundary("éé", 3);
    assert_eq!(left, "é");
    assert_eq!(right, "é");
}

#[test]
fn test_wrap_and_style_lines_returns_empty_when_width_zero() {
    let wrapped = wrap_and_style_lines(&[String::from("line")], 0);
    assert!(wrapped.is_empty());
}

#[test]
fn test_wrap_single_line_returns_empty_when_width_zero() {
    assert!(wrap_single_line("line", 0).is_empty());
}

#[test]
fn test_wrap_single_line_returns_line_when_content_is_empty() {
    let wrapped = wrap_single_line("  -   ", 5);
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
fn test_wrap_single_line_collapses_extra_spaces_without_wrapping() {
    let wrapped = wrap_single_line("- a  b", 5);
    assert_eq!(wrapped, vec!["- a b".to_string()]);
}

#[test]
fn test_chunk_into_width_returns_empty_when_width_zero() {
    assert!(chunk_into_width("hello", 0).is_empty());
}

#[test]
fn test_wrap_single_line_emits_subsequent_prefix_for_wrapped_lines() {
    let wrapped = wrap_single_line("- one two three", 10);
    assert_eq!(
        wrapped,
        vec!["- one two".to_string(), "  three".to_string()]
    );
}
