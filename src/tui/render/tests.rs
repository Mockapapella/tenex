use super::*;
use crate::agent::{Agent, ChildConfig, Status, Storage, WorkspaceKind};
use crate::app::WorktreeConflictInfo;
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

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for cell in &buffer.content {
        text.push_str(cell.symbol());
    }
    text
}

fn buffer_text_rows(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let width: usize = buffer.area.width.into();
    let mut text = String::new();
    for (idx, cell) in buffer.content.iter().enumerate() {
        if idx > 0 && idx % width == 0 {
            text.push('\n');
        }
        text.push_str(cell.symbol());
    }
    text
}

fn render_to_terminal(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render(frame, app);
        })
        .unwrap();
    terminal
}

#[test]
fn test_render_agent_list_labels_plain_dir_agents() {
    let mut app = create_test_app_with_agents();
    let id = app
        .data
        .storage
        .iter()
        .find(|agent| agent.title == "agent-1")
        .expect("missing agent-1")
        .id;
    app.data.storage.get_mut(id).unwrap().workspace_kind = WorkspaceKind::PlainDir;

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("no-git"));
}

#[test]
fn test_render_changelog_mode() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(
        ChangelogMode {
            title: "What's New".to_string(),
            lines: vec!["Hello".to_string()],
            mark_seen_version: None,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("What's New"));
}

#[test]
fn test_render_reconnect_prompt_swarm_title() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(ReconnectPromptMode.into());
    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "agent".to_string(),
        prompt: None,
        branch: "branch".to_string(),
        worktree_path: PathBuf::from("/tmp"),
        repo_root: PathBuf::from("/tmp"),
        existing_branch: None,
        existing_commit: None,
        current_branch: "main".to_string(),
        current_commit: "deadbeef".to_string(),
        swarm_child_count: Some(3),
    });

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("Reconnect Swarm"));
}

#[test]
fn test_render_terminal_prompt_mode() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(TerminalPromptMode.into());
    app.data.input.buffer = "echo hi".to_string();
    app.data.input.cursor = app.data.input.buffer.len();

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("New Terminal"));
}

#[test]
fn test_render_synthesis_prompt_mode() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(SynthesisPromptMode.into());
    app.data.input.buffer = "extra".to_string();
    app.data.input.cursor = app.data.input.buffer.len();

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("Synthesize"));
}

#[test]
fn test_render_normal_mode() {
    let app = create_test_app_with_agents();

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_content_pane_has_top_border() {
    let width = 80_u16;
    let app = create_test_app_with_agents();

    let terminal = render_to_terminal(&app, width, 24);

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
}

#[test]
fn test_render_help_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(HelpMode.into());

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_creating_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(CreatingMode.into());
    app.handle_char('t');
    app.handle_char('e');
    app.handle_char('s');
    app.handle_char('t');

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_prompting_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(PromptingMode.into());
    app.handle_char('f');
    app.handle_char('i');
    app.handle_char('x');

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_prompting_mode_with_scrollbar() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(PromptingMode.into());
    app.data.input.buffer = (0..30)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.data.input.cursor = app.data.input.buffer.len();

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains('█'));
}

#[test]
fn test_render_confirming_kill_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_confirming_kill_mode_no_agent_selected() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let storage = Storage::new();
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("No agent selected"));
}

#[test]
fn test_render_confirming_kill_mode_warns_on_non_deleting_branch() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();
    let mut agent = create_test_agent("agent-1", Status::Running);
    agent.branch = "feature/example".to_string();
    storage.add(agent);
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("This will delete the worktree."));
}

#[test]
fn test_render_confirming_kill_mode_warns_on_tenex_branch_prefix() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();
    let mut agent = create_test_agent("agent-1", Status::Running);
    agent.branch = "tenex/example".to_string();
    storage.add(agent);
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("tenex/example"));
}

