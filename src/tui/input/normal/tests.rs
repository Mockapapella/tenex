use super::*;
use crate::agent::{Agent, AgentRuntime, Storage};
use crate::app::Settings;
use crate::config::Config;
use crate::runtime;
use crate::state::{HelpMode, ScrollingMode};
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[test]
fn test_handle_normal_mode_ignores_actions_when_mode_not_supported() {
    let mut app = App::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    );
    app.apply_mode(HelpMode.into());

    handle_normal_mode(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL)
        .expect("handle normal mode");

    assert_eq!(app.mode, HelpMode.into());
}

#[test]
fn test_handle_normal_mode_ctrl_q_requires_control_modifier_to_exit_scrolling() {
    let mut app = App::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    );
    app.apply_mode(ScrollingMode.into());

    handle_normal_mode(&mut app, KeyCode::Char('q'), KeyModifiers::NONE)
        .expect("handle normal mode");

    assert_eq!(app.mode, ScrollingMode.into());
}

fn create_failing_docker_program_script() -> (TempDir, PathBuf) {
    let docker_dir = TempDir::new().expect("tempdir");
    let docker_path = docker_dir.path().join("docker");
    std::fs::write(&docker_path, "#!/usr/bin/env sh\nexit 1\n").expect("write docker script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&docker_path)
            .expect("metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&docker_path, perms).expect("chmod docker script");
    }
    (docker_dir, docker_path)
}

fn create_docker_root_agent(worktree_path: PathBuf) -> Agent {
    let mut agent = Agent::new(
        "root".to_string(),
        "codex".to_string(),
        "test-branch".to_string(),
        worktree_path,
    );
    agent.runtime = AgentRuntime::Docker;
    agent
}

#[test]
fn test_handle_normal_mode_propagates_dispatch_errors_in_normal_mode() {
    let (_docker_dir, docker_path) = create_failing_docker_program_script();
    runtime::with_docker_program_override_for_tests(docker_path, || {
        let state_file = NamedTempFile::new().expect("state file");
        let storage = Storage::with_path(state_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);

        let worktree_dir = TempDir::new().expect("worktree dir");
        app.data
            .storage
            .add(create_docker_root_agent(worktree_dir.path().to_path_buf()));

        let err = handle_normal_mode(&mut app, KeyCode::Char('t'), KeyModifiers::NONE)
            .expect_err("expected spawn terminal to fail");
        assert!(!err.to_string().is_empty());
    });
}

#[test]
fn test_handle_normal_mode_propagates_dispatch_errors_in_scrolling_mode() {
    let (_docker_dir, docker_path) = create_failing_docker_program_script();
    runtime::with_docker_program_override_for_tests(docker_path, || {
        let state_file = NamedTempFile::new().expect("state file");
        let storage = Storage::with_path(state_file.path().to_path_buf());
        let mut app = App::new(Config::default(), storage, Settings::default(), false);
        app.apply_mode(ScrollingMode.into());

        let worktree_dir = TempDir::new().expect("worktree dir");
        app.data
            .storage
            .add(create_docker_root_agent(worktree_dir.path().to_path_buf()));

        let err = handle_normal_mode(&mut app, KeyCode::Char('t'), KeyModifiers::NONE)
            .expect_err("expected spawn terminal to fail");
        assert!(!err.to_string().is_empty());
    });
}
