//! Help overlay rendering

use crate::app::App;
use crate::config::{Action, get_display_description, get_display_keys};
use ratatui::layout::Margin;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Parse a description with `[x]` mnemonic patterns and return styled spans.
/// The bracketed content is highlighted (bold), brackets are dimmed.
fn styled_mnemonic_description(description: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = description;

    while let Some(start) = remaining.find('[') {
        // Add text before the bracket
        if start > 0 {
            spans.push(Span::styled(
                remaining[..start].to_string(),
                Style::default().fg(colors::TEXT_PRIMARY),
            ));
        }

        // Find the closing bracket
        if let Some(end) = remaining[start..].find(']') {
            let end = start + end;
            let bracket_content = &remaining[start + 1..end];

            // Add styled bracket and content
            spans.push(Span::styled(
                "[".to_string(),
                Style::default().fg(colors::TEXT_DIM),
            ));
            spans.push(Span::styled(
                bracket_content.to_string(),
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                "]".to_string(),
                Style::default().fg(colors::TEXT_DIM),
            ));

            remaining = &remaining[end + 1..];
        } else {
            // No closing bracket, add rest as-is
            spans.push(Span::styled(
                remaining[start..].to_string(),
                Style::default().fg(colors::TEXT_PRIMARY),
            ));
            remaining = "";
        }
    }

    // Add any remaining text after the last bracket
    if !remaining.is_empty() {
        spans.push(Span::styled(
            remaining.to_string(),
            Style::default().fg(colors::TEXT_PRIMARY),
        ));
    }

    spans
}

/// Render the help overlay
pub fn render_help_overlay(frame: &mut Frame<'_>, app: &App) {
    let merge_key_remapped = app.is_merge_key_remapped();

    let mut help_text = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    let mut current_group = None;
    for &action in Action::ALL_FOR_HELP {
        let group = action.group();

        // Add section header when group changes
        if current_group != Some(group) {
            if current_group.is_some() {
                help_text.push(Line::from(""));
            }
            help_text.push(Line::from(Span::styled(
                group.title(),
                Style::default().fg(colors::TEXT_DIM),
            )));
            current_group = Some(group);
        }

        // Build help line with styled mnemonics
        // Use dynamic display functions for keyboard remap support
        let key_str = get_display_keys(action, merge_key_remapped);
        let description = get_display_description(action, merge_key_remapped);

        let mut spans = vec![Span::styled(
            format!("  {key_str:<10} "),
            Style::default().fg(colors::TEXT_DIM),
        )];
        spans.extend(styled_mnemonic_description(description));

        help_text.push(Line::from(spans));
    }

    help_text.push(Line::from(""));
    help_text.push(Line::from(Span::styled(
        "Scroll: ↑/↓, Ctrl+u/d, g/G",
        Style::default().fg(colors::TEXT_MUTED),
    )));
    help_text.push(Line::from(Span::styled(
        "Any other key closes",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let total_lines = help_text.len();

    // Size the modal to fit content when possible; otherwise take as much height as we can.
    let max_height = frame.area().height.saturating_sub(4);
    let min_height = 12u16.min(max_height);
    let desired_height = u16::try_from(total_lines)
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let height = desired_height.min(max_height).max(min_height);

    let area = centered_rect_absolute(50, height, frame.area());

    let visible_height = usize::from(area.height.saturating_sub(2));
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.data.ui.help_scroll.min(max_scroll);
    let scroll_pos = u16::try_from(scroll).unwrap_or(u16::MAX);

    let paragraph = Paragraph::new(help_text)
        .scroll((scroll_pos, 0))
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::SELECTED))
                .border_type(colors::BORDER_TYPE),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);

    if total_lines > visible_height {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for cell in &buffer.content {
            text.push_str(cell.symbol());
        }
        text
    }

    #[test]
    fn test_styled_mnemonic_description_roundtrips_with_prefix_text() {
        let spans = styled_mnemonic_description("Press [x] to exit");
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "Press [x] to exit");
    }

    #[test]
    fn test_styled_mnemonic_description_roundtrips_with_leading_mnemonic() {
        let spans = styled_mnemonic_description("[x] to exit");
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "[x] to exit");
    }

    #[test]
    fn test_styled_mnemonic_description_returns_unclosed_bracket_text_verbatim() {
        let spans = styled_mnemonic_description("Use [x to exit");
        let rendered = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(rendered, "Use [x to exit");
    }

    #[test]
    fn test_render_help_overlay_renders_scrollbar_when_content_exceeds_height() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).expect("terminal");

        let app = App::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );
        terminal
            .draw(|frame| {
                render_help_overlay(frame, &app);
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("░"));
    }

    #[test]
    fn test_render_help_overlay_skips_scrollbar_when_inner_area_is_too_small() {
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).expect("terminal");

        let app = App::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );
        terminal
            .draw(|frame| {
                render_help_overlay(frame, &app);
            })
            .expect("draw");

        assert!(!terminal.backend().buffer().content.is_empty());
    }

    #[test]
    fn test_render_help_overlay_skips_scrollbar_when_scrollbar_area_has_zero_height() {
        let backend = TestBackend::new(80, 6);
        let mut terminal = Terminal::new(backend).expect("terminal");

        let app = App::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );
        terminal
            .draw(|frame| {
                render_help_overlay(frame, &app);
            })
            .expect("draw");

        assert!(!terminal.backend().buffer().content.is_empty());
    }

    #[test]
    fn test_render_help_overlay_does_not_render_scrollbar_when_height_is_sufficient() {
        let backend = TestBackend::new(80, 200);
        let mut terminal = Terminal::new(backend).expect("terminal");

        let app = App::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );
        terminal
            .draw(|frame| {
                render_help_overlay(frame, &app);
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(!text.contains("░"));
    }
}
