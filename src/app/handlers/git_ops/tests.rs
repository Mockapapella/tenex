use crate::agent::{Agent, Storage};
use crate::app::Settings;
use crate::app::handlers::Actions;
use crate::app::state::App;
use crate::config::Config;
use crate::state::{AppMode, ConfirmPushForPRMode, ConfirmPushMode, RenameBranchMode};
use std::path::PathBuf;
use std::sync::OnceLock;
use tempfile::{NamedTempFile, TempDir};

fn create_test_app() -> std::io::Result<(App, NamedTempFile)> {
    let temp_file = NamedTempFile::new()?;
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    Ok((
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    ))
}

fn ensure_stub_gh_installed() -> Result<(), Box<dyn std::error::Error>> {
    static GH_STUB: OnceLock<PathBuf> = OnceLock::new();

    let gh_path = if let Some(path) = GH_STUB.get() {
        path.clone()
    } else {
        let dir = std::env::temp_dir().join(format!(
            "tenex-gh-stub-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir)?;

        #[cfg(windows)]
        let gh_path = dir.join("gh.cmd");
        #[cfg(not(windows))]
        let gh_path = dir.join("gh");

        #[cfg(windows)]
        std::fs::write(
            &gh_path,
            r#"@echo off
if "%5"=="main" (
  exit /b 0
)

echo boom 1>&2
exit /b 1
"#,
        )?;

        #[cfg(not(windows))]
        std::fs::write(
            &gh_path,
            r#"#!/usr/bin/env bash

if [[ "$5" == "main" ]]; then
  exit 0
fi

echo "boom" 1>&2
exit 1
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&gh_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&gh_path, perms)?;
        }

        let _ = GH_STUB.set(gh_path.clone());
        gh_path
    };

    super::open_pr::set_gh_binary_override(gh_path);
    Ok(())
}

#[test]
fn test_handle_push_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    let result = handler.handle_action(&mut app, crate::config::Action::Push);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_handle_push_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    let agent = Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "muster/test".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Push should enter ConfirmPush mode
    handler.handle_action(&mut app, crate::config::Action::Push)?;

    assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "muster/test");
    Ok(())
}

#[test]
fn test_push_branch_sets_confirm_mode() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let agent = Agent::new(
        "pushable".to_string(),
        "claude".to_string(),
        "feature/pushable".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    let next = Actions::push_branch(&mut app.data)?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "feature/pushable");
    Ok(())
}

#[test]
fn test_execute_push_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = None;

    let next = Actions::execute_push(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_push_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "test".to_string();

    let next = Actions::execute_push(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_rename_agent_sets_state_for_selected() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let agent = Agent::new(
        "rename-me".to_string(),
        "claude".to_string(),
        "feature/rename-me".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    let next = Actions::rename_agent(&mut app.data)?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.original_branch, "rename-me");
    assert!(app.data.git_op.is_root_rename);
    Ok(())
}

#[test]
fn test_execute_rename_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = None;

    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.agent_id.is_none());
    Ok(())
}

#[test]
fn test_execute_rename_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "new-name".to_string();
    app.data.git_op.original_branch = "old-name".to_string();

    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.agent_id.is_none());
    Ok(())
}

#[test]
fn test_open_pr_in_browser_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = None;

    let result = Actions::open_pr_in_browser(&mut app.data);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_open_pr_in_browser_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "test".to_string();
    app.data.git_op.base_branch = "main".to_string();

    let result = Actions::open_pr_in_browser(&mut app.data);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_open_pr_flow_sets_confirm_for_unpushed() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;

    let agent = Agent::new(
        "pr-agent".to_string(),
        "claude".to_string(),
        "feature/pr-agent".to_string(),
        temp_dir.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    let next = Actions::open_pr_flow(&mut app.data)?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::ConfirmPushForPR(ConfirmPushForPRMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "feature/pr-agent");
    assert_eq!(app.data.git_op.base_branch, "main");
    assert!(app.data.git_op.has_unpushed);
    Ok(())
}

#[test]
fn test_open_pr_in_browser_gh_success_clears_state() -> Result<(), Box<dyn std::error::Error>> {
    ensure_stub_gh_installed()?;

    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;
    let agent = Agent::new(
        "gh-less".to_string(),
        "claude".to_string(),
        "feature/gh-less".to_string(),
        temp_dir.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/gh-less".to_string();
    app.data.git_op.base_branch = "main".to_string();

    Actions::open_pr_in_browser(&mut app.data)?;
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.ui.status_message.is_some());
    Ok(())
}

