//! Picker modal rendering (count pickers, review info)

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tenex::app::App;

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the child count picker overlay
pub fn render_count_picker_overlay(frame: &mut Frame<'_>, app: &App) {
    // 10 lines of content + 2 for borders = 12 lines
    let area = centered_rect_absolute(40, 12, frame.area());

    let context = if app.spawn.spawning_under.is_some() {
        "Add children to selected agent"
    } else {
        "Create new agent with children"
    };

    let text = vec![
        Line::from(Span::styled(context, Style::default().fg(colors::TEXT_DIM))),
        Line::from(""),
        Line::from(Span::styled(
            "How many child agents?",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "        ▲",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(Span::styled(
            format!("        {}", app.spawn.child_count),
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "        ▼",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "↑/k to increase, ↓/j to decrease",
            Style::default().fg(colors::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            "Enter to continue, Esc to cancel",
            Style::default().fg(colors::TEXT_MUTED),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Spawn Children ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// Render the review info overlay
pub fn render_review_info_overlay(frame: &mut Frame<'_>) {
    let area = centered_rect_absolute(50, 9, frame.area());

    let text = vec![
        Line::from(Span::styled(
            "Select an Agent First",
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "To spawn review agents, you must first",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(Span::styled(
            "select an agent that has a worktree.",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Use j/k or ↑/↓ to navigate the agent list.",
            Style::default().fg(colors::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            "Press any key to dismiss",
            Style::default().fg(colors::TEXT_MUTED),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Review ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// Render the review count picker overlay
pub fn render_review_count_picker_overlay(frame: &mut Frame<'_>, app: &App) {
    // 10 lines of content + 2 for borders = 12 lines
    let area = centered_rect_absolute(40, 12, frame.area());

    let text = vec![
        Line::from(Span::styled(
            "Spawn review agents for selected worktree",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "How many review agents?",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "        ▲",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(Span::styled(
            format!("        {}", app.spawn.child_count),
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "        ▼",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "↑/k to increase, ↓/j to decrease",
            Style::default().fg(colors::TEXT_MUTED),
        )),
        Line::from(Span::styled(
            "Enter to continue, Esc to cancel",
            Style::default().fg(colors::TEXT_MUTED),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Review Agents ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
