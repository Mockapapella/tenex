//! TUI rendering

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use tenex::agent::Status;
use tenex::app::{App, Mode, Tab};

/// Modern color palette - cohesive, muted colors for a clean look
mod colors {
    use ratatui::style::Color;

    // UI Chrome
    pub const BORDER: Color = Color::Rgb(100, 110, 130);
    pub const SURFACE: Color = Color::Rgb(30, 32, 40);
    pub const SURFACE_HIGHLIGHT: Color = Color::Rgb(50, 55, 70);

    // Text
    pub const TEXT_PRIMARY: Color = Color::Rgb(220, 220, 230);
    pub const TEXT_DIM: Color = Color::Rgb(130, 135, 150);
    pub const TEXT_MUTED: Color = Color::Rgb(90, 95, 110);

    // Status (semantic)
    pub const STATUS_RUNNING: Color = Color::Rgb(120, 180, 120);
    pub const STATUS_STARTING: Color = Color::Rgb(200, 180, 100);

    // Diff
    pub const DIFF_ADD: Color = Color::Rgb(120, 180, 120);
    pub const DIFF_REMOVE: Color = Color::Rgb(200, 100, 100);
    pub const DIFF_HUNK: Color = Color::Rgb(100, 140, 200);

    // Modals
    pub const MODAL_BG: Color = Color::Rgb(25, 27, 35);
    pub const MODAL_BORDER_WARNING: Color = Color::Rgb(200, 160, 80);
    pub const MODAL_BORDER_ERROR: Color = Color::Rgb(200, 100, 100);

    // Accent (for confirmations)
    pub const ACCENT_POSITIVE: Color = Color::Rgb(120, 180, 120);
    pub const ACCENT_NEGATIVE: Color = Color::Rgb(200, 100, 100);
}

/// Render the full application UI
#[expect(
    clippy::too_many_lines,
    reason = "render function handles all UI modes in one place"
)]
pub fn render(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    render_main(frame, app, chunks[0]);
    render_status_bar(frame, app, chunks[1]);

    match &app.mode {
        Mode::Help => render_help_overlay(frame),
        Mode::Creating => {
            render_input_overlay(frame, "New Agent", "Enter agent name:", &app.input_buffer);
        }
        Mode::Prompting => render_input_overlay(
            frame,
            "New Agent with Prompt",
            "Enter prompt:",
            &app.input_buffer,
        ),
        Mode::ChildCount => render_count_picker_overlay(frame, app),
        Mode::ChildPrompt => render_input_overlay(
            frame,
            "Spawn Children",
            "Enter task for children:",
            &app.input_buffer,
        ),
        Mode::Broadcasting => render_input_overlay(
            frame,
            "Broadcast Message",
            "Enter message to broadcast to leaf agents:",
            &app.input_buffer,
        ),
        Mode::Confirming(action) => {
            let lines: Vec<Line<'_>> = match action {
                tenex::app::ConfirmAction::Kill => app.selected_agent().map_or_else(
                    || {
                        vec![Line::from(Span::styled(
                            "No agent selected",
                            Style::default().fg(colors::TEXT_PRIMARY),
                        ))]
                    },
                    |agent| {
                        vec![
                            Line::from(Span::styled(
                                "Kill this agent?",
                                Style::default().fg(colors::TEXT_PRIMARY),
                            )),
                            Line::from(""),
                            Line::from(vec![
                                Span::styled("  Name:    ", Style::default().fg(colors::TEXT_DIM)),
                                Span::styled(
                                    &agent.title,
                                    Style::default()
                                        .fg(colors::TEXT_PRIMARY)
                                        .add_modifier(Modifier::BOLD),
                                ),
                            ]),
                            Line::from(vec![
                                Span::styled("  Branch:  ", Style::default().fg(colors::TEXT_DIM)),
                                Span::styled(
                                    &agent.branch,
                                    Style::default().fg(colors::TEXT_PRIMARY),
                                ),
                            ]),
                            Line::from(vec![
                                Span::styled("  Session: ", Style::default().fg(colors::TEXT_DIM)),
                                Span::styled(
                                    &agent.tmux_session,
                                    Style::default().fg(colors::TEXT_PRIMARY),
                                ),
                            ]),
                            Line::from(""),
                            Line::from(Span::styled(
                                "This will delete the worktree and branch.",
                                Style::default().fg(colors::DIFF_REMOVE),
                            )),
                        ]
                    },
                ),
                tenex::app::ConfirmAction::Reset => {
                    vec![Line::from(Span::styled(
                        "Reset all agents?",
                        Style::default().fg(colors::TEXT_PRIMARY),
                    ))]
                }
                tenex::app::ConfirmAction::Quit => {
                    vec![Line::from(Span::styled(
                        "Quit with running agents?",
                        Style::default().fg(colors::TEXT_PRIMARY),
                    ))]
                }
                tenex::app::ConfirmAction::Synthesize => app.selected_agent().map_or_else(
                    || {
                        vec![Line::from(Span::styled(
                            "No agent selected",
                            Style::default().fg(colors::TEXT_PRIMARY),
                        ))]
                    },
                    |agent| {
                        let descendants_count = app.storage.descendants(agent.id).len();
                        let agent_word = if descendants_count == 1 {
                            "agent"
                        } else {
                            "agents"
                        };
                        vec![
                            Line::from(Span::styled(
                                format!("Synthesize {descendants_count} {agent_word}?"),
                                Style::default().fg(colors::TEXT_PRIMARY),
                            )),
                            Line::from(""),
                            Line::from(Span::styled(
                                "This will capture each agent's output, write it to a file,",
                                Style::default().fg(colors::TEXT_DIM),
                            )),
                            Line::from(Span::styled(
                                "and send it to the parent for synthesis.",
                                Style::default().fg(colors::TEXT_DIM),
                            )),
                            Line::from(""),
                            Line::from(Span::styled(
                                "All descendant agents will be terminated.",
                                Style::default().fg(colors::DIFF_REMOVE),
                            )),
                        ]
                    },
                ),
            };
            render_confirm_overlay(frame, lines);
        }
        Mode::ErrorModal(message) => render_error_modal(frame, message),
        _ => {}
    }
}

