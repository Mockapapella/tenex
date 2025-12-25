//! Branch selector modal rendering

use crate::app::{App, Mode};
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::centered_rect_absolute;
use crate::tui::render::colors;

/// Render the branch selector overlay
#[expect(
    clippy::too_many_lines,
    reason = "Branch selector has many UI elements"
)]
pub fn render_branch_selector_overlay(frame: &mut Frame<'_>, app: &App) {
    // Get current branch for context
    let current_branch = &app.git_op.branch_name;

    // Determine title based on mode
    let title = match &app.mode {
        Mode::RebaseBranchSelector => " Rebase onto Branch ",
        Mode::MergeBranchSelector => " Merge Branch ",
        _ => " Select Base Branch ",
    };

    // Calculate how many branches we can display
    let max_visible_branches: usize = 10;
    let header_lines: u16 = 7; // Title + instruction + search box + section headers
    let footer_lines: u16 = 3; // Instructions + border
    // Safe cast: max_visible_branches is a small constant (10)
    #[expect(
        clippy::cast_possible_truncation,
        reason = "max_visible_branches is small constant"
    )]
    let total_height = header_lines + (max_visible_branches as u16) + footer_lines;
    let area = centered_rect_absolute(60, total_height, frame.area());

    let filtered = app.filtered_review_branches();
    let selected_idx = app.review.selected;
    let total_count = filtered.len();

    // Calculate scroll offset to keep selection visible
    let scroll_offset = if selected_idx >= max_visible_branches {
        selected_idx - max_visible_branches + 1
    } else {
        0
    };

    // Build list content with sections
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Show merge/rebase direction instruction
    match &app.mode {
        Mode::MergeBranchSelector => {
            lines.push(Line::from(vec![
                Span::styled(
                    "Select branch to merge ",
                    Style::default().fg(colors::TEXT_DIM),
                ),
                Span::styled(
                    current_branch.clone(),
                    Style::default()
                        .fg(colors::ACCENT_POSITIVE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" into", Style::default().fg(colors::TEXT_DIM)),
            ]));
        }
        Mode::RebaseBranchSelector => {
            lines.push(Line::from(vec![
                Span::styled("Rebase ", Style::default().fg(colors::TEXT_DIM)),
                Span::styled(
                    current_branch.clone(),
                    Style::default()
                        .fg(colors::ACCENT_POSITIVE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " onto selected branch",
                    Style::default().fg(colors::TEXT_DIM),
                ),
            ]));
        }
        _ => {
            lines.push(Line::from(Span::styled(
                "Select base branch for review",
                Style::default().fg(colors::TEXT_DIM),
            )));
        }
    }
    lines.push(Line::from(""));

    // Search box
    lines.push(Line::from(vec![
        Span::styled("Search: ", Style::default().fg(colors::TEXT_DIM)),
        Span::styled(
            format!("{}_", &app.review.filter),
            Style::default().fg(colors::TEXT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(""));

    // Track if we've shown section headers
    let mut shown_local_header = false;
    let mut shown_remote_header = false;
    let mut displayed_count = 0;

    // Iterate through filtered branches with proper indexing
    for (idx, branch) in filtered.iter().enumerate() {
        // Skip branches before scroll offset
        if idx < scroll_offset {
            // But still track if we passed local branches for header logic
            if !branch.is_remote {
                shown_local_header = true;
            }
            continue;
        }

        // Stop if we've shown enough branches
        if displayed_count >= max_visible_branches {
            break;
        }

        // Show section header when transitioning
        if !branch.is_remote && !shown_local_header {
            lines.push(Line::from(Span::styled(
                "── Local ──",
                Style::default().fg(colors::TEXT_MUTED),
            )));
            shown_local_header = true;
        } else if branch.is_remote && !shown_remote_header {
            lines.push(Line::from(Span::styled(
                "── Remote ──",
                Style::default().fg(colors::TEXT_MUTED),
            )));
            shown_remote_header = true;
        }

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

        // Format branch name
        let display_name = if branch.is_remote {
            let remote_prefix = branch.remote.as_deref().unwrap_or("");
            if remote_prefix.is_empty() {
                branch.name.clone()
            } else {
                format!("{}/{}", remote_prefix, branch.name)
            }
        } else {
            branch.name.clone()
        };

        lines.push(Line::from(Span::styled(
            format!("{prefix}{display_name}"),
            style,
        )));
        displayed_count += 1;
    }

    // Show scroll indicator if there are more branches
    if total_count > max_visible_branches {
        let hidden_above = scroll_offset;
        let hidden_below = total_count.saturating_sub(scroll_offset + max_visible_branches);
        if hidden_above > 0 || hidden_below > 0 {
            let indicator = match (hidden_above > 0, hidden_below > 0) {
                (true, true) => format!("  ↑{hidden_above} more above, ↓{hidden_below} more below"),
                (true, false) => format!("  ↑{hidden_above} more above"),
                (false, true) => format!("  ↓{hidden_below} more below"),
                (false, false) => String::new(),
            };
            if !indicator.is_empty() {
                lines.push(Line::from(Span::styled(
                    indicator,
                    Style::default().fg(colors::TEXT_MUTED),
                )));
            }
        }
    }

    // Empty state
    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matching branches",
            Style::default().fg(colors::TEXT_MUTED),
        )));
    }

    // Instructions
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↑/↓ select • Enter confirm • Esc cancel",
        Style::default().fg(colors::TEXT_MUTED),
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        )
        .style(Style::default().bg(colors::MODAL_BG));

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
