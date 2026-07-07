use super::*;
use crate::agent::{Agent, Storage};
use crate::app::Settings;
use crate::config::Config;
use crate::git::BranchInfo;
use std::path::PathBuf;
use tempfile::TempDir;

fn empty_data() -> AppData {
    AppData::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    )
}

fn make_local_branch(name: &str) -> BranchInfo {
    BranchInfo {
        name: name.to_string(),
        full_name: format!("refs/heads/{name}"),
        is_remote: false,
        remote: None,
        last_commit_time: None,
    }
}

fn is_error_modal(mode: &AppMode) -> bool {
    matches!(mode, AppMode::ErrorModal(_))
}

fn success_message(mode: &AppMode) -> Option<&str> {
    match mode {
        AppMode::SuccessModal(state) => Some(state.message.as_str()),
        _ => None,
    }
}

fn is_switch_branch_confirming(mode: &AppMode) -> bool {
    matches!(
        mode,
        AppMode::Confirming(ConfirmingMode {
            action: ConfirmAction::SwitchBranch
        })
    )
}

#[test]
fn test_increment_and_decrement_actions_update_child_count() {
    let mut data = empty_data();
    let initial = data.spawn.child_count;

    let next = IncrementAction
        .execute(ChildCountMode, &mut data)
        .expect("increment action should succeed");
    assert_eq!(next, ChildCountMode.into());
    assert_eq!(data.spawn.child_count, initial + 1);

    let next = DecrementAction
        .execute(ChildCountMode, &mut data)
        .expect("decrement action should succeed");
    assert_eq!(next, ChildCountMode.into());
    assert_eq!(data.spawn.child_count, initial);
}

#[test]
fn test_select_action_in_review_child_count_enters_branch_selector() {
    let mut data = empty_data();
    let next = SelectAction
        .execute(ReviewChildCountMode, &mut data)
        .expect("select action should succeed");
    assert_eq!(next, BranchSelectorMode.into());
}

#[test]
fn test_cancel_action_in_review_info_mode_returns_normal() {
    let mut data = empty_data();
    let next = CancelAction
        .execute(ReviewInfoMode, &mut data)
        .expect("cancel action should succeed");
    assert_eq!(next, AppMode::normal());
}

#[test]
fn test_cancel_action_in_review_info_mode_propagates_forced_action_errors() {
    with_forced_picker_action_error_for_tests(|| {
        let mut data = empty_data();
        let err = CancelAction
            .execute(ReviewInfoMode, &mut data)
            .expect_err("expected forced action error");
        assert!(err.to_string().contains("forced picker action error"));
    });
}

#[test]
fn test_cancel_action_in_branch_selector_clears_review_state() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.filter = "m".to_string();
    data.review.selected = 1;

    let next = CancelAction
        .execute(BranchSelectorMode, &mut data)
        .expect("cancel action should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.review.branches.is_empty());
    assert!(data.review.filter.is_empty());
    assert_eq!(data.review.selected, 0);
}

#[test]
fn test_branch_selector_navigation_actions_update_selection() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main"), make_local_branch("develop")];
    data.review.selected = 0;

    let next = NavigateDownAction
        .execute(BranchSelectorMode, &mut data)
        .expect("navigate down should succeed");
    assert_eq!(next, BranchSelectorMode.into());
    assert_eq!(data.review.selected, 1);

    let next = NavigateUpAction
        .execute(BranchSelectorMode, &mut data)
        .expect("navigate up should succeed");
    assert_eq!(next, BranchSelectorMode.into());
    assert_eq!(data.review.selected, 0);
}

#[test]
fn test_select_action_in_branch_selector_returns_normal_when_review_spawn_succeeds() {
    let dir = TempDir::new().expect("create temp dir");
    let state_path = dir.path().join("state.json");
    let storage = Storage::with_path(state_path);
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);

    data.spawn.child_count = 0;
    let worktree_dir = TempDir::new().expect("create worktree dir");
    let agent = Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "feature".to_string(),
        worktree_dir.path().to_path_buf(),
    );
    data.spawn.spawning_under = Some(agent.id);
    data.storage.add(agent);

    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;

    let next = SelectAction
        .execute(BranchSelectorMode, &mut data)
        .expect("select action should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.review.branches.is_empty());
    assert!(data.review.base_branch.is_none());
}

#[test]
fn test_select_action_in_rebase_branch_selector_noops_without_selection() {
    let mut data = empty_data();
    data.review.branches = Vec::new();

    let state = RebaseBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert_eq!(next, state.into());
}

#[test]
fn test_select_action_in_rebase_branch_selector_returns_error_modal_when_agent_missing() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;
    data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    data.git_op.branch_name = "feature".to_string();

    let state = RebaseBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert!(!is_error_modal(&AppMode::normal()));
    assert!(is_error_modal(&next));
}

#[test]
fn test_select_action_in_rebase_branch_selector_returns_error_modal_when_rebase_errors() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;

    let temp_dir = TempDir::new().expect("temp dir should be created");
    let missing = temp_dir.path().join("missing");
    let agent = Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "feature".to_string(),
        missing,
    );
    data.git_op.agent_id = Some(agent.id);
    data.git_op.branch_name = agent.branch.clone();
    data.storage.add(agent);

    let state = RebaseBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert!(!is_error_modal(&AppMode::normal()));
    assert!(is_error_modal(&next));
}

