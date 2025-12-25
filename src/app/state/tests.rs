use super::*;
use crate::agent::Agent;
use crate::app::AgentProgram;
use std::path::PathBuf;

fn create_test_agent(title: &str) -> Agent {
    Agent::new(
        title.to_string(),
        "claude".to_string(),
        format!("tenex/{title}"),
        PathBuf::from("/tmp/worktree"),
        None,
    )
}

#[test]
fn test_app_new() {
    let app = App::default();
    assert_eq!(app.selected, 0);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.active_tab, Tab::Preview);
    assert!(!app.should_quit);
}

#[test]
fn test_select_next() {
    let mut app = App::default();
    app.storage.add(create_test_agent("agent1"));
    app.storage.add(create_test_agent("agent2"));
    app.storage.add(create_test_agent("agent3"));

    assert_eq!(app.selected, 0);
    app.select_next();
    assert_eq!(app.selected, 1);
    app.select_next();
    assert_eq!(app.selected, 2);
    app.select_next();
    assert_eq!(app.selected, 0);
}

#[test]
fn test_select_prev() {
    let mut app = App::default();
    app.storage.add(create_test_agent("agent1"));
    app.storage.add(create_test_agent("agent2"));
    app.storage.add(create_test_agent("agent3"));

    assert_eq!(app.selected, 0);
    app.select_prev();
    assert_eq!(app.selected, 2);
    app.select_prev();
    assert_eq!(app.selected, 1);
}

#[test]
fn test_select_empty_storage() {
    let mut app = App::default();
    app.select_next();
    assert_eq!(app.selected, 0);
    app.select_prev();
    assert_eq!(app.selected, 0);
}

#[test]
fn test_switch_tab() {
    let mut app = App::default();
    assert_eq!(app.active_tab, Tab::Preview);

    app.switch_tab();
    assert_eq!(app.active_tab, Tab::Diff);

    app.switch_tab();
    assert_eq!(app.active_tab, Tab::Preview);
}

#[test]
fn test_enter_exit_mode() {
    let mut app = App::default();

    app.enter_mode(Mode::Creating);
    assert_eq!(app.mode, Mode::Creating);
    assert!(app.input.buffer.is_empty());

    app.input.buffer.push_str("test");
    app.exit_mode();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.input.buffer.is_empty());
}

#[test]
fn test_error_handling() {
    let mut app = App::default();

    app.set_error("Test error");
    assert_eq!(app.ui.last_error, Some("Test error".to_string()));

    app.clear_error();
    assert!(app.ui.last_error.is_none());
}

#[test]
fn test_status_handling() {
    let mut app = App::default();

    app.set_status("Test status");
    assert_eq!(app.ui.status_message, Some("Test status".to_string()));

    app.clear_status();
    assert!(app.ui.status_message.is_none());
}

#[test]
fn test_handle_char() {
    let mut app = App::default();

    app.handle_char('a');
    assert!(app.input.buffer.is_empty());

    app.enter_mode(Mode::Creating);
    app.handle_char('t');
    app.handle_char('e');
    app.handle_char('s');
    app.handle_char('t');
    assert_eq!(app.input.buffer, "test");
}

#[test]
fn test_handle_backspace() {
    let mut app = App::default();
    app.enter_mode(Mode::Creating);
    app.input.buffer = "test".to_string();
    app.input.cursor = 4; // Cursor at end

    app.handle_backspace();
    assert_eq!(app.input.buffer, "tes");
    assert_eq!(app.input.cursor, 3);

    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    assert!(app.input.buffer.is_empty());
    assert_eq!(app.input.cursor, 0);

    app.handle_backspace();
    assert!(app.input.buffer.is_empty());
}

#[test]
fn test_tab_display() {
    assert_eq!(format!("{}", Tab::Preview), "Preview");
    assert_eq!(format!("{}", Tab::Diff), "Diff");
}

#[test]
fn test_app_mode_default() {
    assert_eq!(Mode::default(), Mode::Normal);
}

#[test]
fn test_input_mode_default() {
    assert_eq!(InputMode::default(), InputMode::Normal);
}