#[test]
fn test_render_confirming_kill_mode_warns_on_plain_dir_root() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();
    let mut agent = create_test_agent("agent-1", Status::Running);
    agent.workspace_kind = WorkspaceKind::PlainDir;
    storage.add(agent);
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("agent-1"));
}

#[test]
fn test_render_confirming_kill_mode_warns_on_child_agent() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();

    let mut root = create_test_agent("root", Status::Running);
    root.collapsed = false;
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_path = root.worktree_path.clone();
    storage.add(root);

    let child = Agent::new_child(
        "child".to_string(),
        "echo".to_string(),
        root_branch,
        root_path,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 1,
            repo_root: None,
        },
    );
    storage.add(child);

    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.data.selected = 2;
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("child"));
}

#[test]
fn test_render_confirming_interrupt_agent_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_confirming_interrupt_agent_mode_no_agent_selected() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let storage = Storage::new();
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("No agent selected"));
}

#[test]
fn test_render_confirming_synthesize_mode_no_agent_selected() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let storage = Storage::new();
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("No agent selected"));
}

#[test]
fn test_render_confirming_synthesize_mode_pluralizes_agents() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    let config = create_test_config();
    let mut storage = Storage::new();

    let root = create_test_agent("root", Status::Running);
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_path = root.worktree_path.clone();
    storage.add(root);

    let child_1 = Agent::new_child(
        "Child 1".to_string(),
        "echo".to_string(),
        root_branch.clone(),
        root_path.clone(),
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session.clone(),
            window_index: 1,
            repo_root: None,
        },
    );
    storage.add(child_1);

    let child_2 = Agent::new_child(
        "Child 2".to_string(),
        "echo".to_string(),
        root_branch,
        root_path,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 2,
            repo_root: None,
        },
    );
    storage.add(child_2);

    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Synthesize 2 agents?"));
}

#[test]
fn test_render_confirming_synthesize_mode_counts_marked_agents() {
    let config = create_test_config();
    let mut storage = Storage::new();

    let mut root = create_test_agent("root", Status::Running);
    root.collapsed = false;
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_path = root.worktree_path.clone();
    storage.add(root);

    let child_1 = Agent::new_child(
        "Child 1".to_string(),
        "echo".to_string(),
        root_branch.clone(),
        root_path.clone(),
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session.clone(),
            window_index: 1,
            repo_root: None,
        },
    );
    let child_1_id = child_1.id;
    storage.add(child_1);

    let grandchild_1 = Agent::new_child(
        "Grandchild 1".to_string(),
        "echo".to_string(),
        root_branch.clone(),
        root_path.clone(),
        ChildConfig {
            parent_id: child_1_id,
            mux_session: root_session.clone(),
            window_index: 3,
            repo_root: None,
        },
    );
    storage.add(grandchild_1);

    let child_2 = Agent::new_child(
        "Child 2".to_string(),
        "echo".to_string(),
        root_branch,
        root_path,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 2,
            repo_root: None,
        },
    );
    storage.add(child_2);

    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    assert!(app.data.toggle_synthesis_mark(child_1_id));
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("Synthesize 2 agents?"));
    assert!(text.contains("Marked descendant subtrees"));
}

#[test]
fn test_render_confirming_synthesize_mode_excludes_terminals() {
    let config = create_test_config();
    let mut storage = Storage::new();

    let mut root = create_test_agent("root", Status::Running);
    root.collapsed = false;
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_path = root.worktree_path.clone();
    storage.add(root);

    let child = Agent::new_child(
        "Child".to_string(),
        "echo".to_string(),
        root_branch.clone(),
        root_path.clone(),
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session.clone(),
            window_index: 1,
            repo_root: None,
        },
    );
    storage.add(child);

    let mut terminal_child = Agent::new_child(
        "Terminal 1".to_string(),
        "terminal".to_string(),
        root_branch,
        root_path,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 2,
            repo_root: None,
        },
    );
    terminal_child.is_terminal = true;
    storage.add(terminal_child);

    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("Synthesize 1 agent?"));
}

