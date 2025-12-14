//! Main layout rendering: agent list, content pane, status bar, tabs

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
};
use tenex::agent::Status;
use tenex::app::{App, Mode, Tab};

use super::colors;

/// Render the main area (agent list + content pane)
pub fn render_main(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    render_agent_list(frame, app, chunks[0]);
    render_content_pane(frame, app, chunks[1]);
}

/// Render the agent list panel
pub fn render_agent_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Use optimized method that pre-computes child info in O(n) instead of O(n²)
    let visible = app.storage.visible_agents_with_info();

    let items: Vec<ListItem<'_>> = visible
        .iter()
        .enumerate()
        .map(|(i, info)| {
            let status_color = match info.agent.status {
                Status::Starting => colors::STATUS_STARTING,
                Status::Running => colors::STATUS_RUNNING,
            };

            let style = if i == app.selected {
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .bg(colors::SURFACE_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::TEXT_PRIMARY)
            };

            // Build indentation based on depth
            let indent = "    ".repeat(info.depth);

            // Collapse/expand indicator (pre-computed)
            let collapse_indicator = if info.has_children {
                if info.agent.collapsed { "▶ " } else { "▼ " }
            } else {
                ""
            };

            // Child count indicator (pre-computed)
            let count_indicator = if info.child_count > 0 {
                format!(" ({})", info.child_count)
            } else {
                String::new()
            };

            let content = Line::from(vec![
                Span::raw(indent),
                Span::styled(
                    format!("{} ", info.agent.status.symbol()),
                    Style::default().fg(status_color),
                ),
                Span::styled(collapse_indicator, Style::default().fg(colors::TEXT_DIM)),
                Span::styled(&info.agent.title, style),
                Span::styled(count_indicator, Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!(" ({})", info.agent.age_string()),
                    Style::default().fg(colors::TEXT_MUTED),
                ),
            ]);

            ListItem::new(content).style(style)
        })
        .collect();

    let title = format!(" Agents ({}) ", app.storage.len());

    // Highlight agents list border when it has focus (not in PreviewFocused mode)
    let border_color = if app.mode == Mode::PreviewFocused {
        colors::BORDER
    } else {
        colors::SELECTED
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_widget(list, area);
}

/// Render the content pane (tabs + preview/diff)
pub fn render_content_pane(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    render_tab_bar(frame, app, chunks[0]);

    match app.active_tab {
        Tab::Preview => render_preview(frame, app, chunks[1]),
        Tab::Diff => render_diff(frame, app, chunks[1]),
    }
}

/// Render the tab bar
pub fn render_tab_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let tabs = vec![
        (" Preview ", app.active_tab == Tab::Preview),
        (" Diff ", app.active_tab == Tab::Diff),
    ];

    let spans: Vec<Span<'_>> = tabs
        .into_iter()
        .map(|(name, active)| {
            if active {
                Span::styled(
                    name,
                    Style::default()
                        .fg(colors::TEXT_PRIMARY)
                        .bg(colors::SURFACE_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(name, Style::default().fg(colors::TEXT_MUTED))
            }
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(colors::SURFACE));
    frame.render_widget(paragraph, area);
}

/// Render the preview pane
pub fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.ui.preview_content;
    let is_focused = app.mode == Mode::PreviewFocused;

    // Parse ANSI escape sequences to preserve terminal colors
    let text = ansi_to_tui::IntoText::into_text(content).unwrap_or_else(|_| {
        // Fallback to plain text if parsing fails
        Text::from(content.as_str())
    });

    let line_count = text.lines.len();
    let visible_height = usize::from(area.height.saturating_sub(2));
    let max_scroll = line_count.saturating_sub(visible_height);
    let scroll = app.ui.preview_scroll.min(max_scroll);
    let scroll_pos = u16::try_from(scroll).unwrap_or(u16::MAX);

    // Use highlighted border when focused, show exit hint in title
    let (border_color, title) = if is_focused {
        (colors::SELECTED, " Terminal Output [Ctrl+q to exit] ")
    } else {
        (colors::BORDER, " Terminal Output ")
    };

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .scroll((scroll_pos, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);

    if line_count > visible_height && area.width != 0 {
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

/// Render the diff pane
pub fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.ui.diff_content;

    let visible_height = usize::from(area.height.saturating_sub(2));
    let total_lines = app.ui.diff_line_ranges.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.ui.diff_scroll.min(max_scroll);
    let end_line = (scroll + visible_height).min(total_lines);

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(end_line.saturating_sub(scroll));
    for &(start, end) in &app.ui.diff_line_ranges[scroll..end_line] {
        let line = &content[start..end];
        let color = if line.starts_with('+') && !line.starts_with("+++") {
            colors::DIFF_ADD
        } else if line.starts_with('-') && !line.starts_with("---") {
            colors::DIFF_REMOVE
        } else if line.starts_with("@@") {
            colors::DIFF_HUNK
        } else {
            colors::TEXT_PRIMARY
        };

        lines.push(Line::styled(line, Style::default().fg(color)));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Git Diff ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);

    if total_lines > visible_height && area.width != 0 {
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

/// Render the status bar
pub fn render_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Don't show error in status bar when error modal is displayed
    let showing_error_modal = matches!(app.mode, Mode::ErrorModal(_));

    let content = match (
        &app.ui.last_error,
        &app.ui.status_message,
        showing_error_modal,
    ) {
        (Some(error), _, false) => Span::styled(
            format!(" Error: {error} "),
            Style::default()
                .fg(colors::DIFF_REMOVE)
                .add_modifier(Modifier::BOLD),
        ),
        (_, Some(status), _) => Span::styled(
            format!(" {status} "),
            Style::default().fg(colors::STATUS_RUNNING),
        ),
        _ => {
            let running = app.running_agent_count();
            let hints = tenex::config::status_hints();
            Span::styled(
                format!(" {running} running | {hints} "),
                Style::default().fg(colors::TEXT_DIM),
            )
        }
    };

    let paragraph = Paragraph::new(Line::from(content)).style(Style::default().bg(colors::SURFACE));
    frame.render_widget(paragraph, area);
}

/// Calculate the inner dimensions of the preview pane (content area without borders)
///
/// This is used to resize tmux windows to match the preview pane size.
#[must_use]
pub fn calculate_preview_dimensions(frame_area: Rect) -> (u16, u16) {
    // Main layout: Vertical split with status bar at bottom (1 line)
    let main_area_height = frame_area.height.saturating_sub(1);

    // Horizontal split: 30% agents, 70% content
    let content_width = u16::try_from((u32::from(frame_area.width) * 70) / 100).unwrap_or(0);

    // Content pane: 1-line tab bar, rest is preview
    let preview_height = main_area_height.saturating_sub(1);

    // Inner area: subtract borders (2 chars total width, 2 lines total height)
    let inner_width = content_width.saturating_sub(2);
    let inner_height = preview_height.saturating_sub(2);

    (inner_width, inner_height)
}
