//! Main layout rendering: agent list, content pane, status bar, tabs

use crate::agent::{Status, WorkspaceKind};
use crate::app::{App, DiffLineMeta, PreviewSelectionPoint, Tab};
use crate::app::{SidebarItem, SidebarProject};
use crate::state::AppMode;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use super::colors;

fn agent_list_item<'a>(
    app: &App,
    idx: usize,
    info: &crate::agent::VisibleAgentInfo<'a>,
) -> ListItem<'a> {
    let (status_symbol, status_color) = match info.agent.status {
        Status::Starting => (info.agent.status.symbol(), colors::STATUS_STARTING),
        Status::Running => {
            if app.data.ui.agent_is_waiting_for_input(info.agent.id) {
                if app.data.ui.agent_has_unseen_waiting_output(info.agent.id) {
                    ("◐", colors::STATUS_STARTING)
                } else {
                    ("○", colors::STATUS_WAITING)
                }
            } else {
                (info.agent.status.symbol(), colors::STATUS_RUNNING)
            }
        }
    };

    let style = if idx == app.data.selected {
        Style::default()
            .fg(colors::TEXT_PRIMARY)
            .bg(colors::SURFACE_HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::TEXT_PRIMARY)
    };

    let indent = "    ".repeat(info.depth);
    let collapse_indicator = if info.has_children {
        if info.agent.collapsed { "▶ " } else { "▼ " }
    } else {
        ""
    };

    let count_indicator = if info.child_count > 0 {
        format!(" ({})", info.child_count)
    } else {
        String::new()
    };

    let mut spans = Vec::new();
    spans.push(Span::raw(indent));
    spans.push(Span::styled(
        format!("{status_symbol} "),
        Style::default().fg(status_color),
    ));
    spans.push(Span::styled(
        collapse_indicator,
        Style::default().fg(colors::TEXT_DIM),
    ));
    spans.push(Span::styled(&info.agent.title, style));
    if info.agent.workspace_kind == WorkspaceKind::PlainDir {
        spans.push(Span::styled(
            " (no-git)",
            Style::default().fg(colors::TEXT_MUTED),
        ));
    }
    spans.push(Span::styled(
        count_indicator,
        Style::default().fg(colors::TEXT_DIM),
    ));
    spans.push(Span::styled(
        format!(" ({})", info.agent.age_string()),
        Style::default().fg(colors::TEXT_MUTED),
    ));

    ListItem::new(Line::from(spans)).style(style)
}