#[test]
fn test_confirm_action_equality() {
    assert_eq!(ConfirmAction::Kill, ConfirmAction::Kill);
    assert_ne!(ConfirmAction::Kill, ConfirmAction::Reset);
}

#[test]
fn test_increment_child_count() {
    let mut app = App::default();
    assert_eq!(app.spawn.child_count, 3);
    app.increment_child_count();
    assert_eq!(app.spawn.child_count, 4);
}

#[test]
fn test_decrement_child_count() {
    let mut app = App::default();
    app.decrement_child_count();
    assert_eq!(app.spawn.child_count, 2);
    app.spawn.child_count = 1;
    app.decrement_child_count();
    assert_eq!(app.spawn.child_count, 1); // Minimum is 1
}

#[test]
fn test_start_spawning_under() {
    let mut app = App::default();
    let id = uuid::Uuid::new_v4();
    app.start_spawning_under(id);
    assert_eq!(app.spawn.spawning_under, Some(id));
    assert_eq!(app.spawn.child_count, 3);
    assert_eq!(app.mode, Mode::ChildCount);
}

#[test]
fn test_start_spawning_root() {
    let mut app = App::default();
    app.start_spawning_root();
    assert!(app.spawn.spawning_under.is_none());
    assert_eq!(app.spawn.child_count, 3);
    assert_eq!(app.mode, Mode::ChildCount);
}

#[test]
fn test_proceed_to_child_prompt() {
    let mut app = App::default();
    app.proceed_to_child_prompt();
    assert_eq!(app.mode, Mode::ChildPrompt);
}

#[test]
fn test_dismiss_error() {
    let mut app = App {
        mode: Mode::ErrorModal("Test error".to_string()),
        ..App::default()
    };
    app.ui.last_error = Some("Test error".to_string());

    // Dismiss it
    app.dismiss_error();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.ui.last_error.is_none());

    // Calling dismiss_error in normal mode should be a no-op for mode
    app.dismiss_error();
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn test_selected_agent_mut() {
    let mut app = App::default();
    // No agents - should return None
    assert!(app.selected_agent_mut().is_none());

    // Add an agent
    app.storage.add(create_test_agent("test"));
    let agent = app.selected_agent_mut();
    assert!(agent.is_some());
    if let Some(a) = agent {
        a.collapsed = true;
    }

    // Verify the change persisted
    assert!(app.selected_agent().is_some_and(|a| a.collapsed));
}

#[test]
fn test_handle_delete() {
    let mut app = App::default();
    app.enter_mode(Mode::Creating);
    app.input.buffer = "test".to_string();
    app.input.cursor = 2; // Cursor at 'st'

    app.handle_delete();
    assert_eq!(app.input.buffer, "tet");
    assert_eq!(app.input.cursor, 2);

    // Delete at end does nothing
    app.input.cursor = 3;
    app.handle_delete();
    assert_eq!(app.input.buffer, "tet");
}

#[test]
fn test_input_cursor_left_right() {
    let mut app = App::default();
    app.input.buffer = "hello".to_string();
    app.input.cursor = 3;

    app.input_cursor_left();
    assert_eq!(app.input.cursor, 2);

    app.input_cursor_right();
    assert_eq!(app.input.cursor, 3);

    // At start, left does nothing
    app.input.cursor = 0;
    app.input_cursor_left();
    assert_eq!(app.input.cursor, 0);

    // At end, right does nothing
    app.input.cursor = 5;
    app.input_cursor_right();
    assert_eq!(app.input.cursor, 5);
}

#[test]
fn test_input_cursor_up_down() {
    let mut app = App::default();
    app.input.buffer = "line1\nline2\nline3".to_string();

    // Start at end of line2
    app.input.cursor = 11; // "line1\nline2" length

    // Move up to line1
    app.input_cursor_up();
    assert_eq!(app.input.cursor, 5); // End of "line1"

    // Move up from line1 does nothing (already at first line)
    app.input_cursor_up();
    assert_eq!(app.input.cursor, 5);

    // Move down to line2
    app.input_cursor_down();
    assert_eq!(app.input.cursor, 11); // End of "line1\nline2"

    // Move down to line3
    app.input_cursor_down();
    assert_eq!(app.input.cursor, 17); // End of string

    // Move down from last line does nothing
    app.input_cursor_down();
    assert_eq!(app.input.cursor, 17);
}

