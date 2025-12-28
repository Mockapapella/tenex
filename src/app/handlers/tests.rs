use super::*;
use crate::agent::{Status, Storage};
use crate::app::CountPickerKind;
use crate::app::Settings;
use crate::config::Config;
use tempfile::NamedTempFile;

fn text_input_mode(kind: TextInputKind) -> Mode {
    Mode::Overlay(OverlayMode::TextInput(kind))
}

fn confirm_action_mode(action: ConfirmAction) -> Mode {
    Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(action)))
}

fn count_picker_mode(kind: CountPickerKind) -> Mode {
    Mode::Overlay(OverlayMode::CountPicker(kind))
}

fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
    let temp_file = NamedTempFile::new()?;
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    Ok((
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    ))
}

#[test]
fn test_handler_new() {
    let handler = Actions::new();
    assert!(!format!("{:?}", handler.session_manager).is_empty());
}

#[test]
fn test_handler_default() {
    let handler = Actions::default();
    assert!(!format!("{:?}", handler.output_capture).is_empty());
}

#[test]
fn test_handle_action_new_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::NewAgent)?;
    assert_eq!(app.mode, text_input_mode(TextInputKind::Creating));
    Ok(())
}

#[test]
fn test_handle_action_new_agent_with_prompt() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::NewAgentWithPrompt)?;
    assert_eq!(app.mode, text_input_mode(TextInputKind::Prompting));
    Ok(())
}

#[test]
fn test_handle_action_help() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::Help)?;
    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    Ok(())
}

#[test]
fn test_handle_action_quit_no_agents() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::Quit)?;
    assert!(app.should_quit);
    Ok(())
}

#[test]
fn test_handle_action_switch_tab() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::SwitchTab)?;
    assert_eq!(app.active_tab, super::super::state::Tab::Diff);
    Ok(())
}

#[test]
fn test_handle_action_navigation() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    for i in 0..3 {
        app.storage.add(Agent::new(
            format!("agent{i}"),
            "claude".to_string(),
            format!("muster/agent{i}"),
            PathBuf::from("/tmp"),
            None,
        ));
    }

    assert_eq!(app.selected, 0);
    handler.handle_action(&mut app, Action::NextAgent)?;
    assert_eq!(app.selected, 1);
    handler.handle_action(&mut app, Action::PrevAgent)?;
    assert_eq!(app.selected, 0);
    Ok(())
}

#[test]
fn test_handle_action_scroll() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::ScrollDown)?;
    assert_eq!(app.ui.preview_scroll, 5);

    handler.handle_action(&mut app, Action::ScrollUp)?;
    assert_eq!(app.ui.preview_scroll, 0);

    handler.handle_action(&mut app, Action::ScrollTop)?;
    assert_eq!(app.ui.preview_scroll, 0);
    Ok(())
}

#[test]
fn test_handle_action_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    app.enter_mode(text_input_mode(TextInputKind::Creating));
    handler.handle_action(&mut app, Action::Cancel)?;
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_kill_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::Kill)?;
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_focus_preview_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // FocusPreview does nothing when no agent is selected (stays in Normal mode)
    let result = handler.handle_action(&mut app, Action::FocusPreview);
    assert!(result.is_ok());
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_quit_with_running_agents() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add a running agent
    let mut agent = Agent::new(
        "running".to_string(),
        "claude".to_string(),
        "muster/running".to_string(),
        PathBuf::from("/tmp"),
        None,
    );
    agent.set_status(Status::Running);
    app.storage.add(agent);

    // Quit should enter confirming mode
    handler.handle_action(&mut app, Action::Quit)?;
    assert_eq!(app.mode, confirm_action_mode(ConfirmAction::Quit));
    Ok(())
}

#[test]
fn test_handle_kill_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "muster/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // Kill should enter confirming mode
    handler.handle_action(&mut app, Action::Kill)?;
    assert_eq!(app.mode, confirm_action_mode(ConfirmAction::Kill));
    Ok(())
}

