//! Input modal rendering

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tenex::app::App;

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render a text input overlay
#[expect(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    reason = "Complex but cohesive rendering; casts are bounded by max_input_height=20"
)]
pub fn render_input_overlay(
    frame: &mut Frame<'_>,
    title: &str,
    prompt: &str,
    input: &str,
    cursor_pos: usize,
) {
    // Insert cursor marker at cursor position
    let text_with_cursor = if cursor_pos >= input.len() {
        format!("{input}│")
    } else {
        let before = &input[..cursor_pos];
        let after = &input[cursor_pos..];
        format!("{before}│{after}")
    };

    // Split into lines
    let input_lines: Vec<String> = if text_with_cursor.is_empty() {
        vec!["│".to_string()]
    } else {
        text_with_cursor.lines().map(String::from).collect()
    };

    let num_input_lines = input_lines.len();

    // Expandable height: min 3 lines for input, max 20, then scroll
    let min_input_height = 3_usize;
    let max_input_height = 20_usize;
    let input_area_height = num_input_lines.clamp(min_input_height, max_input_height);

    // Find which line has the cursor for auto-scroll
    let cursor_line = text_with_cursor[..text_with_cursor.find('│').unwrap_or(0)]
        .matches('\n')
        .count();

    // Calculate scroll to keep cursor visible
    let scroll_offset = if cursor_line >= input_area_height {
        cursor_line - input_area_height + 1
    } else {
        0
    };

    // Total height: borders(2) + prompt(1) + empty(1) + input area + empty(1) + help(1)
    let total_height = (6 + input_area_height) as u16;
    let area = centered_rect_absolute(60, total_height, frame.area());

    // Check if scrolling is needed
    let needs_scrollbar = num_input_lines > input_area_height;

    // Calculate inner area for the input box (after removing borders and prompt)
    // Reserve 1 column for scrollbar if needed
    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 3, // After border + prompt + empty line
        width: area
            .width
            .saturating_sub(if needs_scrollbar { 3 } else { 2 }),
        height: input_area_height as u16,
    };

    // Get visible lines with scroll
    let visible_lines: Vec<Line<'_>> = input_lines
        .iter()
        .skip(scroll_offset)
        .take(input_area_height)
        .map(|line| {
            Line::from(Span::styled(
                line.clone(),
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    // Build content: prompt, empty, (input rendered separately), empty, help
    let header = vec![
        Line::from(Span::styled(
            prompt,
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
    ];

    // Pad for input area
    let mut content_lines = header;
    for _ in 0..input_area_height {
        content_lines.push(Line::from(""));
    }
    content_lines.push(Line::from(""));
    content_lines.push(Line::from(Span::styled(
        "Enter: submit | Alt+Enter: newline | ←→↑↓: move | Esc: cancel",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let paragraph = Paragraph::new(content_lines)
        .block(
            Block::default()
                .title(format!(" {title} "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);

    // Render input area with different background
    let input_paragraph =
        Paragraph::new(visible_lines).style(Style::default().bg(colors::INPUT_BG));
    frame.render_widget(input_paragraph, inner_area);

    // Render scrollbar if needed
    if needs_scrollbar {
        let scrollbar_area = Rect {
            x: inner_area.x + inner_area.width,
            y: inner_area.y,
            width: 1,
            height: input_area_height as u16,
        };

        // Calculate thumb position and size
        let total_lines = num_input_lines;
        let visible_height = input_area_height;
        let thumb_height = ((visible_height * visible_height) / total_lines).max(1);
        let max_scroll = total_lines - visible_height;
        let thumb_pos = if max_scroll > 0 {
            (scroll_offset * (visible_height - thumb_height)) / max_scroll
        } else {
            0
        };

        // Render scrollbar track and thumb
        for i in 0..visible_height {
            let ch = if i >= thumb_pos && i < thumb_pos + thumb_height {
                "█" // Thumb
            } else {
                "░" // Track
            };
            let scrollbar_cell = Rect {
                x: scrollbar_area.x,
                y: scrollbar_area.y + i as u16,
                width: 1,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(ch).style(Style::default().fg(colors::TEXT_DIM)),
                scrollbar_cell,
            );
        }
    }
}

/// Render the rename overlay
pub fn render_rename_overlay(frame: &mut Frame<'_>, app: &App) {
    let is_root = app.git_op.is_root_rename;

    let (title, description) = if is_root {
        (
            "Rename Agent",
            "Renames agent title, branch, and tmux session:",
        )
    } else {
        ("Rename Agent", "Renames agent title and tmux window:")
    };

    // 7 lines of content + 2 for borders = 9 lines
    let area = centered_rect_absolute(55, 9, frame.area());

    let text = vec![
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            description,
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("{}_", &app.input.buffer),
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to rename, Esc to cancel",
            Style::default().fg(colors::TEXT_MUTED),
        )),
    ];

    let block_title = if is_root {
        " Rename Agent (+ Branch) "
    } else {
        " Rename Agent "
    };

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(block_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