#[test]
fn test_render_confirming_synthesize_mode_only_terminals() {
    let config = create_test_config();
    let mut storage = Storage::new();

    let root = create_test_agent("root", Status::Running);
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_path = root.worktree_path.clone();
    storage.add(root);

    let mut terminal_child = Agent::new_child(
        "Terminal 1".to_string(),
        "terminal".to_string(),
        root_branch,
        root_path,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 1,
            repo_root: None,
        },
    );
    terminal_child.is_terminal = true;
    storage.add(terminal_child);

    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let text = buffer_text(&terminal);
    assert!(text.contains("No non-terminal"));
}

#[test]
fn test_render_confirming_reset_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Reset,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_confirming_quit_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Quit,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_confirming_restart_mux_daemon_mode() {
    let mut app = create_test_app_with_agents();
    app.data.ui.muxd_version_mismatch = Some(crate::app::MuxdVersionMismatchInfo {
        socket: "tenex-mux-test.sock".to_string(),
        daemon_version: "tenex-mux/0.0.0".to_string(),
        expected_version: "tenex-mux/0.0.1".to_string(),
        env_mux_socket: Some("tenex-mux-test.sock".to_string()),
    });
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::RestartMuxDaemon,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_confirming_restart_mux_daemon_mode_without_mismatch_info() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let storage = Storage::new();
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::RestartMuxDaemon,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Restart mux daemon?"));
}

#[test]
fn test_render_confirming_restart_mux_daemon_mode_without_env_socket() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();
    storage.add(create_test_agent("agent-1", Status::Running));
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.data.ui.muxd_version_mismatch = Some(crate::app::MuxdVersionMismatchInfo {
        socket: "tenex-mux-test.sock".to_string(),
        daemon_version: "tenex-mux/0.0.0".to_string(),
        expected_version: "tenex-mux/0.0.1".to_string(),
        env_mux_socket: None,
    });
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::RestartMuxDaemon,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Restart mux daemon?"));
}

#[test]
fn test_render_confirming_switch_branch_mode_with_target_branch() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();
    storage.add(create_test_agent("agent-1", Status::Running));
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.data.git_op.branch_name = "from-branch".to_string();
    app.data.git_op.target_branch = "to-branch".to_string();
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::SwitchBranch,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Switch Branch?"));
    assert!(text.contains("from-branch"));
    assert!(text.contains("to-branch"));
}

#[test]
fn test_render_confirming_switch_branch_mode_without_target_branch() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let config = create_test_config();
    let mut storage = Storage::new();
    storage.add(create_test_agent("agent-1", Status::Running));
    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.data.git_op.branch_name = "from-branch".to_string();
    app.data.git_op.target_branch = String::new();
    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::SwitchBranch,
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("<none selected>"));
}

#[test]
fn test_render_with_error() {
    let mut app = create_test_app_with_agents();
    app.set_error("Something went wrong!");

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_success_modal_mode() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = create_test_app_with_agents();
    app.enter_mode(
        SuccessModalMode {
            message: "Operation completed".to_string(),
        }
        .into(),
    );

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Success"));
    assert!(text.contains("Operation completed"));
}

#[test]
fn test_render_update_prompt_mode() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = create_test_app_with_agents();
    let info = crate::update::UpdateInfo {
        current_version: semver::Version::parse("1.0.0").unwrap(),
        latest_version: semver::Version::parse("1.0.1").unwrap(),
    };
    app.enter_mode(UpdatePromptMode { info }.into());

    terminal
        .draw(|frame| {
            render(frame, &app);
        })
        .unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Update Available"));
    assert!(text.contains("1.0.0"));
    assert!(text.contains("1.0.1"));
}