fn render_main(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    render_agent_list(frame, app, chunks[0]);
    render_content_pane(frame, app, chunks[1]);
}

fn render_agent_list(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let visible = app.storage.visible_agents();

    let items: Vec<ListItem<'_>> = visible
        .iter()
        .enumerate()
        .map(|(i, (agent, depth))| {
            let status_color = match agent.status {
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
            let indent = "    ".repeat(*depth);

            // Collapse/expand indicator
            let has_children = app.storage.has_children(agent.id);
            let collapse_indicator = if has_children {
                if agent.collapsed { "▶ " } else { "▼ " }
            } else {
                ""
            };

            // Child count indicator
            let child_count = app.storage.child_count(agent.id);
            let count_indicator = if child_count > 0 {
                format!(" ({child_count})")
            } else {
                String::new()
            };

            let content = Line::from(vec![
                Span::raw(indent),
                Span::styled(
                    format!("{} ", agent.status.symbol()),
                    Style::default().fg(status_color),
                ),
                Span::styled(collapse_indicator, Style::default().fg(colors::TEXT_DIM)),
                Span::styled(&agent.title, style),
                Span::styled(count_indicator, Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    format!(" ({})", agent.age_string()),
                    Style::default().fg(colors::TEXT_MUTED),
                ),
            ]);

            ListItem::new(content).style(style)
        })
        .collect();

    let title = format!(" Agents ({}) ", app.storage.len());
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_widget(list, area);
}

fn render_content_pane(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_tab_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.preview_content;

    // Parse ANSI escape sequences to preserve terminal colors
    let text = ansi_to_tui::IntoText::into_text(content).unwrap_or_else(|_| {
        // Fallback to plain text if parsing fails
        Text::from(content.as_str())
    });

    let line_count = text.lines.len();
    let visible_height = usize::from(area.height.saturating_sub(2));
    let scroll = app
        .preview_scroll
        .min(line_count.saturating_sub(visible_height));
    let scroll_pos = u16::try_from(scroll).unwrap_or(u16::MAX);

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Terminal Output ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .scroll((scroll_pos, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.diff_content;

    let lines: Vec<Line<'_>> = content
        .lines()
        .map(|line| {
            let color = if line.starts_with('+') && !line.starts_with("+++") {
                colors::DIFF_ADD
            } else if line.starts_with('-') && !line.starts_with("---") {
                colors::DIFF_REMOVE
            } else if line.starts_with("@@") {
                colors::DIFF_HUNK
            } else {
                colors::TEXT_PRIMARY
            };

            Line::styled(line, Style::default().fg(color))
        })
        .collect();

    let visible_height = usize::from(area.height.saturating_sub(2));
    let scroll = app
        .diff_scroll
        .min(lines.len().saturating_sub(visible_height));
    let scroll_pos = u16::try_from(scroll).unwrap_or(u16::MAX);

    let paragraph = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Git Diff ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .scroll((scroll_pos, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    // Don't show error in status bar when error modal is displayed
    let showing_error_modal = matches!(app.mode, Mode::ErrorModal(_));

    let content = match (&app.last_error, &app.status_message, showing_error_modal) {
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

fn render_help_overlay(frame: &mut Frame<'_>) {
    use tenex::config::Action;

    // Calculate height: header(2) + sections with actions + footer(2) + borders(2)
    // 4 sections with headers(4) + empty lines between(3) + 17 actions + footer(2) = 26 + 2 borders
    let area = centered_rect_absolute(50, 28, frame.area());

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
        let key_str = action.keys();
        let description = action.description();

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

fn render_input_overlay(frame: &mut Frame<'_>, title: &str, prompt: &str, input: &str) {
    // 5 lines of content + 2 for borders = 7 lines
    let area = centered_rect_absolute(50, 7, frame.area());

    let text = vec![
        Line::from(Span::styled(
            prompt,
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("{input}_"),
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to confirm, Esc to cancel",
            Style::default().fg(colors::TEXT_MUTED),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(format!(" {title} "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_count_picker_overlay(frame: &mut Frame<'_>, app: &App) {
    // 10 lines of content + 2 for borders = 12 lines
    let area = centered_rect_absolute(40, 12, frame.area());

    let context = if app.spawning_under.is_some() {
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
            format!("        {}", app.child_count),
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

fn render_confirm_overlay(frame: &mut Frame<'_>, mut lines: Vec<Line<'_>>) {
    // Add the yes/no prompt at the end
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "[Y]",
            Style::default()
                .fg(colors::ACCENT_POSITIVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("es  ", Style::default().fg(colors::TEXT_PRIMARY)),
        Span::styled(
            "[N]",
            Style::default()
                .fg(colors::ACCENT_NEGATIVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("o", Style::default().fg(colors::TEXT_PRIMARY)),
    ]));

    // Height: content lines + 2 for borders
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = centered_rect_absolute(50, height, frame.area());

    let text = lines;

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::MODAL_BORDER_WARNING)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_error_modal(frame: &mut Frame<'_>, message: &str) {
    // Wrap the error message to fit within the modal width (44 chars after padding)
    let max_line_width = 44;
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Add error icon and header
    lines.push(Line::from(Span::styled(
        "✖ Error",
        Style::default()
            .fg(colors::MODAL_BORDER_ERROR)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Word-wrap the message
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

    // Add dismiss hint
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press any key to dismiss",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    // Height: content lines + 2 for borders, min 7 lines
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX).max(7);
    let area = centered_rect_absolute(50, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Error ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::MODAL_BORDER_ERROR)),
        )
        .style(Style::default().bg(colors::MODAL_BG))
        .wrap(Wrap { trim: false });

    frame.render_widget(Clear, area);
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

/// Create a centered rect with percentage width and absolute height
fn centered_rect_absolute(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical_padding = area.height.saturating_sub(height) / 2;
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(vertical_padding),
            Constraint::Length(height),
            Constraint::Length(vertical_padding),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;
    use tenex::agent::{Agent, Storage};
    use tenex::app::ConfirmAction;
    use tenex::config::Config;

    fn create_test_config() -> Config {
        Config {
            default_program: "echo".to_string(),
            branch_prefix: "tenex/".to_string(),
            worktree_dir: PathBuf::from("/tmp/test-worktrees"),
            auto_yes: false,
            poll_interval_ms: 100,
            max_agents: 10,
        }
    }

    fn create_test_agent(title: &str, status: Status) -> Agent {
        let mut agent = Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("tenex/{title}"),
            PathBuf::from(format!("/tmp/{title}")),
            None,
        );
        agent.set_status(status);
        agent
    }

    fn create_test_app_with_agents() -> App {
        let config = create_test_config();
        let mut storage = Storage::new();

        storage.add(create_test_agent("agent-1", Status::Running));
        storage.add(create_test_agent("agent-2", Status::Starting));
        storage.add(create_test_agent("agent-3", Status::Running));

        App::new(config, storage)
    }

    #[test]
    fn test_render_normal_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let app = create_test_app_with_agents();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_help_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Help);

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_creating_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_prompting_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Prompting);
        app.handle_char('f');
        app.handle_char('i');
        app.handle_char('x');

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirming_kill_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Confirming(ConfirmAction::Kill));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirming_reset_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Confirming(ConfirmAction::Reset));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirming_quit_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_with_error() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.set_error("Something went wrong!");

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_with_status_message() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.set_status("Operation completed");

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_diff_tab() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.switch_tab();
        assert_eq!(app.active_tab, Tab::Diff);

        // Set diff content with various line types
        app.diff_content = r"diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 unchanged
-removed line
+added line
 context"
            .to_string();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_preview_with_content() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.preview_content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_preview_with_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.preview_content = (0..100)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.preview_scroll = 50;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_diff_with_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.switch_tab();
        app.diff_content = (0..100)
            .map(|i| format!("+Added line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.diff_scroll = 50;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_empty_agents() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let app = App::new(create_test_config(), Storage::new());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_with_selection() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.select_next();
        app.select_next();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_various_sizes() -> Result<(), Box<dyn std::error::Error>> {
        for (width, height) in [(40, 12), (80, 24), (120, 40), (200, 50)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend)?;
            let app = create_test_app_with_agents();

            terminal.draw(|frame| {
                render(frame, &app);
            })?;

            let buffer = terminal.backend().buffer();
            assert!(!buffer.content.is_empty());
        }
        Ok(())
    }

    #[test]
    fn test_render_scrolling_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(Mode::Scrolling);

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_scroll_exceeds_content() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.preview_content = "Line 1\nLine 2".to_string();
        // Set scroll position beyond content length
        app.preview_scroll = 1000;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_error_modal() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.set_error("Something went wrong!");

        // Verify app is in ErrorModal mode
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_error_modal_long_message() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.set_error("This is a very long error message that should wrap to multiple lines in the error modal to ensure the word wrapping functionality works correctly");

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_calculate_preview_dimensions() {
        use ratatui::layout::Rect;

        // Test standard terminal size (80x24)
        let area = Rect::new(0, 0, 80, 24);
        let (width, height) = calculate_preview_dimensions(area);

        // Content width = 80 * 70% = 56, minus 2 for borders = 54
        assert_eq!(width, 54);
        // Height = 24 - 1 (status bar) - 1 (tab bar) - 2 (borders) = 20
        assert_eq!(height, 20);
    }

    #[test]
    fn test_calculate_preview_dimensions_large_terminal() {
        use ratatui::layout::Rect;

        // Test larger terminal (120x40)
        let area = Rect::new(0, 0, 120, 40);
        let (width, height) = calculate_preview_dimensions(area);

        // Content width = 120 * 70% = 84, minus 2 for borders = 82
        assert_eq!(width, 82);
        // Height = 40 - 1 - 1 - 2 = 36
        assert_eq!(height, 36);
    }

    #[test]
    fn test_calculate_preview_dimensions_small_terminal() {
        use ratatui::layout::Rect;

        // Test small terminal (40x10)
        let area = Rect::new(0, 0, 40, 10);
        let (width, height) = calculate_preview_dimensions(area);

        // Content width = 40 * 70% = 28, minus 2 for borders = 26
        assert_eq!(width, 26);
        // Height = 10 - 1 - 1 - 2 = 6
        assert_eq!(height, 6);
    }
}