#[test]
fn test_input_cursor_home_end() {
    let mut app = App::default();
    app.input.buffer = "line1\nline2\nline3".to_string();
    app.input.cursor = 8; // Middle of "line2"

    app.input_cursor_home();
    assert_eq!(app.input.cursor, 6); // Start of "line2"

    app.input_cursor_end();
    assert_eq!(app.input.cursor, 11); // End of "line2"

    // Test on first line
    app.input.cursor = 3;
    app.input_cursor_home();
    assert_eq!(app.input.cursor, 0);
}

#[test]
fn test_scroll_methods() {
    let mut app = App::default();
    app.ui.preview_content = "line1\nline2\nline3\nline4\nline5".to_string();
    app.ui.set_diff_content("diff1\ndiff2\ndiff3");
    app.ui.preview_dimensions = Some((80, 2));

    // Test scroll_up in Preview mode
    app.ui.preview_scroll = 2;
    app.scroll_up(1);
    assert_eq!(app.ui.preview_scroll, 1);
    assert!(!app.ui.preview_follow);

    // Test scroll_down in Preview mode
    app.scroll_down(1);
    assert_eq!(app.ui.preview_scroll, 2);

    // Test scroll_to_top in Preview mode
    app.scroll_to_top();
    assert_eq!(app.ui.preview_scroll, 0);
    assert!(!app.ui.preview_follow);

    // Test scroll_to_bottom in Preview mode
    app.scroll_to_bottom(5, 2);
    assert_eq!(app.ui.preview_scroll, 3);
    assert!(app.ui.preview_follow);

    // Switch to Diff tab and test
    app.active_tab = Tab::Diff;
    app.ui.diff_scroll = 2;
    app.scroll_up(1);
    // normalize_scroll clamps to max (1 for 3 lines with 2 visible)
    assert!(app.ui.diff_scroll <= 1);

    app.ui.diff_scroll = 0;
    app.scroll_down(1);
    assert_eq!(app.ui.diff_scroll, 1);

    app.scroll_to_top();
    assert_eq!(app.ui.diff_scroll, 0);

    app.scroll_to_bottom(3, 2);
    assert_eq!(app.ui.diff_scroll, 1);
}

#[test]
fn test_start_planning_swarm() {
    let mut app = App::default();
    let agent = create_test_agent("test");
    let agent_id = agent.id;
    app.storage.add(agent);
    app.start_planning_swarm();
    assert_eq!(app.spawn.spawning_under, Some(agent_id));
    assert_eq!(app.spawn.child_count, 3);
    assert!(app.spawn.use_plan_prompt);
    assert_eq!(app.mode, Mode::ChildCount);
}

#[test]
fn test_start_planning_swarm_no_agent() {
    let mut app = App::default();
    app.start_planning_swarm();
    // Should remain in Normal mode, not enter ChildCount
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.spawn.spawning_under.is_none());
}

#[test]
fn test_toggle_selected_collapse() {
    let mut app = App::default();
    app.storage.add(create_test_agent("test"));

    // Initially collapsed (default is true)
    assert!(app.selected_agent().is_some_and(|a| a.collapsed));

    app.toggle_selected_collapse();
    assert!(app.selected_agent().is_some_and(|a| !a.collapsed));

    app.toggle_selected_collapse();
    assert!(app.selected_agent().is_some_and(|a| a.collapsed));
}

#[test]
fn test_selected_has_children() {
    let mut app = App::default();
    let parent = create_test_agent("parent");
    let parent_id = parent.id;
    app.storage.add(parent);

    // No children initially
    assert!(!app.selected_has_children());

    // Add a child
    let mut child = create_test_agent("child");
    child.parent_id = Some(parent_id);
    app.storage.add(child);

    // Now has children
    assert!(app.selected_has_children());
}

#[test]
fn test_set_preview_dimensions() {
    let mut app = App::default();
    assert!(app.ui.preview_dimensions.is_none());

    app.set_preview_dimensions(100, 50);
    assert_eq!(app.ui.preview_dimensions, Some((100, 50)));
}

