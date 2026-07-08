//! TUI rendering
//!
//! This module contains all rendering logic for the TUI, organized into:
//! - `colors`: Color palette definitions
//! - `main_layout`: Main layout rendering (agent list, content pane, status bar)
//! - `modals`: Modal/overlay rendering

pub mod colors;
pub mod main_layout;
pub mod modals;

use crate::app::AgentRole;
use crate::app::App;
use crate::state::{AppMode, ConfirmAction};
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
        AppMode::Changelog(state) => modals::render_changelog_overlay(frame, app, state),
        AppMode::Help(_) => modals::render_help_overlay(frame, app),
        AppMode::CommandPalette(_) => modals::render_command_palette_overlay(frame, app),
        AppMode::Creating(_) => {
            modals::render_input_overlay(
                frame,
                "New Agent",
                "Enter agent name:",
                &app.data.input.buffer,
                app.data.input.cursor,
            );
        }
        AppMode::Prompting(_) => modals::render_input_overlay(
            frame,
            "New Agent with Prompt",
            "Enter prompt:",
            &app.data.input.buffer,
            app.data.input.cursor,
        ),
        AppMode::ChildCount(_) => modals::render_count_picker_overlay(frame, app),
        AppMode::ChildPrompt(_) => modals::render_input_overlay(
            frame,
            "Spawn Children",
            "Enter task for children:",
            &app.data.input.buffer,
            app.data.input.cursor,
        ),
        AppMode::Broadcasting(_) => modals::render_input_overlay(
            frame,
            "Broadcast Message",
            "Enter message to broadcast to leaf agents:",
            &app.data.input.buffer,
            app.data.input.cursor,
        ),
        AppMode::ReconnectPrompt(_) => {
            let title = app
                .data
                .spawn
                .worktree_conflict
                .as_ref()
                .map_or("Reconnect", |c| {
                    if c.swarm_child_count.is_some() {
                        "Reconnect Swarm"
                    } else {
                        "Reconnect Agent"
                    }
                });
            modals::render_input_overlay(
                frame,
                title,
                "Edit prompt (or leave empty):",
                &app.data.input.buffer,
                app.data.input.cursor,
            );
        }
        AppMode::TerminalPrompt(_) => modals::render_input_overlay(
            frame,
            "New Terminal",
            "Enter startup command (or leave empty):",
            &app.data.input.buffer,
            app.data.input.cursor,
        ),
        AppMode::CustomAgentCommand(_) => {
            let (title, prompt) = match app.data.model_selector.role {
                AgentRole::Default => (
                    "Custom Agent Command",
                    "Enter the command to run for new agents:",
                ),
                AgentRole::Planner => (
                    "Custom Planner Command",
                    "Enter the command to run for planner agents:",
                ),
                AgentRole::Review => (
                    "Custom Review Command",
                    "Enter the command to run for review agents:",
                ),
            };

            modals::render_input_overlay(
                frame,
                title,
                prompt,
                &app.data.input.buffer,
                app.data.input.cursor,
            );
        }
        AppMode::SynthesisPrompt(_) => modals::render_input_overlay(
            frame,
            "Synthesize",
            "Add extra instructions for the parent agent (optional):",
            &app.data.input.buffer,
            app.data.input.cursor,
        ),
        AppMode::Confirming(state) => {
            let action = state.action;
            let lines: Vec<Line<'_>> = match action {
                ConfirmAction::Kill => app.selected_agent().map_or_else(
                    || {
                        vec![Line::from(Span::styled(
                            "No agent selected",
                            Style::default().fg(colors::TEXT_PRIMARY),
                        ))]
                    },
                    |agent| {
                        let warning = if agent.is_root() && agent.is_git_workspace() {
                            let delete_branch =
                                agent.branch.starts_with(&app.data.config.branch_prefix)
                                    || agent.branch.starts_with("tenex/");
                            if delete_branch {
                                "This will delete the worktree and branch."
                            } else {
                                "This will delete the worktree."
                            }
                        } else if agent.is_root() {
                            "This will close the session and stop the agent."
                        } else {
                            "This will close the window and stop the agent."
                        };

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
                                warning,
                                Style::default().fg(colors::DIFF_REMOVE),
                            )),
                        ]
                    },
                ),
                ConfirmAction::InterruptAgent => app.selected_agent().map_or_else(
                    || {
                        vec![Line::from(Span::styled(
                            "No agent selected",
                            Style::default().fg(colors::TEXT_PRIMARY),
                        ))]
                    },
                    |agent| {
                        vec![
                            Line::from(Span::styled(
                                "Send Ctrl+C to this agent?",
                                Style::default()
                                    .fg(colors::TEXT_PRIMARY)
                                    .add_modifier(Modifier::BOLD),
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
                                "This may terminate the agent and close its pane.",
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
                ConfirmAction::RestartMuxDaemon => {
                    app.data.ui.muxd_version_mismatch.as_ref().map_or_else(
                        || {
                            vec![Line::from(Span::styled(
                                "Restart mux daemon?",
                                Style::default().fg(colors::TEXT_PRIMARY),
                            ))]
                        },
                        |info| {
                            let mut lines = vec![
                                Line::from(Span::styled(
                                    "Restart mux daemon?",
                                    Style::default()
                                        .fg(colors::TEXT_PRIMARY)
                                        .add_modifier(Modifier::BOLD),
                                )),
                                Line::from(""),
                                Line::from(vec![
                                    Span::styled(
                                        "  Daemon: ",
                                        Style::default().fg(colors::TEXT_DIM),
                                    ),
                                    Span::styled(
                                        info.daemon_version.as_str(),
                                        Style::default().fg(colors::TEXT_PRIMARY),
                                    ),
                                ]),
                                Line::from(vec![
                                    Span::styled(
                                        "  Tenex:  ",
                                        Style::default().fg(colors::TEXT_DIM),
                                    ),
                                    Span::styled(
                                        info.expected_version.as_str(),
                                        Style::default().fg(colors::TEXT_PRIMARY),
                                    ),
                                ]),
                                Line::from(vec![
                                    Span::styled(
                                        "  Socket: ",
                                        Style::default().fg(colors::TEXT_DIM),
                                    ),
                                    Span::styled(
                                        info.socket.as_str(),
                                        Style::default().fg(colors::TEXT_MUTED),
                                    ),
                                ]),
                            ];

                            if let Some(env_socket) = info.env_mux_socket.as_deref() {
                                lines.push(Line::from(vec![
                                    Span::styled(
                                        "  Env:    ",
                                        Style::default().fg(colors::TEXT_DIM),
                                    ),
                                    Span::styled(
                                        format!("TENEX_MUX_SOCKET={env_socket}"),
                                        Style::default().fg(colors::TEXT_MUTED),
                                    ),
                                ]));
                            }

                            lines.push(Line::from(""));
                            lines.push(Line::from(Span::styled(
                                "All running agent sessions will be restarted.",
                                Style::default().fg(colors::DIFF_REMOVE),
                            )));

                            lines
                        },
                    )
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
                        let targets = app.data.synthesis_targets_for(agent.id);
                        let descendants_count = targets.capture_agent_ids.len();
                        if descendants_count == 0 {
                            return vec![Line::from(Span::styled(
                                "No non-terminal descendants to synthesize",
                                Style::default().fg(colors::TEXT_PRIMARY),
                            ))];
                        }
                        let agent_word = if descendants_count == 1 {
                            "agent"
                        } else {
                            "agents"
                        };
                        let cleanup_warning = if targets.marked {
                            "Marked descendant subtrees will be terminated."
                        } else {
                            "All synthesized descendant subtrees will be terminated."
                        };
                        vec![
                            Line::from(Span::styled(
                                format!("Synthesize {descendants_count} {agent_word}?"),
                                Style::default().fg(colors::TEXT_PRIMARY),
                            )),
                            Line::from(""),
                            Line::from(Span::styled(
                                "This will capture each non-terminal agent's output, write it to a file,",
                                Style::default().fg(colors::TEXT_DIM),
                            )),
                            Line::from(Span::styled(
                                "and send it to the parent for synthesis.",
                                Style::default().fg(colors::TEXT_DIM),
                            )),
                            Line::from(Span::styled(
                                "You'll be prompted for optional extra instructions.",
                                Style::default().fg(colors::TEXT_DIM),
                            )),
                            Line::from(""),
                            Line::from(Span::styled(
                                cleanup_warning,
                                Style::default().fg(colors::DIFF_REMOVE),
                            )),
                        ]
                    },
                ),
                ConfirmAction::WorktreeConflict => {
                    // This case is handled by render_worktree_conflict_overlay
                    vec![]
                }
                ConfirmAction::SwitchBranch => {
                    let from_branch = app.data.git_op.branch_name.clone();
                    let to_branch = app.data.git_op.target_branch.clone();
                    let to_display = if to_branch.is_empty() {
                        "<none selected>".to_string()
                    } else {
                        to_branch
                    };

                    vec![
                        Line::from(Span::styled(
                            "Switch Branch?",
                            Style::default().fg(colors::TEXT_PRIMARY),
                        )),
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("  From: ", Style::default().fg(colors::TEXT_DIM)),
                            Span::styled(
                                from_branch,
                                Style::default().fg(colors::TEXT_PRIMARY),
                            ),
                        ]),
                        Line::from(vec![
                            Span::styled("  To:   ", Style::default().fg(colors::TEXT_DIM)),
                            Span::styled(
                                to_display,
                                Style::default()
                                    .fg(colors::TEXT_PRIMARY)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::from(""),
                        Line::from(Span::styled(
                            "Kills current agent and all children.",
                            Style::default().fg(colors::DIFF_REMOVE),
                        )),
                        Line::from(Span::styled(
                            "Deletes old worktree; uncommitted work is lost.",
                            Style::default().fg(colors::DIFF_REMOVE),
                        )),
                    ]
                }
            };

            // Special handling for worktree conflict with different buttons
            if matches!(action, ConfirmAction::WorktreeConflict) {
                modals::render_worktree_conflict_overlay(frame, app);
            } else {
                modals::render_confirm_overlay(frame, lines);
            }
        }
        AppMode::ErrorModal(state) => modals::render_error_modal(frame, &state.message),
        AppMode::PreparingDocker(state) => {
            modals::render_preparing_docker_modal(frame, &state.message);
        }
        AppMode::ReviewInfo(_) => modals::render_review_info_overlay(frame),
        AppMode::ReviewChildCount(_) => modals::render_review_count_picker_overlay(frame, app),
        AppMode::BranchSelector(_)
        | AppMode::RebaseBranchSelector(_)
        | AppMode::MergeBranchSelector(_)
        | AppMode::SwitchBranchSelector(_) => {
            modals::render_branch_selector_overlay(frame, app);
        }
        AppMode::ModelSelector(_) => modals::render_model_selector_overlay(frame, app),
        AppMode::SettingsMenu(_) => modals::render_settings_menu_overlay(frame, app),
        AppMode::ConfirmPush(_) => modals::render_confirm_push_overlay(frame, app),
        AppMode::RenameBranch(_) => modals::render_rename_overlay(frame, app),
        AppMode::ConfirmPushForPR(_) => modals::render_confirm_push_for_pr_overlay(frame, app),
        AppMode::SuccessModal(state) => modals::render_success_modal(frame, &state.message),
        AppMode::KeyboardRemapPrompt(_) => modals::render_keyboard_remap_overlay(frame),
        AppMode::UpdatePrompt(state) => modals::render_update_prompt_overlay(frame, &state.info),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