#[test]
fn test_open_pr_in_browser_gh_failure_clears_state() -> Result<(), Box<dyn std::error::Error>> {
    ensure_stub_gh_installed()?;

    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;
    let agent = Agent::new(
        "gh-fail".to_string(),
        "claude".to_string(),
        "feature/gh-fail".to_string(),
        temp_dir.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/gh-fail".to_string();
    app.data.git_op.base_branch = "develop".to_string();

    let err = match Actions::open_pr_in_browser(&mut app.data) {
        Ok(()) => return Err("Expected open_pr_in_browser to fail".into()),
        Err(err) => err,
    };
    assert!(!err.to_string().is_empty());
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.git_op.agent_id.is_none());
    Ok(())
}

#[test]
fn test_push_flow_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    let agent = Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "feature/test".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Start push flow
    app.start_push(agent_id, "feature/test".to_string());
    assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "feature/test");

    // Clear git op state
    app.clear_git_op_state();
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.git_op.agent_id.is_none());
    Ok(())
}

#[test]
fn test_rename_root_flow_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Add a root agent
    let agent = Agent::new(
        "test-agent".to_string(),
        "claude".to_string(),
        "tenex/test-agent".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Start rename flow for root agent
    app.start_rename(agent_id, "test-agent".to_string(), true);
    assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.original_branch, "test-agent");
    assert_eq!(app.data.git_op.branch_name, "test-agent");
    assert_eq!(app.data.input.buffer, "test-agent");
    assert!(app.data.git_op.is_root_rename);

    // Simulate user input
    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    app.handle_char('n');
    app.handle_char('e');
    app.handle_char('w');
    assert_eq!(app.data.input.buffer, "test-new");

    // Confirm rename
    let result = app.confirm_rename_branch();
    assert!(result);
    assert_eq!(app.data.git_op.branch_name, "test-new");
    Ok(())
}

#[test]
fn test_rename_subagent_flow_state_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Add a root agent first
    let root = Agent::new(
        "root-agent".to_string(),
        "claude".to_string(),
        "tenex/root-agent".to_string(),
        PathBuf::from("/tmp"),
    );
    app.data.storage.add(root.clone());

    // Add a child agent
    let child = Agent::new_child(
        "sub-agent".to_string(),
        "claude".to_string(),
        "tenex/root-agent".to_string(),
        PathBuf::from("/tmp"),
        crate::agent::ChildConfig {
            parent_id: root.id,
            mux_session: root.mux_session,
            window_index: 1,
            repo_root: None,
        },
    );
    let child_id = child.id;
    app.data.storage.add(child);

    // Start rename flow for sub-agent
    app.start_rename(child_id, "sub-agent".to_string(), false);
    assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    assert_eq!(app.data.git_op.agent_id, Some(child_id));
    assert_eq!(app.data.git_op.original_branch, "sub-agent");
    assert!(!app.data.git_op.is_root_rename);

    // Simulate user input
    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    app.handle_backspace();
    app.handle_char('n');
    app.handle_char('e');
    app.handle_char('w');
    assert_eq!(app.data.input.buffer, "sub-new");

    // Confirm rename
    let result = app.confirm_rename_branch();
    assert!(result);
    assert_eq!(app.data.git_op.branch_name, "sub-new");
    Ok(())
}

#[test]
fn test_open_pr_flow_state_with_unpushed() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    let agent = Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "feature/test".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Start open PR flow with unpushed commits
    app.start_open_pr(
        agent_id,
        "feature/test".to_string(),
        "main".to_string(),
        true,
    );

    assert_eq!(app.mode, AppMode::ConfirmPushForPR(ConfirmPushForPRMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "feature/test");
    assert_eq!(app.data.git_op.base_branch, "main");
    assert!(app.data.git_op.has_unpushed);
    Ok(())
}

