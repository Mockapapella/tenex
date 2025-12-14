//! Slash command palette modal rendering (`/`)

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tenex::app::App;

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the command palette overlay
pub fn render_command_palette_overlay(frame: &mut Frame<'_>, app: &App) {
    let filtered = app.filtered_slash_commands();
    let total_count = filtered.len();

    let max_visible: usize = 8;
    let visible_count = total_count.min(max_visible).max(1);

    // Header + blank + list + blank + help
    let content_height = 1u16 + 1u16 + u16::try_from(visible_count).unwrap_or(1) + 1u16 + 1u16;
    let total_height = content_height.saturating_add(2); // borders

    let area = centered_rect_absolute(60, total_height, frame.area());

    // Insert cursor marker at cursor position
    let input = app.input.buffer.as_str();
    let cursor_pos = app.input.cursor;
    let text_with_cursor = if cursor_pos >= input.len() {
        format!("{input}│")
    } else {
        let before = &input[..cursor_pos];
        let after = &input[cursor_pos..];
        format!("{before}│{after}")
    };

    let mut lines: Vec<Line<'_>> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("Command: ", Style::default().fg(colors::TEXT_DIM)),
        Span::styled(
            text_with_cursor,
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    let selected_idx = app
        .command_palette
        .selected
        .min(total_count.saturating_sub(1));
    let scroll_offset = if selected_idx >= max_visible {
        selected_idx - max_visible + 1
    } else {
        0
    };

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching commands",
            Style::default().fg(colors::TEXT_MUTED),
        )));
    } else {
        for (idx, cmd) in filtered
            .iter()
            .copied()
            .enumerate()
            .skip(scroll_offset)
            .take(max_visible)
        {
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

            let mut name = cmd.name.to_string();
            if !cmd.description.is_empty() {
                name.push_str("  ");
                name.push_str(cmd.description);
            }

            lines.push(Line::from(Span::styled(format!("{prefix}{name}"), style)));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select • Enter run • Esc cancel • Type to filter",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Commands ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