fn project_list_item<'a>(app: &App, idx: usize, project: &'a SidebarProject) -> ListItem<'a> {
    let style = if idx == app.data.selected {
        Style::default()
            .fg(colors::TEXT_PRIMARY)
            .bg(colors::SURFACE_HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(colors::TEXT_DIM)
            .bg(colors::SURFACE)
            .add_modifier(Modifier::BOLD)
    };

    let collapse_indicator = if project.collapsed { "▶ " } else { "▼ " };
    let count = format!(" ({})", project.agent_count);

    ListItem::new(Line::from(vec![
        Span::styled(collapse_indicator, Style::default().fg(colors::TEXT_DIM)),
        Span::styled(&project.label, style),
        Span::styled(count, Style::default().fg(colors::TEXT_DIM)),
    ]))
    .style(style)
}

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
    let visible = app.data.sidebar_items();
    let total_items = visible.len();
    let visible_height = usize::from(area.height.saturating_sub(2));
    let max_scroll = total_items.saturating_sub(visible_height);
    let scroll = app.data.ui.agent_list_scroll.min(max_scroll);

    let items: Vec<ListItem<'_>> = visible
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            SidebarItem::Project(project) => project_list_item(app, i, project),
            SidebarItem::Agent(agent) => agent_list_item(app, i, &agent.info),
        })
        .collect();

    let title = format!(" Agents ({}) ", app.data.storage.len());

    // Highlight agents list border only when it has focus. When a modal is open,
    // the modal should be the highlighted element instead.
    let border_color = if matches!(&app.mode, AppMode::Normal(_)) {
        colors::SELECTED
    } else {
        colors::BORDER
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .border_type(colors::BORDER_TYPE)
                .style(Style::default().bg(colors::SURFACE)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = ListState::default().with_offset(scroll);
    frame.render_stateful_widget(list, area, &mut state);

    if total_items > visible_height && area.width != 0 {
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

/// Render the content pane (tabs + preview/diff)
pub fn render_content_pane(frame: &mut Frame<'_>, app: &App, area: Rect) {
    match app.data.active_tab {
        Tab::Preview => render_preview(frame, app, area),
        Tab::Diff => render_diff(frame, app, area),
        Tab::Commits => render_commits(frame, app, area),
    }
}

fn tab_bar_tab_has_unseen_changes(app: &App, tab: Tab) -> bool {
    let Some(agent) = app.selected_agent() else {
        return false;
    };

    match tab {
        Tab::Preview => false,
        Tab::Diff => {
            if app.data.active_tab == Tab::Diff {
                return false;
            }

            let hash = app.data.ui.diff_hash;
            hash != 0 && hash != app.data.ui.diff_last_seen_hash_for_agent(agent.id)
        }
        Tab::Commits => {
            if app.data.active_tab == Tab::Commits {
                return false;
            }

            let hash = app.data.ui.commits_hash;
            hash != 0 && hash != app.data.ui.commits_last_seen_hash_for_agent(agent.id)
        }
    }
}

fn tab_bar_tab_width(label: &str, has_unseen_changes: bool) -> u16 {
    let label_width = u16::try_from(label.chars().count()).unwrap_or(0);
    let decoration_width = if has_unseen_changes { 4 } else { 2 };
    label_width.saturating_add(decoration_width)
}

/// Returns the tab corresponding to a horizontal offset within the content pane tab bar.
#[must_use]
pub fn tab_for_tab_bar_offset(app: &App, offset_x: u16) -> Option<Tab> {
    let preview_w = tab_bar_tab_width("Preview", false);
    let diff_w = tab_bar_tab_width("Diff", tab_bar_tab_has_unseen_changes(app, Tab::Diff));
    let commits_w = tab_bar_tab_width("Commits", tab_bar_tab_has_unseen_changes(app, Tab::Commits));

    let diff_start = preview_w;
    let commits_start = diff_start.saturating_add(diff_w);
    let commits_end = commits_start.saturating_add(commits_w);

    if offset_x < diff_start {
        return Some(Tab::Preview);
    }

    if offset_x < commits_start {
        return Some(Tab::Diff);
    }

    if offset_x < commits_end {
        return Some(Tab::Commits);
    }

    None
}

fn tab_bar_line(app: &App) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    let mut push_tab = |label: &'static str, active: bool, has_unseen_changes: bool| {
        let bg = if active {
            colors::SURFACE_HIGHLIGHT
        } else {
            colors::SURFACE
        };

        let tab_style = if active {
            Style::default()
                .fg(colors::SELECTED)
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::TEXT_MUTED).bg(bg)
        };

        if has_unseen_changes {
            spans.push(Span::styled(" ", tab_style));
            spans.push(Span::styled(
                "◐ ",
                Style::default().fg(colors::STATUS_STARTING).bg(bg),
            ));
            spans.push(Span::styled(format!("{label} "), tab_style));
        } else {
            spans.push(Span::styled(format!(" {label} "), tab_style));
        }
    };

    push_tab("Preview", app.data.active_tab == Tab::Preview, false);

    let diff_active = app.data.active_tab == Tab::Diff;
    let diff_has_unseen_changes = tab_bar_tab_has_unseen_changes(app, Tab::Diff);

    push_tab("Diff", diff_active, diff_has_unseen_changes);

    let commits_active = app.data.active_tab == Tab::Commits;
    let commits_has_unseen_changes = tab_bar_tab_has_unseen_changes(app, Tab::Commits);

    push_tab("Commits", commits_active, commits_has_unseen_changes);

    Line::from(spans)
}

fn render_tab_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let paragraph = Paragraph::new(tab_bar_line(app)).style(Style::default().bg(colors::SURFACE));
    frame.render_widget(paragraph, area);
}

