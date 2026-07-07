use super::*;
use crate::agent::Agent;
use crate::agent::Storage;
use crate::app::{Settings, WorktreeConflictInfo};
use crate::config::Config;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tempfile::TempDir;

fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::sink)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::with_default(&dispatch, f)
}

fn empty_data() -> AppData {
    AppData::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    )
}

fn make_conflict(prompt: Option<&str>) -> WorktreeConflictInfo {
    WorktreeConflictInfo {
        title: "conflict-title".to_string(),
        prompt: prompt.map(str::to_string),
        branch: "tenex/conflict-title".to_string(),
        worktree_path: PathBuf::from("/tmp/tenex-confirm-action-conflict"),
        repo_root: PathBuf::from("/tmp"),
        existing_branch: Some("tenex/conflict-title".to_string()),
        existing_commit: Some("abc1234".to_string()),
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: None,
    }
}

fn is_reconnect_prompt(mode: &AppMode) -> bool {
    matches!(mode, AppMode::ReconnectPrompt(_))
}

fn is_error_modal(mode: &AppMode) -> bool {
    matches!(mode, AppMode::ErrorModal(_))
}

#[test]
fn test_confirm_yes_quit_sets_should_quit() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::Quit,
    };

    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.should_quit);
}

#[test]
fn test_confirm_yes_kill_succeeds_for_selected_agent() {
    let storage_path = NamedTempFile::new()
        .expect("temp state file should be created")
        .into_temp_path();
    let storage = Storage::with_path(storage_path.to_path_buf());
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);

    let repo_root = TempDir::new().expect("temp repo root should be created");
    let mut root = Agent::new(
        "root".to_string(),
        "bash".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    root.repo_root = Some(repo_root.path().to_path_buf());
    data.storage.add(root);
    data.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::Kill,
    };
    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes kill should succeed");

    assert_eq!(next, AppMode::normal());
    assert!(data.storage.is_empty());
    assert!(
        data.ui
            .status_message
            .as_ref()
            .is_some_and(|msg| msg.contains("Agent killed"))
    );
}

#[test]
fn test_confirm_yes_reset_succeeds_without_agents() {
    let storage_path = NamedTempFile::new()
        .expect("temp state file should be created")
        .into_temp_path();
    let storage = Storage::with_path(storage_path.to_path_buf());
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);

    let state = ConfirmingMode {
        action: ConfirmAction::Reset,
    };
    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes reset should succeed");

    assert_eq!(next, AppMode::normal());
    assert!(data.storage.is_empty());
    assert!(
        data.ui
            .status_message
            .as_ref()
            .is_some_and(|msg| msg.contains("All agents reset"))
    );
}

#[test]
fn test_confirm_yes_synthesize_enters_synthesis_prompt() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::Synthesize,
    };

    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes synthesize should succeed");
    assert_eq!(next, AppMode::SynthesisPrompt(SynthesisPromptMode));
}

#[test]
fn test_confirm_interrupt_agent_yes_returns_to_preview_focus() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };

    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_confirm_interrupt_agent_sends_ctrl_c_to_selected_agent_when_present() {
    let mut data = empty_data();
    data.storage.add(Agent::new(
        "test-agent".to_string(),
        "bash".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    ));
    data.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };

    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_confirm_interrupt_agent_warns_when_send_ctrl_c_fails() {
    let _guard = crate::test_support::lock_mux_test_environment();

    let socket = "tenex-confirm-interrupt\0invalid";
    crate::mux::set_socket_override(socket).expect("socket override should be set");

    let mut data = empty_data();
    let mut agent = Agent::new(
        "test-agent".to_string(),
        "bash".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    agent.mux_session = "tenex-confirm-interrupt-session".to_string();
    agent.window_index = Some(1);
    data.storage.add(agent);
    data.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };
    let next = with_tracing_dispatch(|| ConfirmYesAction.execute(state, &mut data))
        .expect("confirm yes interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_confirm_interrupt_agent_skips_terminal_agent() {
    let mut data = empty_data();
    let mut agent = Agent::new(
        "terminal".to_string(),
        "terminal".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    agent.is_terminal = true;
    data.storage.add(agent);
    data.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };

    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_confirm_interrupt_agent_sends_ctrl_c_when_target_exists() {
    let socket = format!("tenex-confirm-interrupt-{}", std::process::id());
    let _ = crate::mux::set_socket_override(&socket);

    let temp_dir = TempDir::new().expect("temp dir should be created");
    let session = format!("tenex-confirm-interrupt-{}", std::process::id());
    let manager = crate::mux::SessionManager::new();
    manager
        .create(&session, temp_dir.path(), None)
        .expect("mux session should be created");
    let window_index = manager
        .create_window(&session, "target", temp_dir.path(), None)
        .expect("mux window should be created");

    let mut data = empty_data();
    let mut agent = Agent::new(
        "test-agent".to_string(),
        "bash".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    agent.mux_session = session.clone();
    agent.window_index = Some(window_index);
    data.storage.add(agent);
    data.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };
    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));

    let _ = manager.kill(&session);
}