#[test]
fn test_selected_depth() {
    let mut app = App::default();
    // No agent selected
    assert_eq!(app.selected_depth(), 0);

    // Root agent has depth 0
    app.storage.add(create_test_agent("root"));
    assert_eq!(app.selected_depth(), 0);
}

#[test]
fn test_confirm_rename_branch() {
    let mut app = App::default();

    // Empty input returns false
    app.input.buffer = "   ".to_string();
    assert!(!app.confirm_rename_branch());

    // Valid input returns true and sets branch name
    app.input.buffer = "  new-branch  ".to_string();
    assert!(app.confirm_rename_branch());
    assert_eq!(app.git_op.branch_name, "new-branch");
}

// ========== Command palette tests ==========

#[test]
fn test_start_command_palette() {
    let mut app = App::default();
    app.start_command_palette();

    assert_eq!(app.mode, Mode::CommandPalette);
    assert_eq!(app.input.buffer, "/");
    assert_eq!(app.input.cursor, 1);
    assert_eq!(app.command_palette.selected, 0);
}

#[test]
fn test_filtered_slash_commands_no_filter() {
    let app = App::default();
    let commands = app.filtered_slash_commands();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].name, "/agents");
    assert_eq!(commands[1].name, "/help");
}

#[test]
fn test_filtered_slash_commands_with_filter() {
    let mut app = App::default();
    app.input.buffer = "/age".to_string();

    let commands = app.filtered_slash_commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "/agents");
}

#[test]
fn test_filtered_slash_commands_no_match() {
    let mut app = App::default();
    app.input.buffer = "/xyz".to_string();

    let commands = app.filtered_slash_commands();
    assert!(commands.is_empty());
}

#[test]
fn test_select_next_slash_command() {
    let mut app = App::default();
    app.start_command_palette();

    assert_eq!(app.command_palette.selected, 0);
    app.select_next_slash_command();
    assert_eq!(app.command_palette.selected, 1);
    app.select_next_slash_command();
    assert_eq!(app.command_palette.selected, 0);
}

#[test]
fn test_select_prev_slash_command() {
    let mut app = App::default();
    app.start_command_palette();

    assert_eq!(app.command_palette.selected, 0);
    app.select_prev_slash_command();
    assert_eq!(app.command_palette.selected, 1);
    app.select_prev_slash_command();
    assert_eq!(app.command_palette.selected, 0);
}

#[test]
fn test_reset_slash_command_selection() {
    let mut app = App::default();
    app.command_palette.selected = 5;
    app.reset_slash_command_selection();
    assert_eq!(app.command_palette.selected, 0);
}

#[test]
fn test_selected_slash_command() {
    let mut app = App::default();
    app.start_command_palette();

    let cmd = app.selected_slash_command();
    assert!(cmd.is_some());
    if let Some(c) = cmd {
        assert_eq!(c.name, "/agents");
    }

    app.command_palette.selected = 1;
    let cmd = app.selected_slash_command();
    assert!(cmd.is_some());
    if let Some(c) = cmd {
        assert_eq!(c.name, "/help");
    }
}

#[test]
fn test_run_slash_command_agents() {
    let mut app = App::default();
    app.run_slash_command(SlashCommand {
        name: "/agents",
        description: "test",
    });
    assert_eq!(app.mode, Mode::ModelSelector);
}

#[test]
fn test_run_slash_command_help() {
    let mut app = App::default();
    app.run_slash_command(SlashCommand {
        name: "/help",
        description: "test",
    });
    assert_eq!(app.mode, Mode::Help);
}

#[test]
fn test_run_slash_command_unknown() {
    let mut app = App::default();
    app.enter_mode(Mode::CommandPalette);
    app.run_slash_command(SlashCommand {
        name: "/unknown",
        description: "test",
    });
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.ui.status_message.is_some());
}

#[test]
fn test_submit_slash_command_palette_empty() {
    let mut app = App::default();
    app.start_command_palette();
    app.input.buffer = "/".to_string();
    app.submit_slash_command_palette();
    assert_eq!(app.mode, Mode::Normal);
}

#[test]
fn test_submit_slash_command_palette_exact_match() {
    let mut app = App::default();
    app.start_command_palette();
    app.input.buffer = "/help".to_string();
    app.submit_slash_command_palette();
    assert_eq!(app.mode, Mode::Help);
}

