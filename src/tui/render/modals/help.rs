//! Help overlay rendering

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tenex::config::{Action, get_display_description, get_display_keys};

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
                remaining.to_string(),
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
pub fn render_help_overlay(frame: &mut Frame<'_>, merge_key_remapped: bool) {
    // Calculate height: header(2) + sections with actions + footer(2) + borders(2)
    // 5 sections with headers(5) + empty lines between(4) + 19 actions + footer(2) = 30 + 2 borders
    let area = centered_rect_absolute(50, 32, frame.area());

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
        "Press any key to close",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