#[test]
fn test_handle_confirm_quit() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Enter confirming mode for quit
    app.enter_mode(confirm_action_mode(ConfirmAction::Quit));

    handler.handle_action(&mut app, Action::Confirm)?;
    assert!(app.should_quit);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_confirm_reset() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add agents
    for i in 0..3 {
        app.storage.add(Agent::new(
            format!("agent{i}"),
            "claude".to_string(),
            format!("muster/agent{i}"),
            PathBuf::from("/tmp"),
            None,
        ));
    }

    // Enter confirming mode for reset
    app.enter_mode(confirm_action_mode(ConfirmAction::Reset));

    handler.handle_action(&mut app, Action::Confirm)?;
    assert_eq!(app.storage.len(), 0);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_focus_preview_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "test-session".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // FocusPreview should enter PreviewFocused mode
    let result = handler.handle_action(&mut app, Action::FocusPreview);
    assert!(result.is_ok());
    assert_eq!(app.mode, Mode::PreviewFocused);

    // UnfocusPreview should exit to Normal mode
    let result = handler.handle_action(&mut app, Action::UnfocusPreview);
    assert!(result.is_ok());
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_toggle_collapse_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Should not error with no agent selected
    handler.toggle_collapse(&mut app)?;
    Ok(())
}

#[test]
fn test_toggle_collapse_no_children() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "muster/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // Should not error when agent has no children
    handler.toggle_collapse(&mut app)?;
    Ok(())
}

#[test]
fn test_handle_action_spawn_children() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::SpawnChildren)?;
    assert_eq!(app.mode, count_picker_mode(CountPickerKind::ChildCount));
    assert!(app.spawn.spawning_under.is_none());
    Ok(())
}

#[test]
fn test_handle_action_add_children() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    let agent = Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "muster/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    );
    let agent_id = agent.id;
    app.storage.add(agent);

    handler.handle_action(&mut app, Action::AddChildren)?;
    assert_eq!(app.mode, count_picker_mode(CountPickerKind::ChildCount));
    assert_eq!(app.spawn.spawning_under, Some(agent_id));
    Ok(())
}

#[test]
fn test_handle_action_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // No agent - should not enter confirming mode
    handler.handle_action(&mut app, Action::Synthesize)?;
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_action_synthesize_with_children() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add parent agent
    let parent = Agent::new(
        "parent".to_string(),
        "claude".to_string(),
        "tenex/parent".to_string(),
        PathBuf::from("/tmp"),
        None,
    );
    let parent_id = parent.id;
    app.storage.add(parent);

    // Add child agent
    let mut child = Agent::new(
        "child".to_string(),
        "claude".to_string(),
        "tenex/child".to_string(),
        PathBuf::from("/tmp"),
        None,
    );
    child.parent_id = Some(parent_id);
    app.storage.add(child);

    // With children - should enter confirming mode
    handler.handle_action(&mut app, Action::Synthesize)?;
    assert_eq!(app.mode, confirm_action_mode(ConfirmAction::Synthesize));
    Ok(())
}

#[test]
fn test_handle_action_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add agent with no children
    app.storage.add(Agent::new(
        "parent".to_string(),
        "claude".to_string(),
        "tenex/parent".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // No children - should show error modal, not enter confirming mode
    handler.handle_action(&mut app, Action::Synthesize)?;
    assert!(matches!(app.mode, Mode::Overlay(OverlayMode::Error(_))));
    Ok(())
}

#[test]
fn test_handle_action_toggle_collapse() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // No agent - should not error
    handler.handle_action(&mut app, Action::ToggleCollapse)?;
    Ok(())
}

#[test]
fn test_handle_action_broadcast_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // No agent - should not enter mode
    handler.handle_action(&mut app, Action::Broadcast)?;
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_action_broadcast_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "muster/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    handler.handle_action(&mut app, Action::Broadcast)?;
    assert_eq!(app.mode, text_input_mode(TextInputKind::Broadcasting));
    Ok(())
}

#[test]
fn test_handle_scroll_bottom() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    handler.handle_action(&mut app, Action::ScrollBottom)?;
    // ScrollBottom calls scroll_to_bottom(10000, 0) so preview_scroll becomes 10000
    assert_eq!(app.ui.preview_scroll, 10000);
    Ok(())
}

#[test]
fn test_handle_action_review_swarm_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // No agent - should show ReviewInfo
    handler.handle_action(&mut app, Action::ReviewSwarm)?;
    assert_eq!(app.mode, Mode::Overlay(OverlayMode::ReviewInfo));
    Ok(())
}

