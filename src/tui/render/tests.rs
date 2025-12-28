use super::*;
use crate::agent::{Agent, Status, Storage};
use crate::app::ConfirmAction;
use crate::config::Config;
use ratatui::Terminal;
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

    assert_eq!(top_left, Some("┌"));
    assert_eq!(top_right, Some("┐"));
    Ok(())
}

#[test]
fn test_render_help_mode() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();
    app.enter_mode(Mode::Overlay(OverlayMode::Help));

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
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::Creating,
    )));
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
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::Prompting,
    )));
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
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
        ConfirmAction::Kill,
    ))));

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
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
        ConfirmAction::Reset,
    ))));

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
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
        ConfirmAction::Quit,
    ))));

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
    assert_eq!(app.active_tab, crate::app::Tab::Diff);

    // Set diff content with various line types
    app.ui.set_diff_content(
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
    app.ui.preview_content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string();

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
    app.ui.preview_content = (0..100)
        .map(|i| format!("Line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.ui.preview_scroll = 50;

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
    app.ui.set_diff_content(
        (0..100)
            .map(|i| format!("+Added line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    app.ui.diff_scroll = 50;

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
    app.ui.preview_content = "Line 1\nLine 2".to_string();
    // Set scroll position beyond content length
    app.ui.preview_scroll = 1000;

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
    assert!(matches!(app.mode, Mode::Overlay(OverlayMode::Error(_))));

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
    app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
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
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
        ConfirmAction::WorktreeConflict,
    ))));

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
    app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
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
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
        ConfirmAction::WorktreeConflict,
    ))));

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
    app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
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
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::ReconnectPrompt,
    )));
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
    app.enter_mode(Mode::Overlay(OverlayMode::ReviewInfo));

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
    app.spawn.child_count = 5;
    app.enter_mode(Mode::Overlay(OverlayMode::CountPicker(
        CountPickerKind::ReviewChildCount,
    )));

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
    app.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("develop", false),
        create_test_branch_info("main", true),
    ];
    app.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    )));

    terminal.draw(|frame| {
        render(frame, &app);
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
    Ok(())
}

#[test]
fn test_render_branch_selector_with_filter() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    // Set up some branches and a filter
    app.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature-abc", false),
        create_test_branch_info("feature-xyz", false),
    ];
    app.review.filter = "feature".to_string();
    app.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    )));

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
    app.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("develop", false),
    ];
    app.review.selected = 1;
    app.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    )));

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
    app.review.branches = vec![];
    app.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    )));

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
    app.review.branches = branches;
    app.review.selected = 20; // Select one that requires scrolling
    app.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    )));

    terminal.draw(|frame| {
        render(frame, &app);
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
    Ok(())
}

#[test]
fn test_render_branch_selector_mixed_local_remote() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    // Mix of local and remote branches
    app.review.branches = vec![
        create_test_branch_info("main", false),
        create_test_branch_info("feature", false),
        create_test_branch_info("main", true),
        create_test_branch_info("develop", true),
    ];
    app.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    )));

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
    app.spawn.child_count = 5;
    app.enter_mode(Mode::Overlay(OverlayMode::CountPicker(
        CountPickerKind::ChildCount,
    )));

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
    app.spawn.child_count = 3;
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::ChildPrompt,
    )));
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
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::Broadcasting,
    )));
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
    let agent_id = app.storage.visible_agent_at(0).map(|a| a.id);
    app.git_op.agent_id = agent_id;
    app.git_op.branch_name = "feature/test".to_string();
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Push)));

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
    app.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.git_op.branch_name = "test".to_string();
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Push)));

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

    app.git_op.original_branch = "old-name".to_string();
    app.input.buffer = "new-name".to_string();
    app.git_op.is_root_rename = true;
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::RenameBranch,
    )));

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

    app.git_op.original_branch = "sub-agent".to_string();
    app.input.buffer = "new-name".to_string();
    app.git_op.is_root_rename = false;
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::RenameBranch,
    )));

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

    app.git_op.original_branch = "test-agent".to_string();
    app.input.buffer.clear();
    app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
        TextInputKind::RenameBranch,
    )));

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

    app.git_op.branch_name = "feature/new-branch".to_string();
    app.git_op.base_branch = "main".to_string();
    app.git_op.has_unpushed = true;
    app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::PushForPR)));

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
    assert_eq!(app.mode, Mode::Overlay(OverlayMode::CommandPalette));

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
    app.input.buffer = "/xyz".to_string();
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
    assert_eq!(app.mode, Mode::Overlay(OverlayMode::ModelSelector));

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
    app.model_selector.filter = "xyz".to_string();

    terminal.draw(|frame| {
        render(frame, &app);
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());
    Ok(())
}