#[test]
fn test_confirm_interrupt_agent_no_returns_to_preview_focus() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };

    let next = ConfirmNoAction
        .execute(state, &mut data)
        .expect("confirm no interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_confirm_interrupt_agent_cancel_returns_to_preview_focus() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::InterruptAgent,
    };

    let next = CancelAction
        .execute(state, &mut data)
        .expect("cancel interrupt should succeed");
    assert_eq!(next, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_cancel_action_clears_worktree_conflict() {
    let mut data = empty_data();
    data.spawn.worktree_conflict = Some(make_conflict(Some("prompt")));

    let state = ConfirmingMode {
        action: ConfirmAction::WorktreeConflict,
    };
    let next = CancelAction
        .execute(state, &mut data)
        .expect("cancel worktree conflict should succeed");

    assert_eq!(next, AppMode::normal());
    assert!(data.spawn.worktree_conflict.is_none());
}

#[test]
fn test_worktree_reconnect_action_noop_when_not_in_conflict() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::Kill,
    };

    let next = WorktreeReconnectAction
        .execute(state, &mut data)
        .expect("worktree reconnect should succeed");
    assert_eq!(next, state.into());
}

#[test]
fn test_worktree_reconnect_action_enters_prompt_and_preloads_input() {
    let mut data = empty_data();
    data.spawn.worktree_conflict = Some(make_conflict(Some("hello world")));

    let state = ConfirmingMode {
        action: ConfirmAction::WorktreeConflict,
    };
    let next = WorktreeReconnectAction
        .execute(state, &mut data)
        .expect("worktree reconnect should succeed");

    assert!(is_reconnect_prompt(&next));
    assert!(!is_reconnect_prompt(&AppMode::normal()));
    assert_eq!(data.input.buffer, "hello world");
    assert_eq!(data.input.cursor, data.input.buffer.len());
}

#[test]
fn test_submit_action_in_rename_branch_mode_noops_on_empty_input() {
    let mut data = empty_data();
    data.input.buffer = "   ".to_string();

    let state = RenameBranchMode;
    let next = SubmitAction
        .execute(state, &mut data)
        .expect("submit action should succeed");
    assert_eq!(next, state.into());
}

#[test]
fn test_char_and_backspace_in_rename_branch_mode() {
    let mut data = empty_data();
    data.input.buffer = String::new();
    data.input.cursor = 0;

    let state = RenameBranchMode;
    let next = CharInputAction('a')
        .execute(state, &mut data)
        .expect("char input should succeed");
    assert_eq!(next, state.into());
    assert_eq!(data.input.buffer, "a");
    assert_eq!(data.input.cursor, 1);

    let next = BackspaceAction
        .execute(state, &mut data)
        .expect("backspace should succeed");
    assert_eq!(next, state.into());
    assert!(data.input.buffer.is_empty());
    assert_eq!(data.input.cursor, 0);
}

