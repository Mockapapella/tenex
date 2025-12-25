//! Model selector modal rendering (`/agents`)

use crate::app::App;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the model selector overlay
pub fn render_model_selector_overlay(frame: &mut Frame<'_>, app: &App) {
    // 10 lines of content + 2 for borders = 12 lines
    let area = centered_rect_absolute(55, 12, frame.area());

    let filtered = app.filtered_model_programs();
    let selected_idx = app.model_selector.selected;
    let current = app.settings.agent_program;

    let mut lines: Vec<Line<'_>> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Current: ", Style::default().fg(colors::TEXT_DIM)),
        Span::styled(
            current.label(),
            Style::default()
                .fg(colors::ACCENT_POSITIVE)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    lines.push(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(colors::TEXT_DIM)),
        Span::styled(
            format!("{}_", &app.model_selector.filter),
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(""));

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching models",
            Style::default().fg(colors::TEXT_MUTED),
        )));
    } else {
        for (idx, program) in filtered.iter().copied().enumerate() {
            let is_cursor = idx == selected_idx;
            let is_current = program == current;

            let row_style = if is_cursor {
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .bg(colors::SURFACE_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::TEXT_PRIMARY)
            };

            let cursor = if is_cursor { "▶ " } else { "  " };
            let check = if is_current { "✓ " } else { "  " };

            lines.push(Line::from(Span::styled(
                format!("{cursor}{check}{}", program.label()),
                row_style,
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select • Enter confirm • Esc cancel • Type to filter",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Models ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