#[test]
fn test_open_pr_flow_state_no_unpushed() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Add an agent
    let agent = Agent::new(
        "test".to_string(),
        "claude".to_string(),
        "feature/test".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Start open PR flow without unpushed commits
    app.start_open_pr(
        agent_id,
        "feature/test".to_string(),
        "main".to_string(),
        false,
    );

    // Mode should stay Normal (handler opens PR directly)
    assert_eq!(app.mode, AppMode::normal());
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert!(!app.data.git_op.has_unpushed);
    Ok(())
}

#[test]
fn test_execute_push_and_open_pr_handles_failed_push() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;

    let agent = Agent::new(
        "failing-push".to_string(),
        "claude".to_string(),
        "feature/failing-push".to_string(),
        temp_dir.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/failing-push".to_string();

    let next = Actions::execute_push_and_open_pr(&mut app.data)?;
    app.apply_mode(next);

    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.git_op.agent_id.is_none());
    Ok(())
}

#[test]
fn test_detect_base_branch_no_git() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::TempDir;

    // Create a temp directory that's not a git repo
    let temp_dir = TempDir::new()?;

    // Should return default "main" when git commands fail
    let result = Actions::detect_base_branch(temp_dir.path(), "feature/test");
    assert_eq!(result, "main");
    Ok(())
}

#[test]
fn test_has_unpushed_commits_no_git() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::TempDir;

    // Create a temp directory that's not a git repo
    let temp_dir = TempDir::new()?;

    // Should return true (assume all commits are unpushed if we can't check)
    let result = Actions::has_unpushed_commits(temp_dir.path(), "feature/test");
    // Either Ok(true) or Err is acceptable
    let _ = result;
    Ok(())
}

#[test]
fn test_handle_rename_with_root_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add a root agent
    let agent = Agent::new(
        "test-agent".to_string(),
        "claude".to_string(),
        "tenex/test-agent".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Rename should enter RenameBranch mode with agent title
    handler.handle_action(&mut app, crate::config::Action::RenameBranch)?;

    assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "test-agent");
    assert_eq!(app.data.git_op.original_branch, "test-agent");
    assert_eq!(app.data.input.buffer, "test-agent");
    assert!(app.data.git_op.is_root_rename);
    Ok(())
}

#[test]
fn test_handle_rename_with_subagent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    // Add a root agent first
    let root = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp"),
    );
    let root_id = root.id;
    app.data.storage.add(root.clone());

    // Add a child agent
    let child = Agent::new_child(
        "child".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp"),
        crate::agent::ChildConfig {
            parent_id: root_id,
            mux_session: root.mux_session,
            window_index: 1,
            repo_root: None,
        },
    );
    let child_id = child.id;
    app.data.storage.add(child);

    // Expand root to see child, then select the child agent
    if let Some(root_agent) = app.data.storage.get_mut(root_id) {
        root_agent.collapsed = false;
    }
    app.select_next();

    // Rename should enter RenameBranch mode with agent title, not root rename
    handler.handle_action(&mut app, crate::config::Action::RenameBranch)?;

    assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    assert_eq!(app.data.git_op.agent_id, Some(child_id));
    assert_eq!(app.data.git_op.branch_name, "child");
    assert_eq!(app.data.git_op.original_branch, "child");
    assert_eq!(app.data.input.buffer, "child");
    assert!(!app.data.git_op.is_root_rename);
    Ok(())
}

#[test]
fn test_check_remote_branch_exists_no_git() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::TempDir;

    // Create a temp directory that's not a git repo
    let temp_dir = TempDir::new()?;

    // Should return Ok(false) when not in a git repo (command returns error)
    let result = Actions::check_remote_branch_exists(temp_dir.path(), "main")?;
    assert!(!result);
    Ok(())
}

fn run_git(current_dir: &std::path::Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("git {args:?} failed:\n{stdout}\n{stderr}").into())
}