#[test]
fn test_confirm_yes_switch_branch_returns_error_and_clears_state_without_agent() {
    let mut data = empty_data();
    data.git_op.branch_name = "main".to_string();
    data.git_op.target_branch = "feature".to_string();
    data.review.filter = "m".to_string();
    data.review.selected = 3;
    data.review.base_branch = Some("main".to_string());

    let state = ConfirmingMode {
        action: ConfirmAction::SwitchBranch,
    };
    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes switch branch should succeed");

    assert!(is_error_modal(&next));
    assert!(!is_error_modal(&AppMode::normal()));
    assert!(data.git_op.agent_id.is_none());
    assert!(data.git_op.branch_name.is_empty());
    assert!(data.git_op.target_branch.is_empty());
    assert!(data.review.filter.is_empty());
    assert_eq!(data.review.selected, 0);
    assert!(data.review.base_branch.is_none());
}

#[test]
fn test_confirm_yes_restart_mux_daemon_covers_match_arm() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::RestartMuxDaemon,
    };

    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes restart mux daemon should succeed");
    assert_eq!(next, AppMode::normal());
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Mux daemon restarted")
    );
}

#[test]
fn test_confirm_yes_restart_mux_daemon_returns_error_modal_on_failure() {
    let mut data = empty_data();

    crate::mux::set_socket_override("/tmp/tenex\0mux").expect("socket override should be set");

    let state = ConfirmingMode {
        action: ConfirmAction::RestartMuxDaemon,
    };
    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes restart mux daemon should succeed");
    assert!(is_error_modal(&next));
    assert!(!is_error_modal(&AppMode::normal()));
}

#[test]
fn test_confirm_yes_worktree_conflict_noops() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::WorktreeConflict,
    };
    let next = ConfirmYesAction
        .execute(state, &mut data)
        .expect("confirm yes worktree conflict should succeed");
    assert_eq!(next, AppMode::normal());
}

#[test]
fn test_worktree_reconnect_action_enters_prompt_without_conflict_info() {
    let mut data = empty_data();
    data.input.buffer = "existing".to_string();
    data.input.cursor = 3;

    let state = ConfirmingMode {
        action: ConfirmAction::WorktreeConflict,
    };
    let next = WorktreeReconnectAction
        .execute(state, &mut data)
        .expect("worktree reconnect should succeed");
    assert!(is_reconnect_prompt(&next));
    assert!(!is_reconnect_prompt(&AppMode::normal()));
    assert_eq!(data.input.buffer, "existing");
    assert_eq!(data.input.cursor, 3);
}

#[test]
fn test_worktree_recreate_action_noops_when_not_in_conflict() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::Kill,
    };

    let next = WorktreeRecreateAction
        .execute(state, &mut data)
        .expect("worktree recreate should succeed");
    assert_eq!(next, state.into());
}

#[test]
fn test_worktree_recreate_action_errors_when_missing_conflict_info() {
    let mut data = empty_data();
    let state = ConfirmingMode {
        action: ConfirmAction::WorktreeConflict,
    };
    let err = WorktreeRecreateAction
        .execute(state, &mut data)
        .expect_err("Expected recreate to error without conflict info");
    assert!(format!("{err:#}").contains("No worktree conflict info available"));
}

#[test]
fn test_confirm_yes_kill_propagates_save_errors() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let storage = Storage::with_path(temp_dir.path().to_path_buf());
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);

    let repo_root = TempDir::new().expect("temp repo root should be created");
    let mut root = Agent::new(
        "root".to_string(),
        "bash".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    root.repo_root = Some(repo_root.path().to_path_buf());
    data.storage.add(root);
    data.selected = 1;
    let state = ConfirmingMode {
        action: ConfirmAction::Kill,
    };
    assert!(ConfirmYesAction.execute(state, &mut data).is_err());
}

#[test]
fn test_confirm_yes_reset_propagates_save_errors() {
    let temp_dir = TempDir::new().expect("temp dir should be created");
    let storage = Storage::with_path(temp_dir.path().to_path_buf());
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);

    let state = ConfirmingMode {
        action: ConfirmAction::Reset,
    };
    assert!(ConfirmYesAction.execute(state, &mut data).is_err());
}

