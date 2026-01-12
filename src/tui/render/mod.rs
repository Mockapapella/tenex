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
                        let descendants_count = app.data.storage.descendants(agent.id).len();
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
        AppMode::ErrorModal(state) => modals::render_error_modal(frame, &state.message),
        AppMode::ReviewInfo(_) => modals::render_review_info_overlay(frame),
        AppMode::ReviewChildCount(_) => modals::render_review_count_picker_overlay(frame, app),
        AppMode::BranchSelector(_)
        | AppMode::RebaseBranchSelector(_)
        | AppMode::MergeBranchSelector(_) => {
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
mod tests {
    use super::*;
    use crate::agent::{Agent, Status, Storage};
    use crate::config::Config;
    use crate::state::*;
    use ratatui::Terminal;
    use ratatui::backend::Backend;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn create_test_config() -> Config {
        // Use a unique temp directory to avoid conflicts with real worktrees
        let pid = std::process::id();
        Config {
            default_program: "echo".to_string(),
            branch_prefix: format!("tenex-render-test-{pid}/"),
            worktree_dir: PathBuf::from(format!("/tmp/tenex-render-test-{pid}")),
            auto_yes: false,
            poll_interval_ms: 100,
        }
    }

    fn create_test_agent(title: &str, status: Status) -> Agent {
        let pid = std::process::id();
        let mut agent = Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("tenex-render-test-{pid}/{title}"),
            PathBuf::from(format!("/tmp/tenex-render-test-{pid}/{title}")),
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

        App::new(config, storage, crate::app::Settings::default(), false)
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
    fn test_render_content_pane_has_top_border() -> Result<(), Box<dyn std::error::Error>> {
        let width = 80_u16;
        let backend = TestBackend::new(width, 24);
        let mut terminal = Terminal::new(backend)?;
        let app = create_test_app_with_agents();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();

        // Main layout is a 30/70 split; the content pane starts at 30%.
        let content_x = u16::try_from((u32::from(width) * 30) / 100).unwrap_or(0);
        let top_left = buffer
            .cell((content_x, 0))
            .map(ratatui::buffer::Cell::symbol);
        let top_right = buffer
            .cell((width.saturating_sub(1), 0))
            .map(ratatui::buffer::Cell::symbol);

        assert_eq!(top_left, Some("╔"));
        assert_eq!(top_right, Some("╗"));
        Ok(())
    }

    #[test]
    fn test_render_help_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(HelpMode.into());

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
        app.enter_mode(CreatingMode.into());
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
        app.enter_mode(PromptingMode.into());
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
    fn test_render_prompting_mode_with_scrollbar() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(PromptingMode.into());
        app.data.input.buffer = (0..30)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.input.cursor = app.data.input.buffer.len();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for cell in &buffer.content {
            text.push_str(cell.symbol());
        }

        assert!(text.contains('█'));
        Ok(())
    }

    #[test]
    fn test_render_confirming_kill_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into(),
        );

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
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Reset,
            }
            .into(),
        );

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
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

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
    fn test_render_status_bar_shows_error_when_not_in_modal()
    -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.data.ui.last_error = Some("boom".to_string());
        app.mode = AppMode::normal();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for cell in &buffer.content {
            text.push_str(cell.symbol());
        }
        assert!(text.contains("Error: boom"));
        Ok(())
    }

    #[test]
    fn test_render_diff_tab() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.switch_tab();
        assert_eq!(app.data.active_tab, crate::app::Tab::Diff);

        // Set diff content with various line types
        app.data.ui.set_diff_content(
            r"diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 unchanged
-removed line
+added line
 context",
        );

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
        app.data.ui.preview_content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string();

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
        app.data.ui.preview_content = (0..100)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.ui.preview_scroll = 50;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_preview_focused_sets_cursor_position_simple()
    -> Result<(), Box<dyn std::error::Error>> {
        use ratatui::layout::Position;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(PreviewFocusedMode.into());
        app.data.ui.preview_content = (0..10)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.ui.preview_cursor_position = Some((3, 4, false));
        app.data.ui.preview_pane_size = Some((54, 50));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let cursor = terminal.backend_mut().get_cursor_position()?;
        assert_eq!(cursor, Position { x: 28, y: 6 });
        Ok(())
    }

    #[test]
    fn test_render_preview_focused_sets_cursor_position_with_scroll()
    -> Result<(), Box<dyn std::error::Error>> {
        use ratatui::layout::Position;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(PreviewFocusedMode.into());
        app.data.ui.preview_content = (0..50)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.ui.preview_scroll = 16;
        app.data.ui.preview_cursor_position = Some((7, 5, false));
        app.data.ui.preview_pane_size = Some((54, 20));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let cursor = terminal.backend_mut().get_cursor_position()?;
        assert_eq!(cursor, Position { x: 32, y: 21 });
        Ok(())
    }

    #[test]
    fn test_render_diff_with_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.switch_tab();
        app.data.ui.set_diff_content(
            (0..100)
                .map(|i| format!("+Added line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        app.data.ui.diff_scroll = 50;

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
        let app = App::new(
            create_test_config(),
            Storage::new(),
            crate::app::Settings::default(),
            false,
        );

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_waiting_indicator_renders_unseen_waiting_half_moon()
    -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        let waiting_id = app
            .data
            .storage
            .iter()
            .find(|agent| agent.title == "agent-1")
            .map(|agent| agent.id)
            .ok_or("missing agent-1")?;

        app.data.ui.observe_agent_pane_digest(waiting_id, 123);
        app.data.ui.observe_agent_pane_digest(waiting_id, 123);

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for cell in &buffer.content {
            text.push_str(cell.symbol());
        }

        assert!(text.contains("◐"));
        Ok(())
    }

    #[test]
    fn test_render_waiting_indicator_renders_seen_waiting_circle()
    -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        let waiting_id = app
            .data
            .storage
            .iter()
            .find(|agent| agent.title == "agent-1")
            .map(|agent| agent.id)
            .ok_or("missing agent-1")?;

        app.data.ui.observe_agent_pane_digest(waiting_id, 123);
        app.data.ui.observe_agent_pane_digest(waiting_id, 123);
        app.data.ui.mark_agent_pane_seen(waiting_id);

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for cell in &buffer.content {
            text.push_str(cell.symbol());
        }

        assert!(text.contains("○"));
        Ok(())
    }

    #[test]
    fn test_render_agent_list_scrollbar_and_hierarchy_indicators()
    -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend)?;

        let config = create_test_config();
        let mut storage = Storage::new();

        let mut expanded_root = create_test_agent("expanded-root", Status::Running);
        expanded_root.collapsed = false;
        let expanded_root_id = expanded_root.id;
        let expanded_root_mux_session = expanded_root.mux_session.clone();
        storage.add(expanded_root);

        storage.add(Agent::new_child(
            "child-visible".to_string(),
            "echo".to_string(),
            "test".to_string(),
            PathBuf::from("/tmp/tenex-render-test-child-visible"),
            crate::agent::ChildConfig {
                parent_id: expanded_root_id,
                mux_session: expanded_root_mux_session,
                window_index: 1,
            },
        ));

        let collapsed_root = create_test_agent("collapsed-root", Status::Running);
        let collapsed_root_id = collapsed_root.id;
        let collapsed_root_mux_session = collapsed_root.mux_session.clone();
        storage.add(collapsed_root);

        storage.add(Agent::new_child(
            "child-hidden".to_string(),
            "echo".to_string(),
            "test".to_string(),
            PathBuf::from("/tmp/tenex-render-test-child-hidden"),
            crate::agent::ChildConfig {
                parent_id: collapsed_root_id,
                mux_session: collapsed_root_mux_session,
                window_index: 1,
            },
        ));

        for i in 0..30 {
            storage.add(create_test_agent(&format!("agent-{i:02}"), Status::Running));
        }

        let mut app = App::new(config, storage, crate::app::Settings::default(), false);
        app.data.ui.agent_list_scroll = 0;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for cell in &buffer.content {
            text.push_str(cell.symbol());
        }

        assert!(text.contains('░'));
        assert!(text.contains('▼'));
        assert!(text.contains('▶'));
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
        app.enter_mode(ScrollingMode.into());

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
        app.data.ui.preview_content = "Line 1\nLine 2".to_string();
        // Set scroll position beyond content length
        app.data.ui.preview_scroll = 1000;

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
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));

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

    #[test]
    fn test_render_worktree_conflict_overlay() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::WorktreeConflictInfo;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set up worktree conflict info
        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "test-agent".to_string(),
            prompt: Some("test prompt".to_string()),
            branch: "tenex/test-agent".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/worktrees/test-agent"),
            existing_branch: Some("tenex/test-agent".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        });
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_worktree_conflict_overlay_swarm() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::WorktreeConflictInfo;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set up worktree conflict info for a swarm
        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "swarm-root".to_string(),
            prompt: Some("swarm task".to_string()),
            branch: "tenex/swarm-root".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/worktrees/swarm-root"),
            existing_branch: Some("tenex/swarm-root".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: Some(3),
        });
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            }
            .into(),
        );

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_reconnect_prompt_mode() -> Result<(), Box<dyn std::error::Error>> {
        use crate::app::WorktreeConflictInfo;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set up for reconnect prompt mode
        app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
            title: "test-agent".to_string(),
            prompt: Some("original prompt".to_string()),
            branch: "tenex/test-agent".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/worktrees/test-agent"),
            existing_branch: Some("tenex/test-agent".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        });
        app.enter_mode(ReconnectPromptMode.into());
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

    fn create_test_branch_info(name: &str, is_remote: bool) -> crate::git::BranchInfo {
        crate::git::BranchInfo {
            name: name.to_string(),
            full_name: if is_remote {
                format!("refs/remotes/origin/{name}")
            } else {
                format!("refs/heads/{name}")
            },
            is_remote,
            remote: if is_remote {
                Some("origin".to_string())
            } else {
                None
            },
            last_commit_time: None,
        }
    }

    #[test]
    fn test_render_review_info_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(ReviewInfoMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_review_child_count_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.data.spawn.child_count = 5;
        app.enter_mode(ReviewChildCountMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set up some branches
        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("develop", false),
            create_test_branch_info("main", true),
        ];
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_rebase_branch_selector_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.data.git_op.branch_name = "feature/rebase-me".to_string();
        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("develop", false),
            create_test_branch_info("main", true),
        ];
        app.enter_mode(RebaseBranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_merge_branch_selector_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.data.git_op.branch_name = "feature/merge-me".to_string();
        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("main", true),
        ];
        app.enter_mode(MergeBranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_with_filter() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set up some branches and a filter
        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature-abc", false),
            create_test_branch_info("feature-xyz", false),
        ];
        app.data.review.filter = "feature".to_string();
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_with_selection() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set up branches with a selection
        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("develop", false),
        ];
        app.data.review.selected = 1;
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_empty() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Empty branch list
        app.data.review.branches = vec![];
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_scrolled() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Create many branches to trigger scrolling
        let mut branches = Vec::new();
        for i in 0..30 {
            branches.push(create_test_branch_info(&format!("branch-{i:02}"), false));
        }
        app.data.review.branches = branches;
        app.data.review.selected = 20; // Select one that requires scrolling
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_scroll_indicator_below_only()
    -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        let branches = (0..12)
            .map(|i| create_test_branch_info(&format!("branch-{i:02}"), false))
            .collect::<Vec<_>>();
        app.data.review.branches = branches;
        app.data.review.selected = 0;
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_scroll_indicator_above_only()
    -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        let branches = (0..12)
            .map(|i| create_test_branch_info(&format!("branch-{i:02}"), false))
            .collect::<Vec<_>>();
        app.data.review.branches = branches;
        app.data.review.selected = 11;
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        assert!(!terminal.backend().buffer().content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_branch_selector_mixed_local_remote() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Mix of local and remote branches
        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("main", true),
            create_test_branch_info("develop", true),
        ];
        app.enter_mode(BranchSelectorMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_child_count_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.data.spawn.child_count = 5;
        app.enter_mode(ChildCountMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_child_prompt_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.data.spawn.child_count = 3;
        app.enter_mode(ChildPromptMode.into());
        app.handle_char('t');
        app.handle_char('a');
        app.handle_char('s');
        app.handle_char('k');

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_broadcasting_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();
        app.enter_mode(BroadcastingMode.into());
        app.handle_char('m');
        app.handle_char('s');
        app.handle_char('g');

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    // === Push Feature Render Tests ===

    #[test]
    fn test_render_confirm_push_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Get first agent's ID
        let agent_id = app.data.storage.visible_agent_at(0).map(|a| a.id);
        app.data.git_op.agent_id = agent_id;
        app.data.git_op.branch_name = "feature/test".to_string();
        app.enter_mode(ConfirmPushMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirm_push_mode_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        // Set invalid agent ID
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();
        app.enter_mode(ConfirmPushMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_rename_root_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.data.git_op.original_branch = "old-name".to_string();
        app.data.input.buffer = "new-name".to_string();
        app.data.git_op.is_root_rename = true;
        app.enter_mode(RenameBranchMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_rename_subagent_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.data.git_op.original_branch = "sub-agent".to_string();
        app.data.input.buffer = "new-name".to_string();
        app.data.git_op.is_root_rename = false;
        app.enter_mode(RenameBranchMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_rename_empty_input() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.data.git_op.original_branch = "test-agent".to_string();
        app.data.input.buffer.clear();
        app.enter_mode(RenameBranchMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_confirm_push_for_pr_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.data.git_op.branch_name = "feature/new-branch".to_string();
        app.data.git_op.base_branch = "main".to_string();
        app.data.git_op.has_unpushed = true;
        app.enter_mode(ConfirmPushForPRMode.into());

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_command_palette_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_command_palette();
        assert_eq!(app.mode, AppMode::CommandPalette(CommandPaletteMode));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_command_palette_with_filter() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_command_palette();
        app.handle_char('m');
        app.handle_char('o');

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_command_palette_empty_filter() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_command_palette();
        app.data.input.buffer = "/xyz".to_string();
        app.reset_slash_command_selection();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_model_selector_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_model_selector();
        assert_eq!(app.mode, AppMode::ModelSelector(ModelSelectorMode));

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_model_selector_with_filter() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_model_selector();
        app.handle_model_filter_char('c');
        app.handle_model_filter_char('l');

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_model_selector_empty_filter() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_model_selector();
        app.data.model_selector.filter = "xyz".to_string();

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_settings_menu_mode() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.enter_mode(SettingsMenuMode.into());
        app.data.settings_menu.selected = 2;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_custom_agent_command_mode_for_roles() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.enter_mode(CustomAgentCommandMode.into());
        app.data.input.buffer = "my-agent".to_string();
        app.data.input.cursor = app.data.input.buffer.len();

        for role in [
            crate::app::AgentRole::Default,
            crate::app::AgentRole::Planner,
            crate::app::AgentRole::Review,
        ] {
            app.data.model_selector.role = role;
            terminal.draw(|frame| {
                render(frame, &app);
            })?;

            let buffer = terminal.backend().buffer();
            assert!(!buffer.content.is_empty());
        }

        Ok(())
    }

    #[test]
    fn test_render_model_selector_mode_planner() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_model_selector();
        app.data.model_selector.role = crate::app::AgentRole::Planner;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }

    #[test]
    fn test_render_model_selector_mode_review() -> Result<(), Box<dyn std::error::Error>> {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend)?;
        let mut app = create_test_app_with_agents();

        app.start_model_selector();
        app.data.model_selector.role = crate::app::AgentRole::Review;

        terminal.draw(|frame| {
            render(frame, &app);
        })?;

        let buffer = terminal.backend().buffer();
        assert!(!buffer.content.is_empty());
        Ok(())
    }
}
