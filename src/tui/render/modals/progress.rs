//! Progress-style modal rendering.

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the first-use Docker preparation modal.
pub fn render_preparing_docker_modal(frame: &mut Frame<'_>, message: &str) {
    let max_line_width = 44;
    let mut lines: Vec<Line<'_>> = Vec::new();

    lines.push(Line::from(Span::styled(
        "[D] Preparing Docker",
        Style::default()
            .fg(colors::DOCKER_BADGE)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

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

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Please wait. Tenex will continue automatically.",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX).max(7);
    let area = centered_rect_absolute(50, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Docker ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::DOCKER_BADGE))
                .border_type(colors::BORDER_TYPE),
        )
        .style(Style::default().bg(colors::MODAL_BG))
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_render_preparing_docker_modal() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| {
            render_preparing_docker_modal(
                frame,
                "Building the shipped Tenex Docker worker image for first use.",
            );
        })?;

        Ok(())
    }
}
