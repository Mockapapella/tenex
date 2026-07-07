use super::*;
use crate::agent::Storage;
use crate::agent::WorkspaceKind;
use crate::app::Settings;
use crate::config::Config;
use crate::state::{
    AppMode, BroadcastingMode, ChildPromptMode, CreatingMode, CustomAgentCommandMode,
    ErrorModalMode, PromptingMode, ReconnectPromptMode, SynthesisPromptMode, TerminalPromptMode,
};
use tempfile::NamedTempFile;
use tempfile::TempDir;

fn create_test_app() -> (App, NamedTempFile) {
    let temp_file = NamedTempFile::new().unwrap();
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

#[test]
fn test_handle_text_input_char() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());

    handle_text_input_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.buffer, "a");

    handle_text_input_mode(&mut app, KeyCode::Char('b'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.buffer, "ab");
}

#[test]
fn test_handle_text_input_backspace() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = "test".to_string();
    app.data.input.cursor = 4;

    handle_text_input_mode(&mut app, KeyCode::Backspace, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.buffer, "tes");
}

#[test]
fn test_handle_text_input_delete() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = "test".to_string();
    app.data.input.cursor = 0;

    handle_text_input_mode(&mut app, KeyCode::Delete, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.buffer, "est");
}

#[test]
fn test_handle_text_input_cursor_movement() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = "test".to_string();
    app.data.input.cursor = 2;

    handle_text_input_mode(&mut app, KeyCode::Left, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.cursor, 1);

    handle_text_input_mode(&mut app, KeyCode::Right, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.cursor, 2);

    handle_text_input_mode(&mut app, KeyCode::Home, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.cursor, 0);

    handle_text_input_mode(&mut app, KeyCode::End, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.input.cursor, 4);
}

#[test]
fn test_handle_text_input_escape() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = "test".to_string();

    handle_text_input_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_escape_reconnect_prompt_clears_conflict() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReconnectPromptMode.into());
    app.data.spawn.worktree_conflict = Some(crate::app::WorktreeConflictInfo {
        title: "test".to_string(),
        branch: "test".to_string(),
        worktree_path: std::path::PathBuf::from("/tmp"),
        repo_root: std::path::PathBuf::from("/tmp"),
        prompt: None,
        existing_branch: None,
        existing_commit: None,
        current_branch: "main".to_string(),
        current_commit: "abc1234".to_string(),
        swarm_child_count: None,
    });

    handle_text_input_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE).unwrap();

    assert!(app.data.spawn.worktree_conflict.is_none());
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_enter_empty_creating_exits() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = String::new();

    handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_alt_enter_inserts_newline() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = "test".to_string();
    app.data.input.cursor = 4;

    handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::ALT).unwrap();
    assert!(app.data.input.buffer.contains('\n'));
}

#[test]
fn test_text_input_actions_covered_for_all_modes() {
    let mode_factories: [fn() -> AppMode; 8] = [
        || CreatingMode.into(),
        || PromptingMode.into(),
        || ChildPromptMode.into(),
        || BroadcastingMode.into(),
        || ReconnectPromptMode.into(),
        || TerminalPromptMode.into(),
        || CustomAgentCommandMode.into(),
        || SynthesisPromptMode.into(),
    ];

    for make_mode in mode_factories {
        let mode = make_mode();
        let is_reconnect_prompt = matches!(mode, AppMode::ReconnectPrompt(_));
        let (mut app, _temp) = create_test_app();
        app.apply_mode(mode);
        app.data.input.buffer = "hello world".to_string();
        app.data.input.cursor = app.data.input.buffer.len();

        // Delete word, then clear line.
        handle_text_input_mode(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL).unwrap();
        assert!(app.data.input.buffer.is_empty());

        // Insert, backspace, delete, and cursor movement.
        handle_text_input_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Backspace, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Char('b'), KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Delete, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Left, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Right, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Up, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Down, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::Home, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::End, KeyModifiers::NONE).unwrap();
        handle_text_input_mode(&mut app, KeyCode::F(12), KeyModifiers::NONE).unwrap();

        // Cancel (Esc) returns to Normal and clears any ReconnectPrompt conflict.
        if is_reconnect_prompt {
            app.apply_mode(ReconnectPromptMode.into());
            app.data.spawn.worktree_conflict = Some(crate::app::WorktreeConflictInfo {
                title: "test".to_string(),
                branch: "test".to_string(),
                worktree_path: std::path::PathBuf::from("/tmp"),
                repo_root: std::path::PathBuf::from("/tmp"),
                prompt: None,
                existing_branch: None,
                existing_commit: None,
                current_branch: "main".to_string(),
                current_commit: "abc1234".to_string(),
                swarm_child_count: None,
            });
        }

        handle_text_input_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.spawn.worktree_conflict.is_none());
    }
}