#[test]
fn test_review_state_cleared() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Set up some review state
    app.review.branches = vec![crate::git::BranchInfo {
        name: "test".to_string(),
        full_name: "refs/heads/test".to_string(),
        is_remote: false,
        remote: None,
        last_commit_time: None,
    }];
    app.review.filter = "filter".to_string();
    app.review.selected = 1;

    // Clear the state
    app.clear_review_state();

    assert!(app.review.branches.is_empty());
    assert!(app.review.filter.is_empty());
    assert_eq!(app.review.selected, 0);
    assert!(app.review.base_branch.is_none());
    Ok(())
}

#[test]
fn test_review_info_mode_exit() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Enter ReviewInfo mode
    app.show_review_info();
    assert_eq!(app.mode, Mode::Overlay(OverlayMode::ReviewInfo));

    // Cancel should exit
    handler.handle_action(&mut app, Action::Cancel)?;
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_git_op_state_cleared_properly() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Set up git op state
    app.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.git_op.branch_name = "test-branch".to_string();
    app.git_op.original_branch = "original".to_string();
    app.git_op.base_branch = "main".to_string();
    app.git_op.has_unpushed = true;
    app.git_op.is_root_rename = true;

    // Clear the state
    app.clear_git_op_state();

    // Verify all fields are cleared
    assert!(app.git_op.agent_id.is_none());
    assert!(app.git_op.branch_name.is_empty());
    assert!(app.git_op.original_branch.is_empty());
    assert!(app.git_op.base_branch.is_empty());
    assert!(!app.git_op.has_unpushed);
    assert!(!app.git_op.is_root_rename);
    Ok(())
}

#[test]
fn test_worktree_conflict_info_struct() -> Result<(), Box<dyn std::error::Error>> {
    use crate::app::WorktreeConflictInfo;

    let (mut app, _temp) = create_test_app()?;

    // Set up conflict info manually
    app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "test".to_string(),
        prompt: Some("test prompt".to_string()),
        branch: "tenex/test".to_string(),
        worktree_path: std::path::PathBuf::from("/tmp/test"),
        existing_branch: Some("tenex/test".to_string()),
        existing_commit: Some("abc1234".to_string()),
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: None,
    });

    // Verify the conflict info is set
    assert!(
        app.spawn.worktree_conflict.is_some(),
        "Expected worktree_conflict to be set"
    );
    let info = app
        .spawn
        .worktree_conflict
        .as_ref()
        .ok_or("conflict info not set")?;
    assert_eq!(info.title, "test");
    assert_eq!(info.swarm_child_count, None);
    Ok(())
}

#[test]
fn test_worktree_conflict_info_swarm() -> Result<(), Box<dyn std::error::Error>> {
    use crate::app::WorktreeConflictInfo;

    let (mut app, _temp) = create_test_app()?;

    // Set up conflict info for a swarm
    app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "swarm".to_string(),
        prompt: Some("swarm task".to_string()),
        branch: "tenex/swarm".to_string(),
        worktree_path: std::path::PathBuf::from("/tmp/swarm"),
        existing_branch: Some("tenex/swarm".to_string()),
        existing_commit: Some("abc1234".to_string()),
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: Some(3),
    });

    assert!(
        app.spawn.worktree_conflict.is_some(),
        "Expected worktree_conflict to be set"
    );
    let info = app
        .spawn
        .worktree_conflict
        .as_ref()
        .ok_or("conflict info not set")?;
    assert_eq!(info.swarm_child_count, Some(3));
    Ok(())
}

// === Terminal Spawning Tests ===

#[test]
fn test_spawn_terminal_requires_selected_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // No agent selected - SpawnTerminal should do nothing
    handler.handle_action(&mut app, Action::SpawnTerminal)?;
    assert_eq!(app.storage.len(), 0);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_spawn_terminal_prompted_requires_selected_agent() -> Result<(), Box<dyn std::error::Error>>
{
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // No agent selected - SpawnTerminalPrompted should not enter mode
    handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_spawn_terminal_prompted_enters_mode_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // With agent selected - should enter TerminalPrompt mode
    handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
    assert_eq!(app.mode, text_input_mode(TextInputKind::TerminalPrompt));
    Ok(())
}

