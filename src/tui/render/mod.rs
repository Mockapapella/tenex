//! TUI rendering
//!
//! This module contains all rendering logic for the TUI, organized into:
//! - `colors`: Color palette definitions
//! - `main_layout`: Main layout rendering (agent list, content pane, status bar)
//! - `modals`: Modal/overlay rendering

pub mod colors;
pub mod main_layout;
pub mod modals;

use crate::app::{
    App, BranchPickerKind, ConfirmAction, ConfirmKind, CountPickerKind, Mode, OverlayMode,
    TextInputKind,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
};

// Re-export main layout functions for convenience
pub use main_layout::calculate_preview_dimensions;

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

    main_layout::render_main(frame, app, chunks[0]);
    main_layout::render_status_bar(frame, app, chunks[1]);

    match &app.mode {
        Mode::Overlay(overlay) => {
            match overlay {
                OverlayMode::Help => modals::render_help_overlay(frame, app),
                OverlayMode::CommandPalette => modals::render_command_palette_overlay(frame, app),
                OverlayMode::ModelSelector => modals::render_model_selector_overlay(frame, app),
                OverlayMode::TextInput(TextInputKind::RenameBranch) => {
                    modals::render_rename_overlay(frame, app);
                }
                OverlayMode::TextInput(kind) => {
                    if let Some((title, prompt)) = kind.input_overlay_spec(app) {
                        modals::render_input_overlay(
                            frame,
                            title,
                            prompt,
                            &app.input.buffer,
                            app.input.cursor,
                        );
                    }
                }
                OverlayMode::CountPicker(CountPickerKind::ChildCount) => {
                    modals::render_count_picker_overlay(frame, app);
                }
                OverlayMode::CountPicker(CountPickerKind::ReviewChildCount) => {
                    modals::render_review_count_picker_overlay(frame, app);
                }
                OverlayMode::ReviewInfo => modals::render_review_info_overlay(frame),
                OverlayMode::BranchPicker(
                    BranchPickerKind::ReviewBaseBranch
                    | BranchPickerKind::RebaseTargetBranch
                    | BranchPickerKind::MergeFromBranch,
                ) => {
                    modals::render_branch_selector_overlay(frame, app);
                }
                OverlayMode::Confirm(ConfirmKind::Action(action)) => {
                    let lines: Vec<Line<'_>> = match action {
                ConfirmAction::Kill => app.selected_agent().map_or_else(
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
                                    &agent.mux_session,
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
                ConfirmAction::Reset => {
                    vec![Line::from(Span::styled(
                        "Reset all agents?",
                        Style::default().fg(colors::TEXT_PRIMARY),
                    ))]
                }
                ConfirmAction::Quit => {
                    vec![Line::from(Span::styled(
                        "Quit with running agents?",
                        Style::default().fg(colors::TEXT_PRIMARY),
                    ))]
                }
                ConfirmAction::Synthesize => app.selected_agent().map_or_else(
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
                ConfirmAction::WorktreeConflict => {
                    // This case is handled by render_worktree_conflict_overlay
                    vec![]
                }
            };

                    // Special handling for worktree conflict with different buttons
                    if matches!(action, ConfirmAction::WorktreeConflict) {
                        modals::render_worktree_conflict_overlay(frame, app);
                    } else {
                        modals::render_confirm_overlay(frame, lines);
                    }
                }
                OverlayMode::Confirm(ConfirmKind::Push) => {
                    modals::render_confirm_push_overlay(frame, app);
                }
                OverlayMode::Confirm(ConfirmKind::PushForPR) => {
                    modals::render_confirm_push_for_pr_overlay(frame, app);
                }
                OverlayMode::Confirm(ConfirmKind::KeyboardRemap) => {
                    modals::render_keyboard_remap_overlay(frame);
                }
                OverlayMode::Confirm(ConfirmKind::UpdatePrompt(info)) => {
                    modals::render_update_prompt_overlay(frame, info);
                }
                OverlayMode::Error(message) => modals::render_error_modal(frame, message),
                OverlayMode::Success(message) => modals::render_success_modal(frame, message),
            }
        }

        Mode::Normal | Mode::Scrolling | Mode::PreviewFocused | Mode::UpdateRequested(_) => {}
    }
}

#[cfg(test)]
mod tests;