/// Render the preview pane
pub fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let is_focused = matches!(&app.mode, AppMode::PreviewFocused(_));
    let no_agent_selected = app.selected_agent().is_none();

    let full_text = &app.data.ui.preview_text;
    let line_count = full_text.lines.len();

    // Use highlighted border when focused, show exit hint in title.
    let title = if is_focused {
        " Terminal Output (ATTACHED) [Ctrl+q detach] "
    } else {
        " Terminal Output (read-only) "
    };

    let border_color = if is_focused || matches!(&app.mode, AppMode::Scrolling(_)) {
        colors::SELECTED
    } else {
        colors::BORDER
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(colors::BORDER_TYPE)
        .style(Style::default().bg(colors::SURFACE));
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    render_tab_bar(frame, app, chunks[0]);

    let content_area = chunks[1];
    let visible_height = usize::from(content_area.height);
    let max_scroll = line_count.saturating_sub(visible_height);
    let scroll = app.data.ui.preview_scroll.min(max_scroll);
    let start = scroll.min(line_count);
    let end = start.saturating_add(visible_height).min(line_count);
    let mut visible_text = Text {
        alignment: full_text.alignment,
        style: full_text.style,
        lines: full_text.lines[start..end].to_vec(),
    };

    apply_preview_selection(app, start, &mut visible_text);

    let paragraph_style = if no_agent_selected {
        Style::default().fg(colors::TEXT_MUTED).bg(colors::SURFACE)
    } else {
        Style::default().bg(colors::SURFACE)
    };
    let scroll_x = u16::try_from(app.data.ui.preview_scroll_x).unwrap_or(u16::MAX);
    let paragraph = Paragraph::new(visible_text)
        .style(paragraph_style)
        .scroll((0, scroll_x));
    frame.render_widget(paragraph, content_area);

    if is_focused {
        render_preview_cursor(
            frame,
            app,
            content_area,
            scroll,
            scroll_x,
            line_count,
            visible_height,
        );
    }

    // Match common terminal UX (e.g. Claude Code): don't show a scrollbar while we're
    // auto-following the bottom. Show it only once the user scrolls up (paused).
    if !app.data.ui.preview_follow && line_count > visible_height && area.width != 0 {
        let scrollbar_area = Rect {
            x: area.x,
            y: content_area.y,
            width: area.width,
            height: content_area.height,
        };

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

fn apply_preview_selection(app: &App, start_line: usize, visible_text: &mut Text<'static>) {
    if !app.data.ui.preview_selection_dragging {
        return;
    }

    let Some(anchor) = app.data.ui.preview_selection_anchor else {
        return;
    };

    let cursor = app.data.ui.preview_selection_cursor;
    let (selection_start, selection_end) = normalize_preview_selection_points(anchor, cursor);

    for (row, line) in visible_text.lines.iter_mut().enumerate() {
        let line_idx = start_line.saturating_add(row);
        if line_idx < selection_start.line || line_idx > selection_end.line {
            continue;
        }

        apply_preview_selection_to_line(line_idx, selection_start, selection_end, line);
    }
}

const fn normalize_preview_selection_points(
    anchor: PreviewSelectionPoint,
    cursor: PreviewSelectionPoint,
) -> (PreviewSelectionPoint, PreviewSelectionPoint) {
    if anchor.line < cursor.line || (anchor.line == cursor.line && anchor.column <= cursor.column) {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    }
}

fn apply_preview_selection_to_line(
    line_idx: usize,
    selection_start: PreviewSelectionPoint,
    selection_end: PreviewSelectionPoint,
    line: &mut Line<'static>,
) {
    let line_len = line
        .spans
        .iter()
        .map(|span| span.content.as_ref().chars().count())
        .sum::<usize>();
    if line_len == 0 {
        return;
    }

    let (raw_start, raw_end) = if selection_start.line == selection_end.line {
        (selection_start.column, selection_end.column)
    } else if line_idx == selection_start.line {
        (selection_start.column, usize::MAX)
    } else if line_idx == selection_end.line {
        (0usize, selection_end.column)
    } else {
        (0usize, usize::MAX)
    };

    let selection_start_col = raw_start.min(line_len);
    let selection_end_col = raw_end.min(line_len.saturating_sub(1));
    if selection_start_col > selection_end_col {
        return;
    }

    if selection_start_col == 0 && selection_end_col == line_len.saturating_sub(1) {
        for span in &mut line.spans {
            span.style = span.style.bg(colors::DIFF_SELECTION_BG);
        }
        return;
    }

    let original_spans = std::mem::take(&mut line.spans);
    let mut new_spans = Vec::new();
    let mut col = 0usize;

    for span in original_spans {
        let text = span.content.as_ref();
        let span_len = text.chars().count();
        if span_len == 0 {
            new_spans.push(span);
            continue;
        }

        let span_start = col;
        let span_end = span_start.saturating_add(span_len.saturating_sub(1));
        col = col.saturating_add(span_len);

        if span_end < selection_start_col || span_start > selection_end_col {
            new_spans.push(span);
            continue;
        }

        if selection_start_col <= span_start && span_end <= selection_end_col {
            let mut selected_span = span;
            selected_span.style = selected_span.style.bg(colors::DIFF_SELECTION_BG);
            new_spans.push(selected_span);
            continue;
        }

        let overlap_start = selection_start_col.saturating_sub(span_start).min(span_len);
        let overlap_end = selection_end_col
            .saturating_sub(span_start)
            .min(span_len.saturating_sub(1));

        let start_byte = byte_index_for_char(text, overlap_start);
        let end_byte = byte_index_for_char(text, overlap_end.saturating_add(1));

        if overlap_start > 0 && start_byte != 0 {
            new_spans.push(Span::styled(text[..start_byte].to_string(), span.style));
        }

        if start_byte < end_byte {
            new_spans.push(Span::styled(
                text[start_byte..end_byte].to_string(),
                span.style.bg(colors::DIFF_SELECTION_BG),
            ));
        }

        if end_byte < text.len() {
            new_spans.push(Span::styled(text[end_byte..].to_string(), span.style));
        }
    }

    line.spans = new_spans;
}

fn byte_index_for_char(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }

    match text.char_indices().nth(char_idx) {
        Some((byte_idx, _)) => byte_idx,
        None => text.len(),
    }
}

fn render_preview_cursor(
    frame: &mut Frame<'_>,
    app: &App,
    content_area: Rect,
    scroll: usize,
    scroll_x: u16,
    line_count: usize,
    visible_height: usize,
) {
    let Some((cursor_x, cursor_y, cursor_hidden)) = app.data.ui.preview_cursor_position else {
        return;
    };
    if cursor_hidden {
        return;
    }
    let Some((_cols, pane_rows)) = app.data.ui.preview_pane_size else {
        return;
    };

    let pane_rows = usize::from(pane_rows);
    if pane_rows == 0 || visible_height == 0 {
        return;
    }

    let cursor_row = usize::from(cursor_y);
    let cursor_line_index = if line_count >= pane_rows {
        line_count
            .saturating_sub(pane_rows)
            .saturating_add(cursor_row)
    } else {
        cursor_row
    };

    let visible_row = cursor_line_index.saturating_sub(scroll);
    if visible_row >= visible_height {
        return;
    }

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let max_x = content_area.width.saturating_sub(1);
    let cursor_x = if cursor_x < scroll_x {
        0
    } else {
        cursor_x.saturating_sub(scroll_x).min(max_x)
    };
    let cursor_y = u16::try_from(visible_row)
        .unwrap_or(0)
        .min(content_area.height.saturating_sub(1));

    frame.set_cursor_position((
        content_area.x.saturating_add(cursor_x),
        content_area.y.saturating_add(cursor_y),
    ));
}

fn diff_selection_range(app: &App) -> Option<(usize, usize)> {
    let anchor = app.data.ui.diff_visual_anchor?;
    let cursor = app.data.ui.diff_cursor;
    Some((anchor.min(cursor), anchor.max(cursor)))
}

/// Render the diff pane
pub fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.data.ui.diff_content;
    let is_focused = matches!(&app.mode, AppMode::DiffFocused(_));
    let no_agent_selected = app.selected_agent().is_none();
    let title = if is_focused {
        " Git Diff (INTERACTIVE) [Ctrl+q exit] "
    } else {
        " Git Diff "
    };
    let border_color = if is_focused || matches!(&app.mode, AppMode::Scrolling(_)) {
        colors::SELECTED
    } else {
        colors::BORDER
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(colors::BORDER_TYPE)
        .style(Style::default().bg(colors::SURFACE));
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    render_tab_bar(frame, app, chunks[0]);

    let content_area = chunks[1];
    let visible_height = usize::from(content_area.height);
    let total_lines = app.data.ui.diff_line_ranges.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.data.ui.diff_scroll.min(max_scroll);
    let end_line = (scroll + visible_height).min(total_lines);

    let selection_range = if is_focused {
        diff_selection_range(app)
    } else {
        None
    };

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(end_line.saturating_sub(scroll));
    for (offset, &(start, end)) in app.data.ui.diff_line_ranges[scroll..end_line]
        .iter()
        .enumerate()
    {
        let line_idx = scroll.saturating_add(offset);
        let line = &content[start..end];

        if no_agent_selected {
            lines.push(Line::styled(line, Style::default().fg(colors::TEXT_MUTED)));
            continue;
        }

        let meta = app
            .data
            .ui
            .diff_line_meta
            .get(line_idx)
            .unwrap_or(&DiffLineMeta::Unknown);

        let trimmed = line.trim_start();
        let mut style = match meta {
            DiffLineMeta::Info => Style::default().fg(colors::TEXT_MUTED),
            DiffLineMeta::File { .. } => Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
            DiffLineMeta::Hunk { .. } => Style::default().fg(colors::DIFF_HUNK),
            DiffLineMeta::Line { .. } => {
                if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
                    Style::default().fg(colors::DIFF_ADD)
                } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
                    Style::default().fg(colors::DIFF_REMOVE)
                } else if trimmed.starts_with("@@") {
                    Style::default().fg(colors::DIFF_HUNK)
                } else {
                    Style::default().fg(colors::TEXT_PRIMARY)
                }
            }
            DiffLineMeta::Unknown => Style::default().fg(colors::TEXT_PRIMARY),
        };

        if let Some((sel_start, sel_end)) = selection_range
            && line_idx >= sel_start
            && line_idx <= sel_end
        {
            style = style.bg(colors::DIFF_SELECTION_BG);
        }

        if line_idx == app.data.ui.diff_cursor && is_focused {
            style = style.bg(colors::DIFF_CURSOR_BG);
        }

        lines.push(Line::styled(line, style));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(colors::SURFACE));
    frame.render_widget(paragraph, content_area);

    render_diff_scrollbar(
        frame,
        area,
        content_area,
        total_lines,
        visible_height,
        max_scroll,
        scroll,
    );
}

