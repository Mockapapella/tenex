//! Progress-style modal rendering.

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render acknowledgement while a user-requested operation completes.
pub fn render_working_modal(frame: &mut Frame<'_>, title: &str, message: &str) {
    let body = vec![
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(colors::SELECTED)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            message,
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Please wait. Tenex will continue automatically.",
            Style::default().fg(colors::TEXT_MUTED),
        )),
    ];
    let area = centered_rect_absolute(58, 9, frame.area());
    let paragraph = Paragraph::new(body)
        .block(
            Block::default()
                .title(format!(" {title} "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::SELECTED))
                .border_type(colors::BORDER_TYPE),
        )
        .style(Style::default().bg(colors::MODAL_BG))
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
