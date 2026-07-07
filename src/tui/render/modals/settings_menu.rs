//! Agent role selection modal rendering (`/agents`)

use crate::app::{AgentRole, App};
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the settings menu overlay.
pub fn render_settings_menu_overlay(frame: &mut Frame<'_>, app: &App) {
    // Header + blank + 3 items + blank + help = 7, plus borders = 9.
    let area = centered_rect_absolute(60, 9, frame.area());

    let total = AgentRole::ALL.len();
    let selected_idx = app.data.settings_menu.selected.min(total.saturating_sub(1));

    let mut lines: Vec<Line<'_>> = Vec::new();

    lines.push(Line::from(vec![Span::styled(
        "Choose which agent type to configure:",
        Style::default().fg(colors::TEXT_DIM),
    )]));
    lines.push(Line::from(""));

    for (idx, role) in AgentRole::ALL.iter().copied().enumerate() {
        let program = match role {
            AgentRole::Default => app.data.settings.agent_program,
            AgentRole::Planner => app.data.settings.planner_agent_program,
            AgentRole::Review => app.data.settings.review_agent_program,
        };

        let is_selected = idx == selected_idx;
        let style = if is_selected {
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .bg(colors::SURFACE_HIGHLIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::TEXT_PRIMARY)
        };

        let prefix = if is_selected { "▶ " } else { "  " };
        lines.push(Line::from(Span::styled(
            format!("{prefix}{}  ({})", role.menu_label(), program.label()),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select • Enter edit • Esc cancel",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Agents ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::SELECTED))
                .border_type(colors::BORDER_TYPE),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests;
