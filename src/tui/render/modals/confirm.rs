//! Confirmation modal rendering

use crate::app::App;
use crate::update::UpdateInfo;
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render a confirmation overlay with yes/no buttons
pub fn render_confirm_overlay(frame: &mut Frame<'_>, mut lines: Vec<Line<'_>>) {
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

/// Render the worktree conflict overlay
#[expect(
    clippy::too_many_lines,
    reason = "UI layout requires verbose styling code"
)]
pub fn render_worktree_conflict_overlay(frame: &mut Frame<'_>, app: &App) {
    let Some(conflict) = &app.spawn.worktree_conflict else {
        return;
    };

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(Span::styled(
            "Worktree Already Exists",
            Style::default()
                .fg(colors::MODAL_BORDER_WARNING)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("A worktree for \"{}\" already exists.", conflict.title),
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
    ];

    // Show existing worktree info
    lines.push(Line::from(Span::styled(
        "Existing worktree:",
        Style::default()
            .fg(colors::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    )));

    if let Some(ref branch) = conflict.existing_branch {
        lines.push(Line::from(vec![
            Span::styled("  Branch: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(branch.clone(), Style::default().fg(colors::TEXT_PRIMARY)),
        ]));
    }

    if let Some(ref commit) = conflict.existing_commit {
        lines.push(Line::from(vec![
            Span::styled("  Commit: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(commit.clone(), Style::default().fg(colors::TEXT_MUTED)),
        ]));
    }

    lines.push(Line::from(""));

    // Show what a new worktree would be based on
    lines.push(Line::from(Span::styled(
        "New worktree would be based on:",
        Style::default()
            .fg(colors::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("  Branch: ", Style::default().fg(colors::TEXT_DIM)),
        Span::styled(
            conflict.current_branch.clone(),
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Commit: ", Style::default().fg(colors::TEXT_DIM)),
        Span::styled(
            conflict.current_commit.clone(),
            Style::default().fg(colors::TEXT_MUTED),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "What would you like to do?",
        Style::default().fg(colors::TEXT_PRIMARY),
    )));
    lines.push(Line::from(""));

    // Add the choice buttons
    lines.push(Line::from(vec![
        Span::styled(
            "[R]",
            Style::default()
                .fg(colors::ACCENT_POSITIVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "econnect to existing worktree",
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        "    (you can edit the prompt before starting)",
        Style::default().fg(colors::TEXT_MUTED),
    )));
    lines.push(Line::from(vec![
        Span::styled(
            "[D]",
            Style::default()
                .fg(colors::ACCENT_NEGATIVE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "elete and create new from ",
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
        Span::styled(
            conflict.current_branch.clone(),
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            "[Esc]",
            Style::default()
                .fg(colors::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Cancel", Style::default().fg(colors::TEXT_MUTED)),
    ]));

    // Height: content lines + 2 for borders
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = centered_rect_absolute(60, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Worktree Conflict ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::MODAL_BORDER_WARNING)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// Render the confirm push overlay
pub fn render_confirm_push_overlay(frame: &mut Frame<'_>, app: &App) {
    let agent = app.git_op.agent_id.and_then(|id| app.storage.get(id));

    let mut lines: Vec<Line<'_>> = vec![
        Line::from(Span::styled(
            "Push Branch to Remote?",
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if let Some(agent) = agent {
        lines.push(Line::from(vec![
            Span::styled("  Agent:  ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(
                &agent.title,
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  Branch: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(
                &app.git_op.branch_name,
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "Agent not found",
            Style::default().fg(colors::MODAL_BORDER_ERROR),
        )));
    }

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

    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = centered_rect_absolute(50, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Push Branch ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// Render the keyboard remap prompt overlay
/// Shows when terminal doesn't support Kitty keyboard protocol
pub fn render_keyboard_remap_overlay(frame: &mut Frame<'_>) {
    let lines: Vec<Line<'_>> = vec![
        Line::from(Span::styled(
            "Keyboard Compatibility Notice",
            Style::default()
                .fg(colors::ACCENT_WARNING)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "This terminal does not support merging with the",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            "standard [Ctrl+M] command (it is interpreted as",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(Span::styled(
            "[Enter] in older terminals).",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Either check if newer versions of your terminal",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(Span::styled(
            "support the Kitty keyboard protocol, or we can",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(Span::styled(
            "remap [Ctrl+M] to [Ctrl+N] for your convenience.",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Would you like to remap the merge key?",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[Y]",
                Style::default()
                    .fg(colors::ACCENT_POSITIVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("es - Use ", Style::default().fg(colors::TEXT_PRIMARY)),
            Span::styled(
                "[Ctrl+N]",
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" for merge", Style::default().fg(colors::TEXT_PRIMARY)),
        ]),
        Line::from(vec![
            Span::styled(
                "[N]",
                Style::default()
                    .fg(colors::ACCENT_NEGATIVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("o  - Keep ", Style::default().fg(colors::TEXT_PRIMARY)),
            Span::styled(
                "[Ctrl+M]",
                Style::default()
                    .fg(colors::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (won't work)", Style::default().fg(colors::TEXT_MUTED)),
        ]),
    ];

    // Height: content lines + 2 for borders
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = centered_rect_absolute(55, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Keyboard Settings ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::ACCENT_WARNING)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// Render the self-update prompt overlay.
///
/// Shows when a newer Tenex version is available on crates.io.
pub fn render_update_prompt_overlay(frame: &mut Frame<'_>, info: &UpdateInfo) {
    let lines: Vec<Line<'_>> = vec![
        Line::from(Span::styled(
            "Update Available",
            Style::default()
                .fg(colors::ACCENT_POSITIVE)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Current version: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(
                info.current_version.to_string(),
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled("Latest version:  ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(
                info.latest_version.to_string(),
                Style::default().fg(colors::ACCENT_POSITIVE),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Would you like to update Tenex now?",
            Style::default().fg(colors::TEXT_PRIMARY),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[Y]",
                Style::default()
                    .fg(colors::ACCENT_POSITIVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "es - Install update and restart",
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "[N]",
                Style::default()
                    .fg(colors::ACCENT_NEGATIVE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "o  - Continue without updating",
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
        ]),
    ];

    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let area = centered_rect_absolute(55, height, frame.area());

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Update Tenex ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::ACCENT_POSITIVE)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// Render the confirm push for PR overlay
pub fn render_confirm_push_for_pr_overlay(frame: &mut Frame<'_>, app: &App) {
    // 9 lines of content + 2 for borders = 11 lines
    let area = centered_rect_absolute(55, 11, frame.area());

    let text = vec![
        Line::from(Span::styled(
            "Push and Open Pull Request?",
            Style::default()
                .fg(colors::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "You have unpushed commits.",
            Style::default().fg(colors::ACCENT_WARNING),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Branch: ", Style::default().fg(colors::TEXT_DIM)),
            Span::styled(
                &app.git_op.branch_name,
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
            Span::styled(" → ", Style::default().fg(colors::TEXT_MUTED)),
            Span::styled(
                &app.git_op.base_branch,
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Push commits and open PR in browser?",
            Style::default().fg(colors::TEXT_DIM),
        )),
        Line::from(""),
        Line::from(vec![
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
        ]),
    ];

    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .title(" Open PR ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::ACCENT_POSITIVE)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::{Agent, App, agent::Storage};
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;

    fn app_with_agent() -> App {
        let mut app = App::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );
        let agent = Agent::new(
            "render-agent".to_string(),
            "echo".to_string(),
            "tenex/render-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let id = agent.id;
        app.storage.add(agent);
        app.git_op.agent_id = Some(id);
        app.git_op.branch_name = "tenex/render-agent".to_string();
        app
    }

    #[test]
    fn test_render_confirm_overlay_renders_content() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| {
            render_confirm_overlay(frame, vec![Line::from("Testing confirm overlay")]);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirm_push_overlay_without_agent() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let app = App::new(
            Config::default(),
            Storage::new(),
            Settings::default(),
            false,
        );

        terminal.draw(|frame| {
            render_confirm_push_overlay(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirm_push_overlay_with_agent() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let app = app_with_agent();

        terminal.draw(|frame| {
            render_confirm_push_overlay(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirm_push_for_pr_overlay() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = app_with_agent();
        app.git_op.base_branch = "main".to_string();

        terminal.draw(|frame| {
            render_confirm_push_for_pr_overlay(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_keyboard_remap_overlay() -> Result<(), std::io::Error> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| {
            render_keyboard_remap_overlay(frame);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }
}