#[test]
fn test_submit_slash_command_palette_prefix_match() {
    let mut app = App::default();
    app.start_command_palette();
    app.input.buffer = "/hel".to_string();
    app.submit_slash_command_palette();
    assert_eq!(app.mode, Mode::Help);
}

#[test]
fn test_submit_slash_command_palette_unknown() {
    let mut app = App::default();
    app.start_command_palette();
    app.input.buffer = "/xyz".to_string();
    app.submit_slash_command_palette();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.ui.status_message.is_some());
}

#[test]
fn test_confirm_slash_command_selection() {
    let mut app = App::default();
    app.start_command_palette();
    app.command_palette.selected = 1;
    app.confirm_slash_command_selection();
    assert_eq!(app.mode, Mode::Help);
}

// ========== Model selector tests ==========

#[test]
fn test_start_model_selector() {
    let mut app = App::default();
    app.start_model_selector();

    assert_eq!(app.mode, Mode::ModelSelector);
}

#[test]
fn test_filtered_model_programs() {
    let mut app = App::default();
    app.start_model_selector();

    let programs = app.filtered_model_programs();
    assert_eq!(programs.len(), 3);
}

#[test]
fn test_select_next_model_program() {
    let mut app = App::default();
    app.start_model_selector();
    let initial = app.model_selector.selected;

    app.select_next_model_program();
    assert_eq!(app.model_selector.selected, (initial + 1) % 3);
}

#[test]
fn test_select_prev_model_program() {
    let mut app = App::default();
    app.start_model_selector();
    app.model_selector.selected = 0;

    app.select_prev_model_program();
    assert_eq!(app.model_selector.selected, 2);
}

#[test]
fn test_handle_model_filter_char() {
    let mut app = App::default();
    app.start_model_selector();

    app.handle_model_filter_char('c');
    assert_eq!(app.model_selector.filter, "c");
}

#[test]
fn test_handle_model_filter_backspace() {
    let mut app = App::default();
    app.start_model_selector();
    app.model_selector.filter = "abc".to_string();

    app.handle_model_filter_backspace();
    assert_eq!(app.model_selector.filter, "ab");
}

#[test]
fn test_selected_model_program() {
    let mut app = App::default();
    app.start_model_selector();

    let program = app.selected_model_program();
    assert!(program.is_some());
}

#[test]
fn test_agent_spawn_command_claude() {
    let mut app = App::default();
    app.settings.agent_program = AgentProgram::Claude;

    let cmd = app.agent_spawn_command();
    assert_eq!(cmd, app.config.default_program);
}

#[test]
fn test_agent_spawn_command_codex() {
    let mut app = App::default();
    app.settings.agent_program = AgentProgram::Codex;

    let cmd = app.agent_spawn_command();
    assert_eq!(cmd, "codex");
}

#[test]
fn test_agent_spawn_command_custom() {
    let mut app = App::default();
    app.settings.agent_program = AgentProgram::Custom;
    app.settings.custom_agent_command = "my-agent --flag".to_string();

    let cmd = app.agent_spawn_command();
    assert_eq!(cmd, "my-agent --flag");
}

#[test]
fn test_agent_spawn_command_custom_empty() {
    let mut app = App::default();
    app.settings.agent_program = AgentProgram::Custom;
    app.settings.custom_agent_command = "   ".to_string();

    let cmd = app.agent_spawn_command();
    assert_eq!(cmd, app.config.default_program);
}

#[test]
fn test_start_custom_agent_command_prompt() {
    let mut app = App::default();
    app.settings.custom_agent_command = "my-agent".to_string();

    app.start_custom_agent_command_prompt();
    assert_eq!(app.mode, Mode::CustomAgentCommand);
    assert_eq!(app.input.buffer, "my-agent");
}

#[test]
fn test_confirm_model_program_selection_codex() {
    let mut app = App::default();
    app.start_model_selector();
    app.model_selector.selected = 0;

    app.confirm_model_program_selection();
    // Should return to normal mode (may have error due to no file system)
    assert!(matches!(app.mode, Mode::Normal | Mode::ErrorModal(_)));
}
