//! TUI rendering

use muster::agent::Status;
use muster::app::{App, Mode, Tab};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

/// Render the full application UI
pub fn render(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    render_main(frame, app, chunks[0]);
    render_status_bar(frame, app, chunks[1]);

    match &app.mode {
        Mode::Help => render_help_overlay(frame, &app.config.keys),
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
        Mode::Confirming(action) => {
            let lines: Vec<Line<'_>> = match action {
                muster::app::ConfirmAction::Kill => app.selected_agent().map_or_else(
                    || vec![Line::from("No agent selected")],
                    |agent| {
                        vec![
                            Line::from("Kill this agent?"),
                            Line::from(""),
                            Line::from(vec![
                                Span::raw("  Name:    "),
                                Span::styled(
                                    &agent.title,
                                    Style::default().add_modifier(Modifier::BOLD),
                                ),
                            ]),
                            Line::from(vec![
                                Span::raw("  Branch:  "),
                                Span::styled(&agent.branch, Style::default().fg(Color::Cyan)),
                            ]),
                            Line::from(vec![
                                Span::raw("  Session: "),
                                Span::styled(
                                    &agent.tmux_session,
                                    Style::default().fg(Color::Yellow),
                                ),
                            ]),
                            Line::from(""),
                            Line::from(Span::styled(
                                "This will delete the worktree and branch.",
                                Style::default().fg(Color::Red),
                            )),
                        ]
                    },
                ),
                muster::app::ConfirmAction::Reset => {
                    vec![Line::from("Reset all agents?")]
                }
                muster::app::ConfirmAction::Quit => {
                    vec![Line::from("Quit with running agents?")]
                }
            };
            render_confirm_overlay(frame, lines);
        }
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
                Status::Starting => Color::Yellow,
                Status::Running => Color::Green,
                Status::Paused => Color::Blue,
                Status::Stopped => Color::Red,
            };

            let style = if i == app.selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
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
                Span::styled(collapse_indicator, Style::default().fg(Color::Cyan)),
                Span::styled(&agent.title, style),
                Span::styled(count_indicator, Style::default().fg(Color::Magenta)),
                Span::styled(
                    format!(" ({})", agent.age_string()),
                    Style::default().fg(Color::DarkGray),
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
                .border_style(Style::default().fg(Color::Cyan)),
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
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(name, Style::default().fg(Color::Gray))
            }
        })
        .collect();

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = &app.preview_content;
    let lines: Vec<Line<'_>> = content.lines().map(Line::from).collect();

    let visible_height = usize::from(area.height.saturating_sub(2));
    let scroll = app
        .preview_scroll
        .min(lines.len().saturating_sub(visible_height));
    let scroll_pos = u16::try_from(scroll).unwrap_or(u16::MAX);

    let paragraph = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(" Terminal Output ")
                .borders(Borders::ALL),
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
                Color::Green
            } else if line.starts_with('-') && !line.starts_with("---") {
                Color::Red
            } else if line.starts_with("@@") {
                Color::Cyan
            } else {
                Color::White
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
        .block(Block::default().title(" Git Diff ").borders(Borders::ALL))
        .scroll((scroll_pos, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let content = match (&app.last_error, &app.status_message) {
        (Some(error), _) => Span::styled(
            format!(" Error: {error} "),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        (None, Some(status)) => {
            Span::styled(format!(" {status} "), Style::default().fg(Color::Green))
        }
        (None, None) => {
            let running = app.running_agent_count();
            let hints = app.config.keys.status_hints();
            Span::styled(
                format!(" {running} running | {hints} "),
                Style::default().fg(Color::Gray),
            )
        }
    };

    let paragraph = Paragraph::new(Line::from(content)).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(paragraph, area);
}

fn render_help_overlay(frame: &mut Frame<'_>, keys: &muster::config::KeyBindings) {
    use muster::config::Action;

    // Calculate height: header(2) + sections with actions + footer(2) + borders(2)
    // 4 sections with headers(4) + empty lines between(3) + 17 actions + footer(2) = 26 + 2 borders
    let area = centered_rect_absolute(50, 28, frame.area());

    let mut help_text = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
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
                Style::default().fg(Color::Cyan),
            )));
            current_group = Some(group);
        }

        help_text.push(Line::from(keys.help_line(action)));
    }

    help_text.push(Line::from(""));
    help_text.push(Line::from(Span::styled(
        "Press any key to close",
        Style::default().fg(Color::Gray),
    )));

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().bg(Color::Black));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_input_overlay(frame: &mut Frame<'_>, title: &str, prompt: &str, input: &str) {
    // 5 lines of content + 2 for borders = 7 lines
    let area = centered_rect_absolute(50, 7, frame.area());

    let text = vec![
        Line::from(prompt),
        Line::from(""),
        Line::from(Span::styled(
            format!("{input}_"),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press Enter to confirm, Esc to cancel",
            Style::default().fg(Color::Gray),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(format!(" {title} "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().bg(Color::Black));

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
        Line::from(Span::styled(context, Style::default().fg(Color::Gray))),
        Line::from(""),
        Line::from("How many child agents?"),
        Line::from(""),
        Line::from(Span::styled("        ▲", Style::default().fg(Color::Cyan))),
        Line::from(Span::styled(
            format!("        {}", app.child_count),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled("        ▼", Style::default().fg(Color::Cyan))),
        Line::from(""),
        Line::from(Span::styled(
            "↑/k to increase, ↓/j to decrease",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Enter to continue, Esc to cancel",
            Style::default().fg(Color::Gray),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Spawn Children ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().bg(Color::Black));

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
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("es  "),
        Span::styled(
            "[N]",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw("o"),
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
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .style(Style::default().bg(Color::Black));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
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
    use muster::agent::{Agent, Storage};
    use muster::app::ConfirmAction;
    use muster::config::Config;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn create_test_config() -> Config {
        Config {
            default_program: "echo".to_string(),
            branch_prefix: "muster/".to_string(),
            worktree_dir: PathBuf::from("/tmp/test-worktrees"),
            auto_yes: false,
            poll_interval_ms: 100,
            max_agents: 10,
            keys: muster::config::KeyBindings::default(),
        }
    }

    fn create_test_agent(title: &str, status: Status) -> Agent {
        let mut agent = Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("muster/{title}"),
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
        storage.add(create_test_agent("agent-2", Status::Paused));
        storage.add(create_test_agent("agent-3", Status::Stopped));
        storage.add(create_test_agent("agent-4", Status::Starting));

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
}