/// Render the commits pane
pub fn render_commits(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.data.ui.commits_content;

    let border_color = if matches!(&app.mode, AppMode::Scrolling(_)) {
        colors::SELECTED
    } else {
        colors::BORDER
    };

    let block = Block::default()
        .title(" Git Commits ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(colors::BORDER_TYPE)
        .style(Style::default().bg(colors::SURFACE));
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    render_tab_bar(frame, app, chunks[0]);

    let content_area = chunks[1];
    let visible_height = usize::from(content_area.height);
    let total_lines = app.data.ui.commits_line_ranges.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.data.ui.commits_scroll.min(max_scroll);
    let end_line = (scroll + visible_height).min(total_lines);

    let mut lines: Vec<Line<'_>> = Vec::with_capacity(end_line.saturating_sub(scroll));
    for (offset, &(start, end)) in app.data.ui.commits_line_ranges[scroll..end_line]
        .iter()
        .enumerate()
    {
        let line_idx = scroll.saturating_add(offset);
        let line = &content[start..end];

        let trimmed = line.trim_start();
        if line_idx <= 1
            || trimmed.starts_with("Branch:")
            || trimmed.starts_with("Commits:")
            || trimmed == "(No commits)"
        {
            lines.push(Line::styled(line, Style::default().fg(colors::TEXT_MUTED)));
            continue;
        }

        if line.starts_with("    ") {
            lines.push(Line::styled(line, Style::default().fg(colors::TEXT_DIM)));
            continue;
        }

        if line.starts_with("  ") {
            lines.push(Line::styled(line, Style::default().fg(colors::TEXT_MUTED)));
            continue;
        }

        let subject_line = if let Some((hash, subject)) = line.split_once("  ")
            && hash.len() >= 7
            && hash.len() <= 12
            && hash.chars().all(|c| c.is_ascii_hexdigit())
        {
            Line::from(vec![
                Span::styled(
                    hash,
                    Style::default()
                        .fg(colors::DIFF_HUNK)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(subject, Style::default().fg(colors::TEXT_PRIMARY)),
            ])
        } else {
            Line::styled(line, Style::default().fg(colors::TEXT_PRIMARY))
        };
        lines.push(subject_line);
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .style(Style::default().bg(colors::SURFACE));
    frame.render_widget(paragraph, content_area);

    render_commits_scrollbar(
        frame,
        area,
        content_area,
        total_lines,
        visible_height,
        max_scroll,
        scroll,
    );
}

fn render_commits_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    content_area: Rect,
    total_lines: usize,
    visible_height: usize,
    max_scroll: usize,
    scroll: usize,
) {
    if total_lines <= visible_height || area.width == 0 {
        return;
    }

    let scrollbar_area = Rect {
        x: area.x,
        y: content_area.y,
        width: area.width,
        height: content_area.height,
    };

    if scrollbar_area.width == 0 || scrollbar_area.height == 0 {
        return;
    }

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

fn render_diff_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    content_area: Rect,
    total_lines: usize,
    visible_height: usize,
    max_scroll: usize,
    scroll: usize,
) {
    if total_lines <= visible_height || area.width == 0 {
        return;
    }

    let scrollbar_area = Rect {
        x: area.x,
        y: content_area.y,
        width: area.width,
        height: content_area.height,
    };

    if scrollbar_area.width == 0 || scrollbar_area.height == 0 {
        return;
    }

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

/// Render the status bar
pub fn render_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Don't show error in status bar when error modal is displayed
    let showing_error_modal = matches!(&app.mode, AppMode::ErrorModal(_));

    let left_content = match (
        &app.data.ui.last_error,
        &app.data.ui.status_message,
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
            let hints = crate::config::status_hints();
            Span::styled(
                format!(" {running} running | {hints} "),
                Style::default().fg(colors::TEXT_DIM),
            )
        }
    };

    let key_routing = if matches!(&app.mode, AppMode::PreviewFocused(_)) {
        "Keys → Agent (Ctrl+q detach)"
    } else {
        "Keys → Tenex"
    };
    let key_routing_style = if matches!(&app.mode, AppMode::PreviewFocused(_)) {
        Style::default()
            .fg(colors::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::TEXT_DIM)
    };
    let key_routing_span = Span::styled(format!(" {key_routing} "), key_routing_style);

    let key_routing_width = u16::try_from(key_routing.chars().count().saturating_add(2))
        .unwrap_or(0)
        .min(area.width);
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(key_routing_width)])
        .split(area);

    let left = Paragraph::new(Line::from(left_content)).style(Style::default().bg(colors::SURFACE));
    frame.render_widget(left, chunks[0]);

    let right = Paragraph::new(Line::from(key_routing_span))
        .style(Style::default().bg(colors::SURFACE))
        .alignment(Alignment::Right);
    frame.render_widget(right, chunks[1]);
}

