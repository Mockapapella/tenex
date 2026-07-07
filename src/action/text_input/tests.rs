use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::app::{AgentProgram, AgentRole, WorktreeConflictInfo};
use crate::config::Config;
use anyhow::anyhow;
use tempfile::{NamedTempFile, TempDir};

fn error_message(mode: AppMode) -> Option<String> {
    match mode {
        AppMode::ErrorModal(state) => Some(state.message),
        _ => None,
    }
}

#[test]
fn test_ok_or_error_modal_returns_error_modal_for_err() {
    let ok_mode = ok_or_error_modal(Ok(AppMode::normal())).unwrap();
    assert_eq!(error_message(ok_mode), None);

    let modal_mode = ok_or_error_modal(Err(anyhow!("boom"))).unwrap();
    let message = error_message(modal_mode).unwrap();
    assert!(message.starts_with("Failed:"));
    assert!(message.contains("boom"));
}

#[derive(Clone, Copy)]
struct TestState;

impl From<TestState> for AppMode {
    fn from(_value: TestState) -> Self {
        Self::normal()
    }
}

impl ValidIn<TestState> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CharInputAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        if self.0 == 'x' {
            return Err(anyhow!("boom"));
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for BackspaceAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for DeleteAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CursorRightAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CursorUpAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CursorDownAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for CursorEndAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for ClearLineAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<TestState> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

#[test]
fn test_dispatch_text_input_mode_propagates_errors_from_actions() {
    let mut app = App::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    );
    let error =
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Char('x'), KeyModifiers::NONE)
            .unwrap_err();
    assert!(format!("{error}").contains("boom"));
}

#[test]
fn test_dispatch_text_input_mode_forced_error_covers_test_state_branch() {
    with_forced_text_input_dispatch_error_for_tests(|| {
        let mut app = App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );

        let error =
            dispatch_text_input_mode(&mut app, TestState, KeyCode::Char('a'), KeyModifiers::NONE)
                .unwrap_err();
        assert!(format!("{error}").contains("Forced text input dispatch error"));
    });
}

#[test]
fn test_dispatch_text_input_mode_applies_mode_on_success() {
    let mut app = App::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    );

    dispatch_text_input_mode(&mut app, TestState, KeyCode::Enter, KeyModifiers::ALT).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Enter, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Esc, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(
        &mut app,
        TestState,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )
    .unwrap();
    dispatch_text_input_mode(
        &mut app,
        TestState,
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )
    .unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Backspace, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Delete, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Left, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Right, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Up, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Down, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::Home, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::End, KeyModifiers::NONE).unwrap();
    dispatch_text_input_mode(&mut app, TestState, KeyCode::F(13), KeyModifiers::NONE).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_dispatch_text_input_mode_covers_modifier_branches_for_real_modes() {
    fn exercise<State>(state: State)
    where
        State: Copy,
        AppMode: From<State>,
        CancelAction: ValidIn<State, NextState = AppMode>,
        SubmitAction: ValidIn<State, NextState = AppMode>,
        CharInputAction: ValidIn<State, NextState = AppMode>,
        BackspaceAction: ValidIn<State, NextState = AppMode>,
        DeleteAction: ValidIn<State, NextState = AppMode>,
        CursorLeftAction: ValidIn<State, NextState = AppMode>,
        CursorRightAction: ValidIn<State, NextState = AppMode>,
        CursorUpAction: ValidIn<State, NextState = AppMode>,
        CursorDownAction: ValidIn<State, NextState = AppMode>,
        CursorHomeAction: ValidIn<State, NextState = AppMode>,
        CursorEndAction: ValidIn<State, NextState = AppMode>,
        ClearLineAction: ValidIn<State, NextState = AppMode>,
        DeleteWordAction: ValidIn<State, NextState = AppMode>,
    {
        let mut app = App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );

        dispatch_text_input_mode(&mut app, state, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Enter, KeyModifiers::ALT).unwrap();

        dispatch_text_input_mode(&mut app, state, KeyCode::Char('u'), KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Char('u'), KeyModifiers::CONTROL)
            .unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Char('U'), KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Char('U'), KeyModifiers::CONTROL)
            .unwrap();

        dispatch_text_input_mode(&mut app, state, KeyCode::Char('w'), KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Char('w'), KeyModifiers::CONTROL)
            .unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Char('W'), KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, state, KeyCode::Char('W'), KeyModifiers::CONTROL)
            .unwrap();
    }

    exercise(CreatingMode);
    exercise(PromptingMode);
    exercise(ChildPromptMode);
    exercise(BroadcastingMode);
    exercise(ReconnectPromptMode);
    exercise(TerminalPromptMode);
    exercise(CustomAgentCommandMode);
    exercise(SynthesisPromptMode);
}