#[test]
fn test_select_action_in_rebase_branch_selector_succeeds_when_git_succeeds() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;

    let temp_dir = TempDir::new().expect("temp worktree dir should be created");
    let agent = Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "feature".to_string(),
        temp_dir.path().to_path_buf(),
    );
    data.git_op.agent_id = Some(agent.id);
    data.git_op.branch_name = agent.branch.clone();
    data.storage.add(agent);

    crate::git::with_git_program_override_for_tests(PathBuf::from("true"), || {
        let next = SelectAction
            .execute(RebaseBranchSelectorMode, &mut data)
            .expect("rebase select action should succeed");
        assert_eq!(success_message(&AppMode::normal()), None);
        assert_eq!(success_message(&next), Some("Rebased feature onto main"));
    });
}

#[test]
fn test_select_action_in_merge_branch_selector_noops_without_selection() {
    let mut data = empty_data();
    data.review.branches = Vec::new();

    let state = MergeBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert_eq!(next, state.into());
}

#[test]
fn test_select_action_in_merge_branch_selector_returns_error_modal_when_agent_missing() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;
    data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    data.git_op.branch_name = "feature".to_string();

    let state = MergeBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert!(!is_error_modal(&AppMode::normal()));
    assert!(is_error_modal(&next));
}

#[test]
fn test_select_action_in_merge_branch_selector_returns_error_modal_when_merge_errors() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;

    let temp_dir = TempDir::new().expect("temp dir should be created");
    let missing = temp_dir.path().join("missing");
    let agent = Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "feature".to_string(),
        missing,
    );
    data.git_op.agent_id = Some(agent.id);
    data.git_op.branch_name = agent.branch.clone();
    data.storage.add(agent);

    let state = MergeBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert!(!is_error_modal(&AppMode::normal()));
    assert!(is_error_modal(&next));
}

#[test]
fn test_select_action_in_merge_branch_selector_succeeds_when_git_succeeds() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main")];
    data.review.selected = 0;

    let temp_dir = TempDir::new().expect("temp worktree dir should be created");
    let agent = Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "feature".to_string(),
        temp_dir.path().to_path_buf(),
    );
    data.git_op.agent_id = Some(agent.id);
    data.git_op.branch_name = agent.branch.clone();
    data.storage.add(agent);

    crate::git::with_git_program_override_for_tests(PathBuf::from("true"), || {
        let next = SelectAction
            .execute(MergeBranchSelectorMode, &mut data)
            .expect("merge select action should succeed");
        assert_eq!(success_message(&AppMode::normal()), None);
        assert_eq!(success_message(&next), Some("Merged feature into main"));
    });
}

#[test]
fn test_select_action_in_switch_branch_selector_noops_without_selection() {
    let mut data = empty_data();
    data.review.branches = Vec::new();

    let state = SwitchBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert_eq!(next, state.into());
}

#[test]
fn test_select_action_in_switch_branch_selector_enters_confirming() {
    let mut data = empty_data();
    data.review.branches = vec![make_local_branch("main"), make_local_branch("feature")];
    data.review.selected = 1;

    let state = SwitchBranchSelectorMode;
    let next = SelectAction
        .execute(state, &mut data)
        .expect("select action should succeed");
    assert!(!is_switch_branch_confirming(&AppMode::normal()));
    assert!(is_switch_branch_confirming(&next));
    assert_eq!(data.git_op.target_branch, "feature");
}

#[test]
fn test_char_input_in_command_palette_resets_selection() {
    let mut data = empty_data();
    data.command_palette.selected = 1;
    data.input.buffer = "/".to_string();
    data.input.cursor = data.input.buffer.len();

    let next = CharInputAction('a')
        .execute(CommandPaletteMode, &mut data)
        .expect("char input action should succeed");
    assert_eq!(next, CommandPaletteMode.into());
    assert_eq!(data.command_palette.selected, 0);
    assert_eq!(data.input.buffer, "/a");
    assert_eq!(data.input.cursor, 2);
}

#[test]
fn test_command_palette_backspace_exits_when_only_slash_is_present() {
    let mut data = empty_data();
    data.input.buffer = "/".to_string();
    data.input.cursor = data.input.buffer.len();
    data.command_palette.selected = 2;

    let next = BackspaceAction
        .execute(CommandPaletteMode, &mut data)
        .expect("backspace action should succeed");
    assert_eq!(next, AppMode::normal());
    assert_eq!(data.input.buffer, "/");
    assert_eq!(data.command_palette.selected, 2);
}

#[test]
fn test_command_palette_backspace_updates_buffer_and_mode() {
    let mut data = empty_data();
    data.input.buffer = "/a".to_string();
    data.input.cursor = data.input.buffer.len();
    data.command_palette.selected = 2;

    let next = BackspaceAction
        .execute(CommandPaletteMode, &mut data)
        .expect("backspace action should succeed");
    assert_eq!(next, CommandPaletteMode.into());
    assert_eq!(data.input.buffer, "/");
    assert_eq!(data.input.cursor, 1);
    assert_eq!(data.command_palette.selected, 0);

    data.input.buffer = "a".to_string();
    data.input.cursor = data.input.buffer.len();
    data.command_palette.selected = 1;

    let next = BackspaceAction
        .execute(CommandPaletteMode, &mut data)
        .expect("backspace action should succeed");
    assert_eq!(next, AppMode::normal());
    assert_eq!(data.input.buffer, "");
    assert_eq!(data.command_palette.selected, 0);
}