/// Calculate the inner dimensions of the preview pane (content area without borders)
///
/// This is used to resize mux windows to match the preview pane size.
#[must_use]
pub fn calculate_preview_dimensions(frame_area: Rect) -> (u16, u16) {
    // Main layout: Vertical split with status bar at bottom (1 line)
    let main_area_height = frame_area.height.saturating_sub(1);

    // Horizontal split: 30% agents, 70% content
    let content_width = u16::try_from((u32::from(frame_area.width) * 70) / 100).unwrap_or(0);

    // Content pane: preview/diff pane with 1-line tab bar inside
    let preview_height = main_area_height;

    // Inner area: subtract borders + 1-line tab bar (2 chars total width, 3 lines total height)
    let inner_width = content_width.saturating_sub(2);
    let inner_height = preview_height.saturating_sub(3);

    (inner_width, inner_height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::{DiffFocusedMode, ScrollingMode};
    use ratatui::{Terminal, backend::TestBackend};
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<Vec<_>>()
            .join("")
    }

    fn cell_at(
        buffer: &ratatui::buffer::Buffer,
        x: u16,
        y: u16,
    ) -> anyhow::Result<&ratatui::buffer::Cell> {
        buffer
            .cell((x, y))
            .ok_or_else(|| anyhow::anyhow!("Missing cell at ({x}, {y})"))
    }

    #[test]
    fn test_normalize_preview_selection_points_orders_by_line_then_column() {
        let anchor = PreviewSelectionPoint { line: 2, column: 0 };
        let cursor = PreviewSelectionPoint {
            line: 1,
            column: 99,
        };
        let (start, end) = normalize_preview_selection_points(anchor, cursor);
        assert_eq!(start, cursor);
        assert_eq!(end, anchor);

        let anchor = PreviewSelectionPoint {
            line: 1,
            column: 10,
        };
        let cursor = PreviewSelectionPoint { line: 1, column: 9 };
        let (start, end) = normalize_preview_selection_points(anchor, cursor);
        assert_eq!(start, cursor);
        assert_eq!(end, anchor);
    }

    #[test]
    fn test_apply_preview_selection_to_line_highlights_entire_line_when_fully_selected() {
        let mut line: Line<'static> = Line::from(vec![
            Span::styled("hello ", Style::default().fg(colors::TEXT_PRIMARY)),
            Span::styled("world", Style::default().fg(colors::TEXT_MUTED)),
        ]);

        let start = PreviewSelectionPoint { line: 0, column: 0 };
        let end = PreviewSelectionPoint {
            line: 0,
            column: usize::MAX,
        };
        apply_preview_selection_to_line(0, start, end, &mut line);

        assert_eq!(line_text(&line), "hello world");
        for span in &line.spans {
            assert_eq!(span.style.bg, Some(colors::DIFF_SELECTION_BG));
        }
    }

    #[test]
    fn test_apply_preview_selection_to_line_splits_spans_for_partial_selection() {
        let mut line: Line<'static> = Line::from(vec![
            Span::styled("hello ", Style::default().fg(colors::TEXT_PRIMARY)),
            Span::styled("world", Style::default().fg(colors::TEXT_PRIMARY)),
        ]);

        let start = PreviewSelectionPoint { line: 0, column: 6 };
        let end = PreviewSelectionPoint { line: 0, column: 8 };
        apply_preview_selection_to_line(0, start, end, &mut line);

        assert_eq!(line_text(&line), "hello world");
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content.as_ref(), "hello ");
        assert_eq!(line.spans[0].style.bg, None);
        assert_eq!(line.spans[1].content.as_ref(), "wor");
        assert_eq!(line.spans[1].style.bg, Some(colors::DIFF_SELECTION_BG));
        assert_eq!(line.spans[2].content.as_ref(), "ld");
        assert_eq!(line.spans[2].style.bg, None);
    }

    #[test]
    fn test_apply_preview_selection_to_line_highlights_entire_span_when_selected() {
        let mut line: Line<'static> = Line::from(vec![
            Span::styled("foo", Style::default().fg(colors::TEXT_PRIMARY)),
            Span::styled("bar", Style::default().fg(colors::TEXT_PRIMARY)),
        ]);

        let start = PreviewSelectionPoint { line: 0, column: 0 };
        let end = PreviewSelectionPoint { line: 0, column: 2 };
        apply_preview_selection_to_line(0, start, end, &mut line);

        assert_eq!(line_text(&line), "foobar");
        assert_eq!(line.spans[0].content.as_ref(), "foo");
        assert_eq!(line.spans[0].style.bg, Some(colors::DIFF_SELECTION_BG));
        assert_eq!(line.spans[1].content.as_ref(), "bar");
        assert_eq!(line.spans[1].style.bg, None);
    }

    #[test]
    fn test_apply_preview_selection_to_line_returns_early_when_selection_starts_at_eol() {
        let mut line: Line<'static> = Line::from(vec![Span::styled(
            "abc",
            Style::default().fg(colors::TEXT_PRIMARY),
        )]);

        let start = PreviewSelectionPoint { line: 0, column: 3 };
        let end = PreviewSelectionPoint {
            line: 0,
            column: usize::MAX,
        };
        apply_preview_selection_to_line(0, start, end, &mut line);

        assert_eq!(line_text(&line), "abc");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].style.bg, None);
    }

    #[test]
    fn test_apply_preview_selection_to_line_partial_selection_to_end_of_span_uses_full_end_byte() {
        let mut line: Line<'static> = Line::from(vec![Span::styled(
            "world",
            Style::default().fg(colors::TEXT_PRIMARY),
        )]);

        let start = PreviewSelectionPoint { line: 0, column: 2 };
        let end = PreviewSelectionPoint { line: 0, column: 4 };
        apply_preview_selection_to_line(0, start, end, &mut line);

        assert_eq!(line_text(&line), "world");
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.as_ref(), "wo");
        assert_eq!(line.spans[0].style.bg, None);
        assert_eq!(line.spans[1].content.as_ref(), "rld");
        assert_eq!(line.spans[1].style.bg, Some(colors::DIFF_SELECTION_BG));
    }

    #[test]
    fn test_tab_bar_renders_unseen_diff_dot() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.selected = 1;
        app.data.active_tab = Tab::Preview;

        app.data.ui.diff_hash = 123;
        app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 0);

        let line = tab_bar_line(&app);
        assert!(line_text(&line).contains("◐ Diff"));
        Ok(())
    }

    #[test]
    fn test_tab_bar_hides_unseen_diff_dot_when_viewing_diff_tab() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.selected = 1;
        app.data.active_tab = Tab::Diff;

        app.data.ui.diff_hash = 123;
        app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 0);

        let line = tab_bar_line(&app);
        assert!(!line_text(&line).contains('◐'));
        Ok(())
    }

    #[test]
    fn test_tab_bar_hides_unseen_diff_dot_when_hash_seen() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.selected = 1;
        app.data.active_tab = Tab::Preview;

        app.data.ui.diff_hash = 123;
        app.data.ui.set_diff_last_seen_hash_for_agent(agent_id, 123);

        let line = tab_bar_line(&app);
        assert!(!line_text(&line).contains('◐'));
        Ok(())
    }

    #[test]
    fn test_no_agent_selected_placeholder_has_consistent_color_across_tabs() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        app.data.ui.set_preview_content("(No agent selected)");
        app.data.ui.set_diff_content("(No agent selected)");
        app.data.ui.set_commits_content("(No agent selected)");

        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| {
            app.data.active_tab = Tab::Preview;
            render_preview(frame, &app, frame.area());
        })?;
        let preview_fg = {
            let buffer = terminal.backend().buffer();
            let cell = cell_at(buffer, 1, 2)?;
            assert_eq!(cell.symbol(), "(");
            cell.fg
        };
        assert_eq!(preview_fg, colors::TEXT_MUTED);

        terminal.draw(|frame| {
            app.data.active_tab = Tab::Diff;
            render_diff(frame, &app, frame.area());
        })?;
        let diff_fg = {
            let buffer = terminal.backend().buffer();
            let cell = cell_at(buffer, 1, 2)?;
            assert_eq!(cell.symbol(), "(");
            cell.fg
        };
        assert_eq!(diff_fg, colors::TEXT_MUTED);

        terminal.draw(|frame| {
            app.data.active_tab = Tab::Commits;
            render_commits(frame, &app, frame.area());
        })?;
        let commits_fg = {
            let buffer = terminal.backend().buffer();
            let cell = cell_at(buffer, 1, 2)?;
            assert_eq!(cell.symbol(), "(");
            cell.fg
        };
        assert_eq!(commits_fg, colors::TEXT_MUTED);

        Ok(())
    }

    #[test]
    fn test_tab_bar_renders_unseen_commits_dot() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.selected = 1;
        app.data.active_tab = Tab::Preview;

        app.data.ui.diff_hash = 0;
        app.data.ui.commits_hash = 123;
        app.data
            .ui
            .set_commits_last_seen_hash_for_agent(agent_id, 0);

        let line = tab_bar_line(&app);
        assert!(line_text(&line).contains("◐ Commits"));
        Ok(())
    }

    #[test]
    fn test_tab_bar_hides_unseen_commits_dot_when_viewing_commits_tab() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.selected = 1;
        app.data.active_tab = Tab::Commits;

        app.data.ui.diff_hash = 0;
        app.data.ui.commits_hash = 123;
        app.data
            .ui
            .set_commits_last_seen_hash_for_agent(agent_id, 0);

        let line = tab_bar_line(&app);
        assert!(!line_text(&line).contains('◐'));
        Ok(())
    }

    #[test]
    fn test_tab_bar_hides_unseen_commits_dot_when_hash_seen() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.selected = 1;
        app.data.active_tab = Tab::Preview;

        app.data.ui.diff_hash = 0;
        app.data.ui.commits_hash = 123;
        app.data
            .ui
            .set_commits_last_seen_hash_for_agent(agent_id, 123);

        let line = tab_bar_line(&app);
        assert!(!line_text(&line).contains('◐'));
        Ok(())
    }

    #[test]
    fn test_tab_for_tab_bar_offset_selects_commits_and_none_after_end() -> anyhow::Result<()> {
        let (app, _temp) = create_test_app()?;

        let preview_w = tab_bar_tab_width("Preview", false);
        let diff_w = tab_bar_tab_width("Diff", false);
        let commits_w = tab_bar_tab_width("Commits", false);

        let diff_start = preview_w;
        let commits_start = diff_start.saturating_add(diff_w);
        let commits_end = commits_start.saturating_add(commits_w);

        assert_eq!(
            tab_for_tab_bar_offset(&app, commits_start),
            Some(Tab::Commits)
        );
        assert_eq!(tab_for_tab_bar_offset(&app, commits_end), None);
        Ok(())
    }

    #[test]
    fn test_tab_bar_tab_has_unseen_changes_preview_is_false() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        app.data.storage.add(agent);
        app.data.selected = 1;

        assert!(!tab_bar_tab_has_unseen_changes(&app, Tab::Preview));
        Ok(())
    }

    #[test]
    fn test_render_content_pane_renders_commits() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.data.active_tab = Tab::Commits;
        app.data
            .ui
            .set_commits_content("Branch: main\nCommits: main..HEAD (0 shown)");

        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_content_pane(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Git Commits"));
        Ok(())
    }

    #[test]
    fn test_render_preview_cursor_returns_on_invalid_state() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 0, 10, 10);
        })?;

        app.data.ui.preview_cursor_position = Some((0, 0, true));
        app.data.ui.preview_pane_size = Some((40, 10));
        terminal.draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 0, 10, 10);
        })?;

        app.data.ui.preview_cursor_position = Some((0, 0, false));
        app.data.ui.preview_pane_size = None;
        terminal.draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 0, 10, 10);
        })?;

        app.data.ui.preview_cursor_position = Some((0, 0, false));
        app.data.ui.preview_pane_size = Some((40, 10));
        terminal.draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 0, 10, 0);
        })?;

        app.data.ui.preview_cursor_position = Some((0, 0, false));
        app.data.ui.preview_pane_size = Some((40, 5));
        terminal.draw(|frame| {
            render_preview_cursor(frame, &app, frame.area(), 0, 0, 10, 1);
        })?;

        app.data.ui.preview_cursor_position = Some((0, 0, false));
        app.data.ui.preview_pane_size = Some((40, 5));
        terminal.draw(|frame| {
            let mut area = frame.area();
            area.width = 0;
            render_preview_cursor(frame, &app, area, 0, 0, 5, 5);
        })?;

        Ok(())
    }

    #[test]
    fn test_render_diff_focused_applies_styles_and_selection() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        app.data.storage.add(agent);
        app.data.selected = 1;

        app.enter_mode(DiffFocusedMode.into());
        app.data.active_tab = Tab::Diff;
        let model = crate::git::DiffModel {
            files: vec![crate::git::DiffFile {
                path: std::path::PathBuf::from("file.txt"),
                status: crate::git::FileStatus::Modified,
                meta: Vec::new(),
                hunks: vec![crate::git::DiffHunk {
                    header: "@@ -1,1 +1,2 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 2,
                    lines: vec![
                        crate::git::DiffHunkLine {
                            origin: '+',
                            content: "added".to_string(),
                            old_lineno: None,
                            new_lineno: Some(1),
                        },
                        crate::git::DiffHunkLine {
                            origin: '-',
                            content: "removed".to_string(),
                            old_lineno: Some(1),
                            new_lineno: None,
                        },
                        crate::git::DiffHunkLine {
                            origin: ' ',
                            content: "@@ inline".to_string(),
                            old_lineno: Some(2),
                            new_lineno: Some(2),
                        },
                        crate::git::DiffHunkLine {
                            origin: ' ',
                            content: "context".to_string(),
                            old_lineno: Some(3),
                            new_lineno: Some(3),
                        },
                    ],
                }],
                additions: 1,
                deletions: 1,
            }],
            summary: crate::git::DiffSummary {
                files_changed: 1,
                additions: 1,
                deletions: 1,
            },
            hash: 1,
        };

        let (content, meta) = app.data.ui.build_diff_view(&model);
        app.data.ui.set_diff_view(content, meta);
        app.data.ui.diff_visual_anchor = Some(3);
        app.data.ui.diff_cursor = 4;

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_diff(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("INTERACTIVE"));
        assert!(text.contains("+added"));
        assert!(text.contains("-removed"));
        Ok(())
    }

    #[test]
    fn test_render_commits_shows_selected_border_in_scrolling_mode() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.data.active_tab = Tab::Commits;
        app.data
            .ui
            .set_commits_content("Branch: main\nCommits: main..HEAD (0 shown)");

        app.enter_mode(ScrollingMode.into());

        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_commits(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Git Commits"));
        Ok(())
    }

    #[test]
    fn test_scrollbars_return_when_scrollbar_area_is_empty() -> anyhow::Result<()> {
        let (_app, _temp) = create_test_app()?;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| {
            let area = frame.area();
            let content_area = Rect { height: 0, ..area };

            render_commits_scrollbar(frame, area, content_area, 10, 1, 9, 0);
            render_diff_scrollbar(frame, area, content_area, 10, 1, 9, 0);
        })?;

        Ok(())
    }

    #[test]
    fn test_render_commits_renders_subject_meta_and_body() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.data.active_tab = Tab::Commits;
        app.data.ui.set_commits_content(
            [
                "Branch: tenex/test",
                "Commits: main..HEAD (1 shown)",
                "abcdef1  Add thing",
                "  2026-01-11 12:34 • Test Author • (HEAD -> tenex/test)",
                "    This is the body.",
                "zzzzzzz  Not hex (should not parse as hash)",
            ]
            .join("\n"),
        );

        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_commits(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("abcdef1"));
        assert!(text.contains("Add thing"));
        assert!(text.contains("Test Author"));
        assert!(text.contains("This is the body."));
        assert!(text.contains("Not hex"));
        Ok(())
    }

    #[test]
    fn test_render_commits_renders_scrollbar_when_overflowing() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.data.active_tab = Tab::Commits;

        let mut lines = Vec::new();
        lines.push("Branch: tenex/test".to_string());
        lines.push("Commits: main..HEAD (100 shown)".to_string());
        for idx in 0..100 {
            lines.push(format!("{idx:07x}  Commit {idx}"));
        }

        app.data.ui.set_commits_content(lines.join("\n"));

        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_commits(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains('░') || text.contains('█'));
        Ok(())
    }

    #[test]
    fn test_render_preview_hides_scrollbar_when_following() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.data.active_tab = Tab::Preview;

        app.data.ui.set_preview_content(
            (0..50)
                .map(|i| format!("Line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_follow = true;
        app.data.ui.preview_scroll = usize::MAX;

        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_preview(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(!text.contains('░') && !text.contains('█'));
        Ok(())
    }

    #[test]
    fn test_render_preview_shows_scrollbar_when_paused() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.data.active_tab = Tab::Preview;

        app.data.ui.set_preview_content(
            (0..50)
                .map(|i| format!("Line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.preview_follow = false;
        app.data.ui.preview_scroll = 0;

        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|frame| render_preview(frame, &app, frame.area()))?;

        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains('░') || text.contains('█'));
        Ok(())
    }
}