#[test]
fn test_keyboard_remap_prompt_actions_cover_success_paths() {
    let settings_path = NamedTempFile::new()
        .expect("settings file should be created")
        .into_temp_path();
    Settings::set_test_path_override(settings_path.to_path_buf())
        .expect("settings override should be set");

    let mut data = empty_data();
    data.settings = Settings::default();

    let next = ConfirmYesAction
        .execute(KeyboardRemapPromptMode, &mut data)
        .expect("confirm yes keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.settings.merge_key_remapped);
    assert!(data.settings.keyboard_remap_asked);

    let next = ConfirmNoAction
        .execute(KeyboardRemapPromptMode, &mut data)
        .expect("confirm no keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(!data.settings.merge_key_remapped);
    assert!(data.settings.keyboard_remap_asked);
}

#[test]
fn test_cancel_action_in_keyboard_remap_prompt_logs_and_returns_normal() {
    let mut data = empty_data();
    data.settings = Settings::default();

    let next = CancelAction
        .execute(KeyboardRemapPromptMode, &mut data)
        .expect("cancel keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.settings.keyboard_remap_asked);
}

#[test]
fn test_keyboard_remap_prompt_actions_cover_error_paths() {
    let mut data = empty_data();
    data.settings = Settings::default();

    let next =
        with_tracing_dispatch(|| ConfirmYesAction.execute(KeyboardRemapPromptMode, &mut data))
            .expect("confirm yes keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.settings.merge_key_remapped);
    assert!(data.settings.keyboard_remap_asked);

    let next =
        with_tracing_dispatch(|| ConfirmNoAction.execute(KeyboardRemapPromptMode, &mut data))
            .expect("confirm no keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(!data.settings.merge_key_remapped);
    assert!(data.settings.keyboard_remap_asked);

    let next = with_tracing_dispatch(|| CancelAction.execute(KeyboardRemapPromptMode, &mut data))
        .expect("cancel keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(!data.settings.merge_key_remapped);
    assert!(data.settings.keyboard_remap_asked);
}

#[test]
fn test_cancel_action_in_keyboard_remap_prompt_returns_normal_when_settings_writable() {
    let settings_path = NamedTempFile::new()
        .expect("settings file should be created")
        .into_temp_path();
    Settings::set_test_path_override(settings_path.to_path_buf())
        .expect("settings override should be set");

    let mut data = empty_data();
    data.settings = Settings::default();

    let next = CancelAction
        .execute(KeyboardRemapPromptMode, &mut data)
        .expect("cancel keyboard remap prompt should succeed");
    assert_eq!(next, AppMode::normal());
    assert!(data.settings.keyboard_remap_asked);
}

#[test]
fn test_confirm_no_switch_branch_clears_git_state() {
    let mut data = empty_data();
    data.git_op.branch_name = "main".to_string();
    data.git_op.target_branch = "feature".to_string();
    data.review.filter = "m".to_string();
    data.review.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::SwitchBranch,
    };
    let next = ConfirmNoAction
        .execute(state, &mut data)
        .expect("confirm no switch branch should succeed");

    assert_eq!(next, AppMode::normal());
    assert!(data.git_op.agent_id.is_none());
    assert!(data.git_op.branch_name.is_empty());
    assert!(data.git_op.target_branch.is_empty());
    assert!(data.review.branches.is_empty());
    assert!(data.review.filter.is_empty());
    assert_eq!(data.review.selected, 0);
}

#[test]
fn test_cancel_action_switch_branch_clears_git_state() {
    let mut data = empty_data();
    data.git_op.branch_name = "main".to_string();
    data.git_op.target_branch = "feature".to_string();
    data.review.filter = "m".to_string();
    data.review.selected = 1;

    let state = ConfirmingMode {
        action: ConfirmAction::SwitchBranch,
    };
    let next = CancelAction
        .execute(state, &mut data)
        .expect("cancel switch branch should succeed");

    assert_eq!(next, AppMode::normal());
    assert!(data.git_op.agent_id.is_none());
    assert!(data.git_op.branch_name.is_empty());
    assert!(data.git_op.target_branch.is_empty());
    assert!(data.review.branches.is_empty());
    assert!(data.review.filter.is_empty());
    assert_eq!(data.review.selected, 0);
}