#[test]
fn test_render_with_status_message() {
    let mut app = create_test_app_with_agents();
    app.set_status("Operation completed");

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_status_bar_shows_error_when_not_in_modal() {
    let mut app = create_test_app_with_agents();
    app.data.ui.last_error = Some("boom".to_string());
    app.mode = AppMode::normal();

    let terminal = render_to_terminal(&app, 80, 24);
    let text = buffer_text(&terminal);
    assert!(text.contains("Error: boom"));
}

#[test]
fn test_render_diff_tab() {
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

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_preview_with_content() {
    let mut app = create_test_app_with_agents();
    app.data
        .ui
        .set_preview_content("Line 1\nLine 2\nLine 3\nLine 4\nLine 5");

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_preview_with_scroll() {
    let mut app = create_test_app_with_agents();
    app.data.ui.set_preview_content(
        (0..100)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.preview_scroll = 50;

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_preview_focused_sets_cursor_position_simple() {
    use ratatui::layout::Position;

    let mut app = create_test_app_with_agents();
    app.enter_mode(PreviewFocusedMode.into());
    app.data.ui.set_preview_content(
        (0..10)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.preview_cursor_position = Some((3, 4, false));
    app.data.ui.preview_pane_size = Some((54, 50));

    let mut terminal = render_to_terminal(&app, 80, 24);
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    assert_eq!(cursor, Position { x: 28, y: 6 });
}

#[test]
fn test_render_preview_focused_sets_cursor_position_with_scroll() {
    use ratatui::layout::Position;

    let mut app = create_test_app_with_agents();
    app.enter_mode(PreviewFocusedMode.into());
    app.data.ui.set_preview_content(
        (0..50)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.preview_scroll = 16;
    app.data.ui.preview_cursor_position = Some((7, 5, false));
    app.data.ui.preview_pane_size = Some((54, 20));

    let mut terminal = render_to_terminal(&app, 80, 24);
    let cursor = terminal.backend_mut().get_cursor_position().unwrap();
    assert_eq!(cursor, Position { x: 32, y: 21 });
}

#[test]
fn test_render_diff_with_scroll() {
    let mut app = create_test_app_with_agents();
    app.switch_tab();
    app.data.ui.set_diff_content(
        (0..100)
            .map(|i| format!("+Added line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.data.ui.diff_scroll = 50;

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_empty_agents() {
    let app = App::new(
        create_test_config(),
        Storage::new(),
        crate::app::Settings::default(),
        false,
    );

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_waiting_indicator_renders_unseen_waiting_half_moon() {
    let mut app = create_test_app_with_agents();

    let waiting_id = app
        .data
        .storage
        .iter()
        .find(|agent| agent.title == "agent-1")
        .map(|agent| agent.id)
        .expect("missing agent-1");

    app.data.ui.observe_agent_pane_digest(waiting_id, 123);
    app.data.ui.observe_agent_pane_digest(waiting_id, 123);

    let terminal = render_to_terminal(&app, 80, 24);
    let text = buffer_text(&terminal);
    assert!(text.contains("◐"));
}

#[test]
fn test_render_waiting_indicator_renders_seen_waiting_circle() {
    let mut app = create_test_app_with_agents();

    let waiting_id = app
        .data
        .storage
        .iter()
        .find(|agent| agent.title == "agent-1")
        .map(|agent| agent.id)
        .expect("missing agent-1");

    app.data.ui.observe_agent_pane_digest(waiting_id, 123);
    app.data.ui.observe_agent_pane_digest(waiting_id, 123);
    app.data.ui.mark_agent_pane_seen(waiting_id);

    let terminal = render_to_terminal(&app, 80, 24);
    let text = buffer_text(&terminal);
    assert!(text.contains("○"));
}

#[test]
fn test_render_agent_list_scrollbar_and_hierarchy_indicators() {
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
            repo_root: None,
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
            repo_root: None,
        },
    ));

    for i in 0..30 {
        storage.add(create_test_agent(
            &format!("zz-agent-{i:02}"),
            Status::Running,
        ));
    }

    let mut app = App::new(config, storage, crate::app::Settings::default(), false);
    app.data.ui.agent_list_scroll = 0;

    let terminal = render_to_terminal(&app, 80, 12);
    let text = buffer_text(&terminal);
    assert!(text.contains('░'));
    assert!(text.contains('▼'));
    assert!(text.contains('▶'));
}

#[test]
fn test_render_with_selection() {
    let mut app = create_test_app_with_agents();
    app.select_next();
    app.select_next();

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_various_sizes() {
    for (width, height) in [(40, 12), (80, 24), (120, 40), (200, 50)] {
        let app = create_test_app_with_agents();
        let terminal = render_to_terminal(&app, width, height);
        assert!(!terminal.backend().buffer().content.is_empty());
    }
}

#[test]
fn test_render_scrolling_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(ScrollingMode.into());

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_scroll_exceeds_content() {
    let mut app = create_test_app_with_agents();
    app.data.ui.set_preview_content("Line 1\nLine 2");
    // Set scroll position beyond content length
    app.data.ui.preview_scroll = 1000;

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_error_modal() {
    let mut app = create_test_app_with_agents();
    app.set_error("Something went wrong!");

    let normal_app = create_test_app_with_agents();
    for (candidate, expected) in [(&app, true), (&normal_app, false)] {
        assert_eq!(matches!(&candidate.mode, AppMode::ErrorModal(_)), expected);
    }

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_error_modal_long_message() {
    let mut app = create_test_app_with_agents();
    app.set_error("This is a very long error message that should wrap to multiple lines in the error modal to ensure the word wrapping functionality works correctly");

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
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
fn test_render_worktree_conflict_overlay() {
    use crate::app::WorktreeConflictInfo;

    let mut app = create_test_app_with_agents();

    // Set up worktree conflict info
    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "test-agent".to_string(),
        prompt: Some("test prompt".to_string()),
        branch: "tenex/test-agent".to_string(),
        worktree_path: std::path::PathBuf::from("/tmp/worktrees/test-agent"),
        repo_root: std::path::PathBuf::from("/tmp"),
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

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_worktree_conflict_overlay_noops_without_conflict_info() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::WorktreeConflict,
        }
        .into(),
    );

    let terminal = render_to_terminal(&app, 80, 24);
    let text = buffer_text(&terminal);
    assert!(!text.contains("Worktree Already Exists"));
}

#[test]
fn test_render_worktree_conflict_overlay_swarm() {
    use crate::app::WorktreeConflictInfo;

    let mut app = create_test_app_with_agents();

    // Set up worktree conflict info for a swarm
    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "swarm-root".to_string(),
        prompt: Some("swarm task".to_string()),
        branch: "tenex/swarm-root".to_string(),
        worktree_path: std::path::PathBuf::from("/tmp/worktrees/swarm-root"),
        repo_root: std::path::PathBuf::from("/tmp"),
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

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_reconnect_prompt_mode() {
    use crate::app::WorktreeConflictInfo;

    let mut app = create_test_app_with_agents();

    // Set up for reconnect prompt mode
    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "test-agent".to_string(),
        prompt: Some("original prompt".to_string()),
        branch: "tenex/test-agent".to_string(),
        worktree_path: std::path::PathBuf::from("/tmp/worktrees/test-agent"),
        repo_root: std::path::PathBuf::from("/tmp"),
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

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
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
fn test_render_review_info_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(ReviewInfoMode.into());

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_review_child_count_mode() {
    let mut app = create_test_app_with_agents();
    app.data.spawn.child_count = 5;
    app.enter_mode(ReviewChildCountMode.into());

    let terminal = render_to_terminal(&app, 80, 24);

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
}

#[test]
fn test_render_branch_selector_mode() {
    let mut app = create_test_app_with_agents();

    // Set up some branches
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("develop", false),
        create_test_branch_info("main", true),
    ];
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_rebase_branch_selector_mode() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.branch_name = "feature/rebase-me".to_string();
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("develop", false),
        create_test_branch_info("main", true),
    ];
    app.enter_mode(RebaseBranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_merge_branch_selector_mode() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.branch_name = "feature/merge-me".to_string();
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("main", true),
    ];
    app.enter_mode(MergeBranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_switch_branch_selector_mode() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.branch_name = "feature/switch-me".to_string();
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("develop", false),
        create_test_branch_info("main", true),
    ];
    app.enter_mode(SwitchBranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_with_filter() {
    let mut app = create_test_app_with_agents();

    // Set up some branches and a filter
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature-abc", false),
        create_test_branch_info("feature-xyz", false),
    ];
    app.data.review.filter = "feature".to_string();
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_with_selection() {
    let mut app = create_test_app_with_agents();

    // Set up branches with a selection
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("develop", false),
    ];
    app.data.review.selected = 1;
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_empty() {
    let mut app = create_test_app_with_agents();

    // Empty branch list
    app.data.review.branches = vec![];
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_scrolled() {
    let mut app = create_test_app_with_agents();

    // Create many branches to trigger scrolling
    let mut branches = Vec::new();
    for i in 0..30 {
        branches.push(create_test_branch_info(&format!("branch-{i:02}"), false));
    }
    app.data.review.branches = branches;
    app.data.review.selected = 20; // Select one that requires scrolling
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_scrolled_skips_remote_branches_above_offset() {
    let mut app = create_test_app_with_agents();

    let mut branches = Vec::new();
    for i in 0..5 {
        branches.push(create_test_branch_info(&format!("local-{i:02}"), false));
    }
    for i in 0..20 {
        branches.push(create_test_branch_info(&format!("remote-{i:02}"), true));
    }

    app.data.review.branches = branches;
    app.data.review.selected = 18;
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_scroll_indicator_below_only() {
    let mut app = create_test_app_with_agents();

    let branches = (0..12)
        .map(|i| create_test_branch_info(&format!("branch-{i:02}"), false))
        .collect::<Vec<_>>();
    app.data.review.branches = branches;
    app.data.review.selected = 0;
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_scroll_indicator_above_only() {
    let mut app = create_test_app_with_agents();

    let branches = (0..12)
        .map(|i| create_test_branch_info(&format!("branch-{i:02}"), false))
        .collect::<Vec<_>>();
    app.data.review.branches = branches;
    app.data.review.selected = 11;
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_branch_selector_mixed_local_remote() {
    let mut app = create_test_app_with_agents();

    // Mix of local and remote branches
    let remote_without_prefix = crate::git::BranchInfo {
        name: "no-remote-prefix".to_string(),
        full_name: "refs/remotes/no-remote-prefix".to_string(),
        is_remote: true,
        remote: None,
        last_commit_time: None,
    };
    app.data.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("main", true),
        create_test_branch_info("develop", true),
        remote_without_prefix,
    ];
    app.enter_mode(BranchSelectorMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_child_count_mode() {
    let mut app = create_test_app_with_agents();
    app.data.spawn.child_count = 5;
    app.enter_mode(ChildCountMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_child_count_mode_shows_subagent_context_when_spawning_under() {
    let mut app = create_test_app_with_agents();

    let parent_id = app.data.storage.iter().next().expect("missing agent").id;
    app.data.spawn.start_spawning_under(parent_id);
    app.data.spawn.child_count = 2;
    app.enter_mode(ChildCountMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    let text = buffer_text_rows(&terminal);
    assert!(text.contains("Spawn sub-agents for selected"));
    assert!(!text.contains("Spawn new root"));
}

#[test]
fn test_render_child_prompt_mode() {
    let mut app = create_test_app_with_agents();
    app.data.spawn.child_count = 3;
    app.enter_mode(ChildPromptMode.into());
    app.handle_char('t');
    app.handle_char('a');
    app.handle_char('s');
    app.handle_char('k');

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_broadcasting_mode() {
    let mut app = create_test_app_with_agents();
    app.enter_mode(BroadcastingMode.into());
    app.handle_char('m');
    app.handle_char('s');
    app.handle_char('g');

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

// === Push Feature Render Tests ===

#[test]
fn test_render_confirm_push_mode() {
    let mut app = create_test_app_with_agents();

    // Get first agent's ID
    let agent_id = app.data.storage.iter().next().map(|a| a.id);
    app.data.git_op.agent_id = agent_id;
    app.data.git_op.branch_name = "feature/test".to_string();
    app.enter_mode(ConfirmPushMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_confirm_push_mode_no_agent() {
    let mut app = create_test_app_with_agents();

    // Set invalid agent ID
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "test".to_string();
    app.enter_mode(ConfirmPushMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_rename_root_mode() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.original_branch = "old-name".to_string();
    app.data.input.buffer = "new-name".to_string();
    app.data.git_op.is_root_rename = true;
    app.enter_mode(RenameBranchMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_rename_subagent_mode() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.original_branch = "sub-agent".to_string();
    app.data.input.buffer = "new-name".to_string();
    app.data.git_op.is_root_rename = false;
    app.enter_mode(RenameBranchMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_rename_empty_input() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.original_branch = "test-agent".to_string();
    app.data.input.buffer.clear();
    app.enter_mode(RenameBranchMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_confirm_push_for_pr_mode() {
    let mut app = create_test_app_with_agents();

    app.data.git_op.branch_name = "feature/new-branch".to_string();
    app.data.git_op.base_branch = "main".to_string();
    app.data.git_op.has_unpushed = true;
    app.enter_mode(ConfirmPushForPRMode.into());

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_command_palette_mode() {
    let mut app = create_test_app_with_agents();

    app.start_command_palette();
    assert_eq!(app.mode, AppMode::CommandPalette(CommandPaletteMode));

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_command_palette_with_filter() {
    let mut app = create_test_app_with_agents();

    app.start_command_palette();
    app.handle_char('m');
    app.handle_char('o');

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_command_palette_empty_filter() {
    let mut app = create_test_app_with_agents();

    app.start_command_palette();
    app.data.input.buffer = "/xyz".to_string();
    app.reset_slash_command_selection();

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_model_selector_mode() {
    let mut app = create_test_app_with_agents();

    app.start_model_selector();
    assert_eq!(app.mode, AppMode::ModelSelector(ModelSelectorMode));

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_model_selector_with_filter() {
    let mut app = create_test_app_with_agents();

    app.start_model_selector();
    app.handle_model_filter_char('c');
    app.handle_model_filter_char('l');

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_model_selector_empty_filter() {
    let mut app = create_test_app_with_agents();

    app.start_model_selector();
    app.data.model_selector.filter = "xyz".to_string();

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_settings_menu_mode() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(SettingsMenuMode.into());
    app.data.settings_menu.selected = 2;

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_custom_agent_command_mode_for_roles() {
    let mut app = create_test_app_with_agents();

    app.enter_mode(CustomAgentCommandMode.into());
    app.data.input.buffer = "my-agent".to_string();
    app.data.input.cursor = app.data.input.buffer.len();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    for role in [
        crate::app::AgentRole::Default,
        crate::app::AgentRole::Planner,
        crate::app::AgentRole::Review,
    ] {
        app.data.model_selector.role = role;
        terminal
            .draw(|frame| {
                render(frame, &app);
            })
            .unwrap();
        assert!(!terminal.backend().buffer().content.is_empty());
    }
}

#[test]
fn test_render_model_selector_mode_planner() {
    let mut app = create_test_app_with_agents();

    app.start_model_selector();
    app.data.model_selector.role = crate::app::AgentRole::Planner;

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_model_selector_mode_review() {
    let mut app = create_test_app_with_agents();

    app.start_model_selector();
    app.data.model_selector.role = crate::app::AgentRole::Review;

    let terminal = render_to_terminal(&app, 80, 24);
    assert!(!terminal.backend().buffer().content.is_empty());
}