#[test]
fn test_text_input_submit_error_paths() {
    let _guard = crate::test_support::lock_mux_test_environment();
    let non_git_dir = TempDir::new().unwrap();

    // CreatingMode should succeed by creating a plain-directory agent.
    {
        let (mut app, _temp) = create_test_app();
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));
        app.apply_mode(CreatingMode.into());
        app.data.input.buffer = "agent".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, AppMode::normal());
        let agent = app.data.storage.iter().next().expect("Missing agent");
        assert_eq!(agent.workspace_kind, WorkspaceKind::PlainDir);
        let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
    }

    // PromptingMode should also succeed outside git.
    {
        let (mut app, _temp) = create_test_app();
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));
        app.apply_mode(PromptingMode.into());
        app.data.input.buffer = "prompt".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, AppMode::normal());
        let agent = app.data.storage.iter().next().expect("Missing agent");
        assert_eq!(agent.workspace_kind, WorkspaceKind::PlainDir);
        let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
    }

    // ChildPromptMode: force an error by pointing to a missing parent agent.
    {
        let (mut app, _temp) = create_test_app();
        app.data.spawn.spawning_under = Some(uuid::Uuid::new_v4());
        app.apply_mode(ChildPromptMode.into());
        app.data.input.buffer = "task".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    // BroadcastingMode: errors without a selected agent.
    {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(BroadcastingMode.into());
        app.data.input.buffer = "msg".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    // ReconnectPromptMode: errors without conflict info.
    {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ReconnectPromptMode.into());
        app.data.input.buffer = "new prompt".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    // TerminalPromptMode: errors without a selected agent.
    {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(TerminalPromptMode.into());
        app.data.input.buffer = "echo hi".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }))
        );
    }

    // Custom agent command: empty input should stay in mode and set a status message.
    {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CustomAgentCommandMode.into());
        app.data.input.buffer = "   ".to_string();
        app.data.input.cursor = app.data.input.buffer.len();
        handle_text_input_mode(&mut app, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, CustomAgentCommandMode.into());
    }
}

#[test]
fn test_handle_text_input_mode_propagates_dispatch_errors() {
    let mode_factories: [fn() -> AppMode; 8] = [
        || CreatingMode.into(),
        || PromptingMode.into(),
        || ChildPromptMode.into(),
        || BroadcastingMode.into(),
        || ReconnectPromptMode.into(),
        || TerminalPromptMode.into(),
        || CustomAgentCommandMode.into(),
        || SynthesisPromptMode.into(),
    ];

    crate::action::with_forced_text_input_dispatch_error_for_tests(|| {
        for make_mode in mode_factories {
            let (mut app, _temp) = create_test_app();
            let mode = make_mode();
            app.apply_mode(mode);

            let err = handle_text_input_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)
                .expect_err("Expected forced dispatch error");
            assert!(format!("{err}").contains("Forced text input dispatch error"));
        }
    });
}

#[test]
fn test_handle_text_input_mode_noops_when_not_in_text_input_mode() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(AppMode::normal());
    app.data.input.buffer = "hello".to_string();
    app.data.input.cursor = app.data.input.buffer.len();

    handle_text_input_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE).unwrap();

    assert_eq!(app.data.input.buffer, "hello");
    assert_eq!(app.data.input.cursor, 5);
}