fn create_test_data() -> (AppData, NamedTempFile) {
    let temp_file = NamedTempFile::new().expect("temp state file should be created");
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        AppData::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

#[test]
fn test_submit_action_prompting_mode_covers_prompt_branches() {
    let docker_dir = TempDir::new().expect("docker script dir");
    let docker_path = docker_dir.path().join("docker");
    std::fs::write(&docker_path, "#!/usr/bin/env sh\nexit 1\n").expect("write docker script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = std::fs::metadata(&docker_path)
            .expect("read docker script metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&docker_path, perms).expect("chmod docker script");
    }

    crate::runtime::with_docker_program_override_for_tests(docker_path, || {
        let cwd_dir = TempDir::new().expect("cwd dir");
        for buffer in ["", "hello"] {
            let (mut data, _temp) = create_test_data();
            data.settings.docker_for_new_roots = true;
            data.cwd_project_root = Some(cwd_dir.path().to_path_buf());
            data.input.buffer = buffer.to_string();

            let next = SubmitAction
                .execute(PromptingMode, &mut data)
                .expect("submit prompting mode");
            let message = error_message(next).expect("expected error modal");
            assert!(message.starts_with("Failed:"));
        }
    });
}

#[test]
fn test_submit_action_child_prompt_mode_covers_prompt_branches() {
    for buffer in ["", "do the thing"] {
        let (mut data, _temp) = create_test_data();
        data.spawn.spawning_under = Some(uuid::Uuid::new_v4());
        data.input.buffer = buffer.to_string();

        let next = SubmitAction
            .execute(ChildPromptMode, &mut data)
            .expect("submit child prompt mode");
        let message = error_message(next).expect("expected error modal");
        assert!(message.starts_with("Failed:"));
    }
}

#[test]
fn test_submit_action_reconnect_prompt_mode_covers_prompt_branches() {
    let docker_dir = TempDir::new().expect("docker script dir");
    let docker_path = docker_dir.path().join("docker");
    std::fs::write(&docker_path, "#!/usr/bin/env sh\nexit 1\n").expect("write docker script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = std::fs::metadata(&docker_path)
            .expect("read docker script metadata")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&docker_path, perms).expect("chmod docker script");
    }

    crate::runtime::with_docker_program_override_for_tests(docker_path, || {
        let worktree_dir = TempDir::new().expect("worktree dir");
        let repo_root = TempDir::new().expect("repo root");

        for buffer in ["", "prompt"] {
            let (mut data, _temp) = create_test_data();
            data.settings.docker_for_new_roots = true;
            data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
                title: "test agent".to_string(),
                prompt: None,
                branch: "agent/test".to_string(),
                worktree_path: worktree_dir.path().to_path_buf(),
                repo_root: repo_root.path().to_path_buf(),
                existing_branch: None,
                existing_commit: None,
                current_branch: "master".to_string(),
                current_commit: "deadbeef".to_string(),
                swarm_child_count: None,
            });
            data.input.buffer = buffer.to_string();

            let next = SubmitAction
                .execute(ReconnectPromptMode, &mut data)
                .expect("submit reconnect prompt mode");
            let message = error_message(next).expect("expected error modal");
            assert!(message.starts_with("Failed:"));
        }
    });
}

#[test]
fn test_submit_action_terminal_prompt_mode_covers_command_branches() {
    for buffer in ["", "git status"] {
        let (mut data, _temp) = create_test_data();
        data.input.buffer = buffer.to_string();

        let next = SubmitAction
            .execute(TerminalPromptMode, &mut data)
            .expect("submit terminal prompt mode");
        let message = error_message(next).expect("expected error modal");
        assert!(message.starts_with("Failed:"));
    }
}

#[test]
fn test_submit_action_custom_agent_command_mode_rejects_empty_input() {
    let (mut data, _temp) = create_test_data();
    data.model_selector.role = AgentRole::Default;
    data.input.buffer = "   ".to_string();

    let next = SubmitAction
        .execute(CustomAgentCommandMode, &mut data)
        .expect("submit custom agent command");
    assert_eq!(
        std::mem::discriminant(&next),
        std::mem::discriminant(&AppMode::CustomAgentCommand(CustomAgentCommandMode))
    );
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Custom agent command cannot be empty")
    );
}

#[test]
fn test_submit_action_custom_agent_command_mode_sets_custom_command_for_each_role() {
    let temp_dir = TempDir::new().expect("temp settings dir");
    Settings::set_test_path_override(temp_dir.path().join("settings.json"))
        .expect("set test settings path override");

    let (mut data, _temp) = create_test_data();
    let command = "echo hello";

    for role in AgentRole::ALL {
        data.settings = Settings::default();
        data.model_selector.role = *role;
        data.input.buffer = command.to_string();

        let next = SubmitAction
            .execute(CustomAgentCommandMode, &mut data)
            .expect("submit custom agent command");
        assert_eq!(next, AppMode::normal());

        match *role {
            AgentRole::Default => {
                assert_eq!(data.settings.custom_agent_command, command);
                assert_eq!(data.settings.agent_program, AgentProgram::Custom);
            }
            AgentRole::Planner => {
                assert_eq!(data.settings.planner_custom_agent_command, command);
                assert_eq!(data.settings.planner_agent_program, AgentProgram::Custom);
            }
            AgentRole::Review => {
                assert_eq!(data.settings.review_custom_agent_command, command);
                assert_eq!(data.settings.review_agent_program, AgentProgram::Custom);
            }
        }

        let expected_status = format!("{} set to custom", role.menu_label());
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some(expected_status.as_str())
        );
    }
}

#[test]
fn test_submit_action_custom_agent_command_mode_returns_error_modal_when_save_fails() {
    let temp_file = NamedTempFile::new().expect("temp state file should be created");
    Settings::set_test_path_override(temp_file.path().join("settings.json"))
        .expect("set test settings path override");

    let (mut data, _temp) = create_test_data();
    data.model_selector.role = AgentRole::Default;
    data.input.buffer = "echo hello".to_string();

    let next = SubmitAction
        .execute(CustomAgentCommandMode, &mut data)
        .expect("submit custom agent command");
    let message = error_message(next).expect("expected error modal");
    assert!(message.starts_with("Failed to save settings:"));
}

#[test]
fn test_submit_action_synthesis_prompt_mode_covers_prompt_branches() {
    for buffer in ["   ", "follow these steps"] {
        let (mut data, _temp) = create_test_data();
        data.input.buffer = buffer.to_string();

        let next = SubmitAction
            .execute(SynthesisPromptMode, &mut data)
            .expect("submit synthesis prompt mode");
        let message = error_message(next).expect("expected error modal");
        assert_eq!(message, "No agent selected");
    }
}
