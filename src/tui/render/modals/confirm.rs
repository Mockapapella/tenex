//! Confirmation modal rendering

use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use tenex::app::App;

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
    let Some(conflict) = &app.worktree_conflict else {
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
    let agent = app.git_op_agent_id.and_then(|id| app.storage.get(id));

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
                &app.git_op_branch_name,
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
                &app.git_op_branch_name,
                Style::default().fg(colors::TEXT_PRIMARY),
            ),
            Span::styled(" → ", Style::default().fg(colors::TEXT_MUTED)),
            Span::styled(
                &app.git_op_base_branch,
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
