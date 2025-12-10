//! Error modal rendering

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render an error modal with word-wrapped message
pub fn render_error_modal(frame: &mut Frame<'_>, message: &str) {
    // Wrap the error message to fit within the modal width (44 chars after padding)
    let max_line_width = 44;
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Add error icon and header
    lines.push(Line::from(Span::styled(
        "✖ Error",
        Style::default()
            .fg(colors::MODAL_BORDER_ERROR)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Word-wrap the message
    let words: Vec<&str> = message.split_whitespace().collect();
    let mut current_line = String::new();

    for word in words {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= max_line_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(Line::from(Span::styled(
                current_line.clone(),
                Style::default().fg(colors::TEXT_PRIMARY),
            )));
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(Line::from(Span::styled(
            current_line,
            Style::default().fg(colors::TEXT_PRIMARY),
        )));
    }

    // Add dismiss hint
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press any key to dismiss",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    // Height: content lines + 2 for borders, min 7 lines
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX).max(7);
    let area = centered_rect_absolute(50, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Error ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::MODAL_BORDER_ERROR)),
        )
        .style(Style::default().bg(colors::MODAL_BG))
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
