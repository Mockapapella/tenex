//! Changelog / "What's New" overlay rendering.

use crate::app::App;
use crate::state::ChangelogMode;
use ratatui::layout::Margin;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the changelog overlay.
pub fn render_changelog_overlay(frame: &mut Frame<'_>, app: &App, state: &ChangelogMode) {
    let total_lines = state.lines.len();

    // Similar sizing behavior to the help overlay: attempt to fit, but don't exceed the frame.
    let max_height = frame.area().height.saturating_sub(4);
    let min_height = 12u16.min(max_height);
    let desired_height = u16::try_from(total_lines)
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    let area = centered_rect_absolute(60, height, frame.area());

    let visible_height = usize::from(area.height.saturating_sub(2));
    let inner_width = area.width.saturating_sub(2);

    let mut wrapped = wrap_and_style_lines(&state.lines, inner_width);
    if inner_width != 0 && wrapped.len() > visible_height {
        let reserved_width = inner_width.saturating_sub(1);
        if reserved_width != inner_width {
            wrapped = wrap_and_style_lines(&state.lines, reserved_width);
        }
    }

    let wrapped_lines = wrapped.len();
    let max_scroll = wrapped_lines.saturating_sub(visible_height);
    let scroll = app.data.ui.changelog_scroll.min(max_scroll);
    let scroll_pos = u16::try_from(scroll).unwrap_or(u16::MAX);

    let paragraph = Paragraph::new(wrapped)
        .scroll((scroll_pos, 0))
        .block(
            Block::default()
                .title(format!(" {} ", state.title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::ACCENT_POSITIVE))
                .border_type(colors::BORDER_TYPE),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);

    if wrapped_lines > visible_height && area.width != 0 {
        let scrollbar_area = area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        });

        if scrollbar_area.width != 0 && scrollbar_area.height != 0 {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("░"))
                .track_style(Style::default().fg(colors::TEXT_MUTED))
                .thumb_style(Style::default().fg(colors::TEXT_PRIMARY));

            let mut scrollbar_state = ScrollbarState::new(max_scroll.saturating_add(1))
                .position(scroll)
                .viewport_content_length(visible_height);

            frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
        }
    }
}

fn wrap_and_style_lines(lines: &[String], width: u16) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let width = usize::from(width);
    let mut out = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if line.is_empty() {
            out.push(Line::from(""));
            continue;
        }

        let style = style_for_line(idx, line);
        for wrapped in wrap_single_line(line, width) {
            out.push(Line::from(Span::styled(wrapped, style)));
        }
    }

    out
}

fn style_for_line(idx: usize, line: &str) -> Style {
    if idx == 0 {
        return Style::default()
            .fg(colors::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD);
    }

    if line.starts_with('v') && line.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit()) {
        return Style::default()
            .fg(colors::ACCENT_POSITIVE)
            .add_modifier(Modifier::BOLD);
    }

    if line.starts_with("### ") {
        return Style::default()
            .fg(colors::TEXT_DIM)
            .add_modifier(Modifier::BOLD);
    }

    Style::default().fg(colors::TEXT_PRIMARY)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_render_changelog_overlay_renders_content() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
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

        terminal.draw(|frame| {
            render_changelog_overlay(frame, &app, &state);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
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
}