#[test]
fn test_spawn_terminal_increments_counter() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    app.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // Counter starts at 0
    assert_eq!(app.spawn.terminal_counter, 0);

    // Get first terminal name
    let name1 = app.next_terminal_name();
    assert_eq!(name1, "Terminal 1");
    assert_eq!(app.spawn.terminal_counter, 1);

    // Get second terminal name
    let name2 = app.next_terminal_name();
    assert_eq!(name2, "Terminal 2");
    assert_eq!(app.spawn.terminal_counter, 2);
    Ok(())
}

#[test]
fn test_terminal_is_marked_as_terminal() {
    use crate::agent::{Agent, ChildConfig};
    use std::path::PathBuf;

    // Create a terminal child
    let mut terminal = Agent::new_child(
        "Terminal 1".to_string(),
        "terminal".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp"),
        None,
        ChildConfig {
            parent_id: uuid::Uuid::new_v4(),
            mux_session: "test-session".to_string(),
            window_index: 2,
        },
    );
    terminal.is_terminal = true;

    assert!(terminal.is_terminal);
    assert_eq!(terminal.program, "terminal");
}

#[test]
fn test_terminal_spawning_flow_end_to_end() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // 1. Without agent - [t] does nothing
    handler.handle_action(&mut app, Action::SpawnTerminal)?;
    assert_eq!(app.storage.len(), 0);

    // 2. Without agent - [T] does nothing
    handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
    assert_eq!(app.mode, Mode::Normal);

    // 3. Add an agent
    app.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    // 4. With agent - [T] enters prompt mode
    handler.handle_action(&mut app, Action::SpawnTerminalPrompted)?;
    assert_eq!(app.mode, text_input_mode(TextInputKind::TerminalPrompt));

    // 5. Cancel and verify we're back to normal
    app.exit_mode();
    assert_eq!(app.mode, Mode::Normal);

    Ok(())
}

// === New Handler Helper Function Tests ===

#[test]
fn test_handle_unfocus_preview() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::PreviewFocused;

    Actions::handle_unfocus_preview(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_unfocus_preview_not_in_preview() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Normal;

    // Should not change mode if not in PreviewFocused
    Actions::handle_unfocus_preview(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_kill_action_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    Actions::handle_kill_action(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_kill_action_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let (mut app, _temp) = create_test_app()?;
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    Actions::handle_kill_action(&mut app);
    assert_eq!(app.mode, confirm_action_mode(ConfirmAction::Kill));
    Ok(())
}

#[test]
fn test_handle_quit_action_no_running_agents() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    Actions::handle_quit_action(&mut app);
    assert!(app.should_quit);
    Ok(())
}

#[test]
fn test_handle_quit_action_with_running_agents() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let (mut app, _temp) = create_test_app()?;
    let mut agent = Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    );
    agent.status = Status::Running;
    app.storage.add(agent);

    Actions::handle_quit_action(&mut app);
    assert!(!app.should_quit);
    assert_eq!(app.mode, confirm_action_mode(ConfirmAction::Quit));
    Ok(())
}

#[test]
fn test_handle_add_children_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    Actions::handle_add_children(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    Actions::handle_synthesize(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let (mut app, _temp) = create_test_app()?;
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    Actions::handle_synthesize(&mut app);
    // Should show error, not enter mode
    assert!(matches!(app.mode, Mode::Overlay(OverlayMode::Error(_))));
    Ok(())
}

#[test]
fn test_handle_broadcast_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    Actions::handle_broadcast(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_broadcast_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let (mut app, _temp) = create_test_app()?;
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    Actions::handle_broadcast(&mut app);
    assert_eq!(app.mode, text_input_mode(TextInputKind::Broadcasting));
    Ok(())
}

#[test]
fn test_handle_spawn_terminal_prompted_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    Actions::handle_spawn_terminal_prompted(&mut app);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_spawn_terminal_prompted_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    use crate::agent::Agent;
    use std::path::PathBuf;

    let (mut app, _temp) = create_test_app()?;
    app.storage.add(Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
        None,
    ));

    Actions::handle_spawn_terminal_prompted(&mut app);
    assert_eq!(app.mode, text_input_mode(TextInputKind::TerminalPrompt));
    Ok(())
}