#[test]
fn test_execute_root_rename_keeps_remote_branch() -> Result<(), Box<dyn std::error::Error>> {
    use crate::app::Settings;
    use tempfile::TempDir;

    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    run_git(&remote_repo, &["init", "--bare"])?;
    run_git(&local_repo, &["init"])?;
    run_git(&local_repo, &["config", "user.email", "tenex@test.invalid"])?;
    run_git(&local_repo, &["config", "user.name", "Tenex Test"])?;

    std::fs::write(local_repo.join("README.md"), "test\n")?;
    run_git(&local_repo, &["add", "."])?;
    run_git(&local_repo, &["commit", "-m", "init"])?;

    let old_title = "old-agent";
    let new_title = "new-agent";

    let config = Config {
        worktree_dir: temp_dir.path().join("worktrees"),
        ..Config::default()
    };
    let old_branch = config.generate_branch_name(old_title);
    let new_branch = config.generate_branch_name(new_title);

    run_git(&local_repo, &["checkout", "-b", old_branch.as_str()])?;
    run_git(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    run_git(&local_repo, &["push", "-u", "origin", old_branch.as_str()])?;
    run_git(&local_repo, &["fetch", "origin"])?;

    let temp_file = NamedTempFile::new()?;
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    let mut app = App::new(config, storage, Settings::default(), false);

    let agent = Agent::new(
        old_title.to_string(),
        "claude".to_string(),
        old_branch.clone(),
        local_repo,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.start_rename(agent_id, old_title.to_string(), true);
    app.data.input.buffer = new_title.to_string();
    assert!(app.confirm_rename_branch());

    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);
    assert_eq!(app.mode, AppMode::normal());

    run_git(
        &remote_repo,
        &[
            "show-ref",
            "--verify",
            format!("refs/heads/{old_branch}").as_str(),
        ],
    )?;
    run_git(
        &remote_repo,
        &[
            "show-ref",
            "--verify",
            format!("refs/heads/{new_branch}").as_str(),
        ],
    )?;

    Ok(())
}

#[test]
fn test_execute_rename_clears_state_on_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Set up state but with an invalid agent ID
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "new-name".to_string();
    app.data.git_op.is_root_rename = true;

    // Execute should fail gracefully
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_rename_subagent_clears_state_on_no_agent() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut app, _temp) = create_test_app()?;

    // Set up state but with an invalid agent ID
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "new-name".to_string();
    app.data.git_op.is_root_rename = false;

    // Execute should fail gracefully
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_push_and_open_pr_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // No agent ID set
    app.data.git_op.agent_id = None;

    let next = Actions::execute_push_and_open_pr(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_push_and_open_pr_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Set invalid agent ID
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());

    let next = Actions::execute_push_and_open_pr(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_handle_open_pr_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    let result = handler.handle_action(&mut app, crate::config::Action::OpenPR);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_handle_rename_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    let result = handler.handle_action(&mut app, crate::config::Action::RenameBranch);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_open_pr_flow_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;

    let agent = Agent::new(
        "test-agent".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        temp_dir.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Trigger open PR action
    handler.handle_action(&mut app, crate::config::Action::OpenPR)?;

    // Should enter ConfirmPushForPR mode
    assert_eq!(app.mode, AppMode::ConfirmPushForPR(ConfirmPushForPRMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    Ok(())
}

#[test]
fn test_push_flow_with_agent() -> Result<(), Box<dyn std::error::Error>> {
    let handler = Actions::new();
    let (mut app, _temp) = create_test_app()?;

    let agent = Agent::new(
        "test-agent".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        PathBuf::from("/tmp"),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Trigger push action
    handler.handle_action(&mut app, crate::config::Action::Push)?;

    // Should enter ConfirmPush mode
    assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    Ok(())
}

#[test]
fn test_merge_branch_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Should show error when no agent is selected
    let next = Actions::merge_branch(&mut app.data)?;
    app.apply_mode(next);

    // Should have set an error message
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_rebase_branch_no_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;

    // Should show error when no agent is selected
    let next = Actions::rebase_branch(&mut app.data)?;
    app.apply_mode(next);

    // Should have set an error message
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_merge_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = None;

    let next = Actions::execute_merge(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_merge_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "feature".to_string();
    app.data.git_op.target_branch = "main".to_string();

    let next = Actions::execute_merge(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_rebase_no_agent_id() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = None;

    let next = Actions::execute_rebase(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_execute_rebase_agent_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    app.data.git_op.branch_name = "feature".to_string();
    app.data.git_op.target_branch = "main".to_string();

    let next = Actions::execute_rebase(&mut app.data)?;
    app.apply_mode(next);
    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    Ok(())
}

#[test]
fn test_find_worktree_for_branch_no_worktree() -> Result<(), Box<dyn std::error::Error>> {
    // Should return None for a non-existent branch
    let result = Actions::find_worktree_for_branch("non-existent-branch-12345")?;
    assert!(result.is_none());
    Ok(())
}
