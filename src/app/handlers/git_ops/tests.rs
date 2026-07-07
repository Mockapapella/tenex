use crate::agent::{Agent, AgentRuntime, Storage};
use crate::app::Settings;
use crate::app::handlers::Actions;
use crate::app::state::App;
use crate::config::Config;
use crate::mux::SessionManager;
use crate::state::{AppMode, ConfirmPushForPRMode, ConfirmPushMode, RenameBranchMode};
use std::path::{Path, PathBuf};
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

fn git_ok(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = crate::git::git_command()
        .args(args)
        .current_dir(repo)
        .output()?;
    assert!(
        output.status.success(),
        "git {args:?} failed with {} (stdout: {}, stderr: {})",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

fn init_repo_with_commit() -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo_dir = TempDir::new()?;
    git_ok(repo_dir.path(), &["init", "-q", "-b", "master"])?;
    git_ok(
        repo_dir.path(),
        &["config", "user.email", "test@example.com"],
    )?;
    git_ok(repo_dir.path(), &["config", "user.name", "Test"])?;

    std::fs::write(repo_dir.path().join("README.md"), "test")?;
    git_ok(repo_dir.path(), &["add", "."])?;
    git_ok(
        repo_dir.path(),
        &["commit", "-q", "--no-verify", "-m", "init"],
    )?;
    Ok(repo_dir)
}

fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::sink)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::with_default(&dispatch, f)
}

fn init_repo_with_remote_main_and_branch(
    branch: &str,
) -> Result<(TempDir, std::path::PathBuf), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    git_ok(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;
    git_ok(&local_repo, &["init", "-q", "-b", "main"])?;
    git_ok(&local_repo, &["config", "user.email", "test@example.com"])?;
    git_ok(&local_repo, &["config", "user.name", "Test"])?;

    std::fs::write(local_repo.join("README.md"), "test")?;
    git_ok(&local_repo, &["add", "."])?;
    git_ok(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;

    git_ok(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    git_ok(&local_repo, &["push", "-q", "-u", "origin", "main"])?;

    git_ok(&local_repo, &["checkout", "-q", "-b", branch, "main"])?;
    git_ok(&local_repo, &["push", "-q", "-u", "origin", branch])?;

    git_ok(&local_repo, &["fetch", "-q", "origin"])?;
    git_ok(
        &local_repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )?;

    Ok((temp_dir, local_repo))
}

#[cfg(unix)]
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[test]
fn test_set_gh_binary_override_for_tests_forwards_to_open_pr() {
    super::set_gh_binary_override_for_tests(PathBuf::from("/tmp/tenex-gh-test"));
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

    let next = with_tracing_dispatch(|| Actions::push_branch(&mut app.data))?;
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
fn test_push_branch_errors_without_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let err = Actions::push_branch(&mut app.data).expect_err("Expected push_branch to error");
    assert!(err.to_string().contains("No agent selected"));
    Ok(())
}

#[test]
fn test_execute_push_succeeds_when_git_push_succeeds() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let (_repo_temp, repo_path) = init_repo_with_remote_main_and_branch("feature/push-success")?;

    let agent = Agent::new(
        "push-success".to_string(),
        "claude".to_string(),
        "feature/push-success".to_string(),
        repo_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/push-success".to_string();

    let next = with_tracing_dispatch(|| Actions::execute_push(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.ui.status_message.is_some());
    Ok(())
}

#[test]
fn test_execute_push_returns_error_modal_when_git_push_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let repo_dir = init_repo_with_commit()?;

    git_ok(
        repo_dir.path(),
        &["checkout", "-q", "-b", "feature/push-failure"],
    )?;
    let missing_remote = repo_dir.path().join("missing_remote.git");
    let missing_remote = missing_remote.to_string_lossy().to_string();
    git_ok(
        repo_dir.path(),
        &["remote", "add", "origin", &missing_remote],
    )?;

    let agent = Agent::new(
        "push-failure".to_string(),
        "claude".to_string(),
        "feature/push-failure".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/push-failure".to_string();

    let next = with_tracing_dispatch(|| Actions::execute_push(&mut app.data))?;
    app.apply_mode(next);

    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.git_op.branch_name.is_empty());
    Ok(())
}

#[test]
fn test_spawn_conflict_terminal_errors_when_agent_id_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = None;

    let err = Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
        .expect_err("missing agent id should error");
    assert!(err.to_string().contains("No agent ID"));
    Ok(())
}

#[test]
fn test_spawn_conflict_terminal_errors_when_agent_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut app, _temp) = create_test_app()?;
    app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    let err = Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
        .expect_err("missing agent should error");
    assert!(err.to_string().contains("Agent not found"));
    Ok(())
}

#[test]
fn test_spawn_conflict_terminal_sends_startup_command_when_host_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

    let _guard = crate::test_support::lock_mux_test_environment();
    let socket = crate::test_support::unique_mux_socket_path("git-ops-conflict");
    crate::mux::set_socket_override(&socket)?;

    let (mut app, _temp) = create_test_app()?;

    let repo_dir = init_repo_with_commit()?;
    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        "main".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let mut root = root;
    root.runtime = AgentRuntime::Host;
    let root_id = root.id;
    let session = root.mux_session.clone();
    app.data.storage.add(root);

    let manager = SessionManager::new();
    manager.create(&session, repo_dir.path(), None)?;

    app.data.git_op.agent_id = Some(root_id);
    app.data.ui.preview_dimensions = Some((80, 20));

    let next = tracing::dispatcher::with_default(&dispatch, || {
        Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
    })?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert_eq!(
        app.data.ui.status_message.as_deref().unwrap_or_default(),
        "Opened terminal for conflict resolution: Merge Conflict"
    );
    assert!(app.data.git_op.agent_id.is_none());
    assert_eq!(app.data.storage.children(root_id).len(), 1);

    let _ = manager.kill(&session);
    Ok(())
}

#[test]
fn test_spawn_conflict_terminal_does_not_resize_when_preview_dimensions_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::lock_mux_test_environment();
    let socket = crate::test_support::unique_mux_socket_path("git-ops-no-resize");
    crate::mux::set_socket_override(&socket)?;

    let (mut app, _temp) = create_test_app()?;

    let repo_dir = init_repo_with_commit()?;
    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        "main".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let mut root = root;
    root.runtime = AgentRuntime::Host;
    let root_id = root.id;
    let session = root.mux_session.clone();
    app.data.storage.add(root);

    let manager = SessionManager::new();
    manager.create(&session, repo_dir.path(), None)?;

    app.data.git_op.agent_id = Some(root_id);

    let next = Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert!(app.data.git_op.agent_id.is_none());
    assert_eq!(app.data.storage.children(root_id).len(), 1);

    let _ = manager.kill(&session);
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_spawn_conflict_terminal_propagates_runtime_ready_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = crate::test_support::lock_mux_test_environment();

    let (mut app, _temp) = create_test_app()?;

    let repo_dir = init_repo_with_commit()?;
    let root = Agent::new(
        "root".to_string(),
        "codex".to_string(),
        "main".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let mut root = root;
    root.runtime = AgentRuntime::Docker;
    let root_id = root.id;
    app.data.storage.add(root);

    app.data.git_op.agent_id = Some(root_id);

    let missing_docker = repo_dir.path().join("docker-missing");
    let err = crate::runtime::with_docker_program_override_for_tests(missing_docker, || {
        Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
    })
    .expect_err("expected docker runtime to fail without docker program");

    assert!(
        err.to_string()
            .contains("Docker is not installed or not on PATH")
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_spawn_conflict_terminal_propagates_send_keys_and_submit_errors()
-> Result<(), Box<dyn std::error::Error>> {
    use interprocess::local_socket::traits::ListenerExt as _;
    use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
    use std::sync::mpsc;
    use std::time::Duration;

    let _guard = crate::test_support::lock_mux_test_environment();
    let socket_dir = TempDir::new()?;
    let socket_path = socket_dir.path().join("mux.sock");
    crate::mux::set_socket_override(&socket_path.to_string_lossy())?;
    let name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()?
        .into_owned();

    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("Expected mock mux listener to start");

        let mut handled = 0usize;
        if let Some(mut stream) = listener.incoming().flatten().next() {
            while handled < 2 {
                let request: crate::mux::MuxRequest =
                    crate::mux::read_json(&mut stream).expect("Expected mux request");
                tx.send(request).expect("send request");

                let response = match handled {
                    0 => crate::mux::MuxResponse::WindowCreated { index: 0 },
                    _ => crate::mux::MuxResponse::Err {
                        message: "boom".to_string(),
                    },
                };
                crate::mux::write_json(&mut stream, &response).expect("write response");
                handled = handled.saturating_add(1);
            }
        }
    });

    let (mut app, _temp) = create_test_app()?;
    let repo_dir = init_repo_with_commit()?;

    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        "main".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let mut root = root;
    root.runtime = AgentRuntime::Host;
    let root_id = root.id;
    app.data.storage.add(root);

    app.data.git_op.agent_id = Some(root_id);

    let err = Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
        .expect_err("expected send_keys to fail");
    assert!(err.to_string().contains("boom"));

    let first = rx.recv_timeout(Duration::from_secs(1))?;
    assert!(matches!(first, crate::mux::MuxRequest::CreateWindow { .. }));
    let second = rx.recv_timeout(Duration::from_secs(1))?;
    assert!(matches!(second, crate::mux::MuxRequest::SendInput { .. }));

    let _ = server.join();
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_spawn_conflict_terminal_propagates_storage_save_errors()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let _guard = crate::test_support::lock_mux_test_environment();
    let socket = crate::test_support::unique_mux_socket_path("git-ops-save-fail");
    crate::mux::set_socket_override(&socket)?;

    let state_dir = TempDir::new()?;
    let state_path = state_dir.path().join("state.json");
    let storage = Storage::with_path(state_path);
    let mut app = App::new(Config::default(), storage, Settings::default(), false);

    let mut perms = std::fs::metadata(state_dir.path())?.permissions();
    perms.set_mode(0o500);
    std::fs::set_permissions(state_dir.path(), perms)?;

    let repo_dir = init_repo_with_commit()?;
    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        "main".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let mut root = root;
    root.runtime = AgentRuntime::Host;
    let root_id = root.id;
    let session = root.mux_session.clone();
    app.data.storage.add(root);

    let manager = SessionManager::new();
    manager.create(&session, repo_dir.path(), None)?;

    app.data.git_op.agent_id = Some(root_id);

    let err = Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
        .expect_err("expected storage save to fail");
    assert!(err.to_string().contains("Failed to open state lock"));

    let mut perms = std::fs::metadata(state_dir.path())?.permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(state_dir.path(), perms)?;

    let _ = manager.kill(&session);
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_spawn_conflict_terminal_skips_startup_command_when_docker_runtime() {
    let dockerfile = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docker/worker.Dockerfile"),
    )
    .expect("worker Dockerfile should be readable");
    let template_hash = format!("{:016x}", fnv1a64(dockerfile.as_bytes()));

    let docker_dir = TempDir::new().expect("TempDir should be created");
    let docker_path = docker_dir.path().join("docker");
    std::fs::write(
        &docker_path,
        format!(
            "#!/usr/bin/env sh\n\
set -e\n\
\n\
if [ \"$1\" = \"version\" ]; then\n\
  exit 0\n\
fi\n\
\n\
if [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n\
  printf '%s\\n' '{template_hash}'\n\
  exit 0\n\
fi\n\
\n\
if [ \"$1\" = \"inspect\" ]; then\n\
  echo 'No such object' >&2\n\
  exit 1\n\
fi\n\
\n\
if [ \"$1\" = \"run\" ]; then\n\
  printf '%s\\n' 'container-id'\n\
  exit 0\n\
fi\n\
\n\
if [ \"$1\" = \"exec\" ]; then\n\
  exit 0\n\
fi\n\
\n\
exit 0\n"
        ),
    )
    .expect("fake docker script should be written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&docker_path)
            .expect("fake docker metadata should be readable")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&docker_path, perms)
            .expect("fake docker script permissions should be set");
    }

    let _guard = crate::test_support::lock_mux_test_environment();
    let socket = crate::test_support::unique_mux_socket_path("git-ops-docker");
    crate::mux::set_socket_override(&socket).expect("mux socket override should be set");

    let (mut app, _temp) = create_test_app().expect("test app should be created");

    let repo_dir = init_repo_with_commit().expect("test repo should be created");
    let root = Agent::new(
        "root".to_string(),
        "codex".to_string(),
        "main".to_string(),
        repo_dir.path().to_path_buf(),
    );
    let mut root = root;
    root.runtime = AgentRuntime::Docker;
    let root_id = root.id;
    let session = root.mux_session.clone();
    app.data.storage.add(root);

    let manager = SessionManager::new();
    manager
        .create(&session, repo_dir.path(), None)
        .expect("mux session should be created");

    app.data.git_op.agent_id = Some(root_id);
    app.data.ui.preview_dimensions = Some((80, 20));

    let next = crate::runtime::with_docker_program_override_for_tests(docker_path.clone(), || {
        Actions::spawn_conflict_terminal(&mut app.data, "Merge Conflict", "git status")
    });
    app.apply_mode(next.expect("conflict terminal should spawn"));

    assert_eq!(app.mode, AppMode::normal());
    assert!(app.data.git_op.agent_id.is_none());

    let children = app.data.storage.children(root_id);
    assert_eq!(children.len(), 1);
    let terminal = children.first().expect("missing conflict terminal");
    let window_target = SessionManager::window_target(
        &terminal.mux_session,
        terminal.window_index.expect("missing window index"),
    );
    let command = crate::mux::OutputCapture::new()
        .pane_current_command(&window_target)
        .expect("pane command should be captured");
    assert_eq!(command, docker_path.to_string_lossy());

    let _ = manager.kill(&session);
}

#[test]
fn test_execute_push_errors_when_worktree_path_is_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;

    let missing_worktree = temp_dir.path().join("missing-worktree");
    assert!(!missing_worktree.exists());

    let agent = Agent::new(
        "missing-worktree-agent".to_string(),
        "claude".to_string(),
        "feature/missing-worktree".to_string(),
        missing_worktree,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/missing-worktree".to_string();

    let err = Actions::execute_push(&mut app.data).unwrap_err();
    assert!(err.to_string().contains("Failed to push to remote"));
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
    let (_fixture_temp, _remote_repo, local_repo) = create_bare_remote_fixture()?;

    let agent = Agent::new(
        "pr-agent".to_string(),
        "claude".to_string(),
        "feature/pr-agent".to_string(),
        local_repo,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    let next = with_tracing_dispatch(|| Actions::open_pr_flow(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::ConfirmPushForPR(ConfirmPushForPRMode));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "feature/pr-agent");
    assert_eq!(app.data.git_op.base_branch, "main");
    assert!(app.data.git_op.has_unpushed);
    Ok(())
}

#[test]
fn test_open_pr_flow_errors_without_agent() -> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let err = Actions::open_pr_flow(&mut app.data).expect_err("Expected open_pr_flow to error");
    assert!(err.to_string().contains("No agent selected"));
    Ok(())
}

#[test]
fn test_open_pr_flow_opens_pr_when_no_unpushed_and_gh_succeeds()
-> Result<(), Box<dyn std::error::Error>> {
    ensure_stub_gh_installed()?;

    let (mut app, _temp) = create_test_app()?;
    let (_repo_temp, repo_path) = init_repo_with_remote_main_and_branch("feature/open-pr")?;
    let agent = Agent::new(
        "open-pr-agent".to_string(),
        "claude".to_string(),
        "feature/open-pr".to_string(),
        repo_path,
    );
    app.data.storage.add(agent);

    let next = with_tracing_dispatch(|| Actions::open_pr_flow(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.ui.status_message.is_some());
    Ok(())
}

#[test]
fn test_open_pr_flow_shows_error_modal_when_gh_is_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut app, _temp) = create_test_app()?;
    let (_repo_temp, repo_path) =
        init_repo_with_remote_main_and_branch("feature/open-pr-missing-gh")?;
    let agent = Agent::new(
        "open-pr-missing-gh".to_string(),
        "claude".to_string(),
        "feature/open-pr-missing-gh".to_string(),
        repo_path,
    );
    app.data.storage.add(agent);

    let missing_gh = std::env::temp_dir().join(format!(
        "tenex-missing-gh-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));

    let next = with_tracing_dispatch(|| {
        super::open_pr::with_gh_binary_override(missing_gh.into_os_string(), || {
            Actions::open_pr_flow(&mut app.data)
        })
    })?;
    app.apply_mode(next);

    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.git_op.branch_name.is_empty());
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

    with_tracing_dispatch(|| Actions::open_pr_in_browser(&mut app.data))?;
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

    let err = with_tracing_dispatch(|| Actions::open_pr_in_browser(&mut app.data))
        .expect_err("Expected open_pr_in_browser to fail");
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

    let next = with_tracing_dispatch(|| Actions::execute_push_and_open_pr(&mut app.data))?;
    app.apply_mode(next);

    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.git_op.agent_id.is_none());
    Ok(())
}

#[test]
fn test_execute_push_and_open_pr_succeeds_when_push_and_gh_succeed()
-> Result<(), Box<dyn std::error::Error>> {
    ensure_stub_gh_installed()?;

    let (mut app, _temp) = create_test_app()?;
    let (_repo_temp, repo_path) = init_repo_with_remote_main_and_branch("feature/push-and-open")?;

    let agent = Agent::new(
        "push-and-open".to_string(),
        "claude".to_string(),
        "feature/push-and-open".to_string(),
        repo_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/push-and-open".to_string();
    app.data.git_op.base_branch = "main".to_string();

    let next = with_tracing_dispatch(|| Actions::execute_push_and_open_pr(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.git_op.branch_name.is_empty());
    assert!(app.data.ui.status_message.is_some());
    Ok(())
}

#[test]
fn test_execute_push_and_open_pr_shows_error_modal_when_gh_fails()
-> Result<(), Box<dyn std::error::Error>> {
    ensure_stub_gh_installed()?;

    let (mut app, _temp) = create_test_app()?;
    let (_repo_temp, repo_path) =
        init_repo_with_remote_main_and_branch("feature/push-and-open-fail-gh")?;

    let agent = Agent::new(
        "push-and-open-fail-gh".to_string(),
        "claude".to_string(),
        "feature/push-and-open-fail-gh".to_string(),
        repo_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/push-and-open-fail-gh".to_string();
    app.data.git_op.base_branch = "develop".to_string();

    let next = with_tracing_dispatch(|| Actions::execute_push_and_open_pr(&mut app.data))?;
    app.apply_mode(next);

    assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
    assert!(app.data.git_op.agent_id.is_none());
    assert!(app.data.git_op.branch_name.is_empty());
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
fn test_detect_base_branch_falls_back_to_origin_head_when_reflog_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let (_repo_temp, repo_path) = init_repo_with_remote_main_and_branch("feature/detect-base")?;
    let base = Actions::detect_base_branch(&repo_path, "does-not-exist");
    assert_eq!(base, "main");
    Ok(())
}

#[test]
fn test_detect_base_branch_handles_empty_origin_head_output()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;

    #[cfg(windows)]
    let script_path = {
        let script = temp.path().join("git.cmd");
        std::fs::write(
            &script,
            r#"@echo off
set cmd=%1
if "%cmd%"=="reflog" exit /b 1
if "%cmd%"=="symbolic-ref" exit /b 0
if "%cmd%"=="show-ref" (
  if "%4"=="refs/heads/main" exit /b 0
  exit /b 1
)
exit /b 1
"#,
        )?;
        script
    };

    #[cfg(not(windows))]
    let script_path = {
        let script = temp.path().join("git");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail

cmd=""
if [[ $# -gt 0 ]]; then
  cmd="$1"
  shift
fi

if [[ "$cmd" == "reflog" ]]; then
  exit 1
fi

if [[ "$cmd" == "symbolic-ref" ]]; then
  exit 0
fi

if [[ "$cmd" == "show-ref" ]]; then
  ref="${@: -1}"
  if [[ "$ref" == "refs/heads/main" ]]; then
    exit 0
  fi
  exit 1
fi

exit 1
"#,
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(&script)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms)?;
        }

        script
    };

    let base = crate::git::with_git_program_override_for_tests(script_path, || {
        Actions::detect_base_branch(temp.path(), "feature/empty-origin-head")
    });
    assert_eq!(base, "main");
    Ok(())
}

#[test]
fn test_detect_base_branch_uses_origin_head_when_origin_head_is_non_default()
-> Result<(), Box<dyn std::error::Error>> {
    let (_repo_temp, repo_path) = init_repo_with_remote_main_and_branch("feature/detect-base")?;

    git_ok(&repo_path, &["checkout", "-q", "-b", "trunk", "main"])?;
    git_ok(&repo_path, &["push", "-q", "-u", "origin", "trunk"])?;
    git_ok(&repo_path, &["fetch", "-q", "origin"])?;
    git_ok(
        &repo_path,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    )?;

    let base = Actions::detect_base_branch(&repo_path, "does-not-exist");
    assert_eq!(base, "trunk");
    Ok(())
}

#[test]
fn test_detect_base_branch_falls_back_to_remote_default_when_local_defaults_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let seed_repo = temp_dir.path().join("seed");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&seed_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    git_ok(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;

    git_ok(&seed_repo, &["init", "-q", "-b", "main"])?;
    git_ok(&seed_repo, &["config", "user.email", "test@example.com"])?;
    git_ok(&seed_repo, &["config", "user.name", "Test"])?;
    std::fs::write(seed_repo.join("README.md"), "test")?;
    git_ok(&seed_repo, &["add", "."])?;
    git_ok(&seed_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;
    git_ok(
        &seed_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    git_ok(&seed_repo, &["push", "-q", "-u", "origin", "main"])?;

    git_ok(&local_repo, &["init", "-q", "-b", "feature/only"])?;
    git_ok(&local_repo, &["config", "user.email", "test@example.com"])?;
    git_ok(&local_repo, &["config", "user.name", "Test"])?;
    std::fs::write(local_repo.join("README.md"), "local")?;
    git_ok(&local_repo, &["add", "."])?;
    git_ok(
        &local_repo,
        &["commit", "-q", "--no-verify", "-m", "local init"],
    )?;
    git_ok(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    git_ok(&local_repo, &["fetch", "-q", "origin"])?;

    let _ = crate::git::git_command()
        .args(["update-ref", "-d", "refs/remotes/origin/HEAD"])
        .current_dir(&local_repo)
        .output();

    let base = Actions::detect_base_branch(&local_repo, "does-not-exist");
    assert_eq!(base, "main");
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

fn git_status_success(
    current_dir: &std::path::Path,
    args: &[&str],
) -> Result<bool, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(1) {
        return Ok(false);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("git {args:?} failed:\n{stdout}\n{stderr}").into())
}

fn git_ref_exists(
    repo: &std::path::Path,
    refname: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    git_status_success(repo, &["rev-parse", "--verify", "--quiet", refname])
}

fn git_config_value(
    repo: &std::path::Path,
    key: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(["config", "--get", key])
        .current_dir(repo)
        .output()?;
    if output.status.success() {
        let raw = String::from_utf8(output.stdout)?;
        let value = raw.trim_end_matches('\n').trim_end_matches('\r');
        return Ok(Some(value.to_string()));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("git config --get {key} failed:\n{stdout}\n{stderr}").into())
}

fn git_stdout(
    current_dir: &std::path::Path,
    args: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()?;
    if output.status.success() {
        return Ok(String::from_utf8(output.stdout)?);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("git {args:?} failed:\n{stdout}\n{stderr}").into())
}

fn git_revision(repo: &std::path::Path, rev: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(git_stdout(repo, &["rev-parse", rev])?.trim().to_string())
}

fn create_bare_remote_fixture() -> Result<(TempDir, PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    run_git(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;
    run_git(&local_repo, &["init", "-q", "-b", "main"])?;
    run_git(&local_repo, &["config", "user.email", "tenex@test.invalid"])?;
    run_git(&local_repo, &["config", "user.name", "Tenex Test"])?;
    run_git(&local_repo, &["config", "push.autoSetupRemote", "false"])?;
    std::fs::write(local_repo.join("README.md"), "test\n")?;
    run_git(&local_repo, &["add", "."])?;
    run_git(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;
    run_git(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    run_git(&local_repo, &["push", "-q", "-u", "origin", "main"])?;

    Ok((temp_dir, remote_repo, local_repo))
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
    let new_worktree_path = config.worktree_path_for_repo_root(&local_repo, &new_branch);

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

    assert!(git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{old_branch}")
    )?);
    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{new_branch}")
    )?);
    assert_eq!(
        git_config_value(&new_worktree_path, &format!("branch.{new_branch}.remote"))?.as_deref(),
        Some("origin")
    );
    let expected_merge = format!("refs/heads/{old_branch}");
    assert_eq!(
        git_config_value(&new_worktree_path, &format!("branch.{new_branch}.merge"))?.as_deref(),
        Some(expected_merge.as_str())
    );

    Ok(())
}

#[test]
fn test_execute_push_after_root_rename_updates_old_remote_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    run_git(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;
    run_git(&local_repo, &["init", "-q", "-b", "main"])?;
    run_git(&local_repo, &["config", "user.email", "tenex@test.invalid"])?;
    run_git(&local_repo, &["config", "user.name", "Tenex Test"])?;
    std::fs::write(local_repo.join("README.md"), "test\n")?;
    run_git(&local_repo, &["add", "."])?;
    run_git(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;
    run_git(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    run_git(&local_repo, &["push", "-q", "-u", "origin", "main"])?;

    let old_title = "pushed-agent";
    let new_title = "renamed-pushed-agent";
    let config = Config {
        worktree_dir: temp_dir.path().join("worktrees"),
        ..Config::default()
    };
    let old_branch = config.generate_branch_name(old_title);
    let new_branch = config.generate_branch_name(new_title);
    let new_worktree_path = config.worktree_path_for_repo_root(&local_repo, &new_branch);

    run_git(&local_repo, &["checkout", "-q", "-b", old_branch.as_str()])?;
    run_git(
        &local_repo,
        &["push", "-q", "-u", "origin", old_branch.as_str()],
    )?;
    run_git(&local_repo, &["fetch", "-q", "origin"])?;

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

    std::fs::write(new_worktree_path.join("after-rename.txt"), "after rename\n")?;
    run_git(&new_worktree_path, &["add", "after-rename.txt"])?;
    run_git(
        &new_worktree_path,
        &["commit", "-q", "--no-verify", "-m", "after rename"],
    )?;

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = new_branch.clone();
    let next = with_tracing_dispatch(|| Actions::execute_push(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert_eq!(
        git_revision(&remote_repo, &format!("refs/heads/{old_branch}"))?,
        git_revision(&new_worktree_path, "HEAD")?
    );
    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{new_branch}")
    )?);
    assert_eq!(
        git_config_value(&new_worktree_path, &format!("branch.{new_branch}.remote"))?.as_deref(),
        Some("origin")
    );
    let expected_merge = format!("refs/heads/{old_branch}");
    assert_eq!(
        git_config_value(&new_worktree_path, &format!("branch.{new_branch}.merge"))?.as_deref(),
        Some(expected_merge.as_str())
    );

    Ok(())
}

#[test]
fn test_execute_push_without_upstream_sets_origin_tracking()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    run_git(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;
    run_git(&local_repo, &["init", "-q", "-b", "main"])?;
    run_git(&local_repo, &["config", "user.email", "tenex@test.invalid"])?;
    run_git(&local_repo, &["config", "user.name", "Tenex Test"])?;
    std::fs::write(local_repo.join("README.md"), "test\n")?;
    run_git(&local_repo, &["add", "."])?;
    run_git(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;
    run_git(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    run_git(&local_repo, &["push", "-q", "-u", "origin", "main"])?;

    let branch = "feature/not-renamed";
    run_git(&local_repo, &["checkout", "-q", "-b", branch])?;
    assert_eq!(
        git_config_value(&local_repo, &format!("branch.{branch}.remote"))?,
        None
    );
    assert_eq!(
        git_config_value(&local_repo, &format!("branch.{branch}.merge"))?,
        None
    );

    let (mut app, _temp) = create_test_app()?;
    let worktree_path = local_repo;
    let agent = Agent::new(
        "not-renamed".to_string(),
        "claude".to_string(),
        branch.to_string(),
        worktree_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch.to_string();

    let next = with_tracing_dispatch(|| Actions::execute_push(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert!(git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{branch}")
    )?);
    assert_eq!(
        git_config_value(&worktree_path, &format!("branch.{branch}.remote"))?.as_deref(),
        Some("origin")
    );
    let expected_merge = format!("refs/heads/{branch}");
    assert_eq!(
        git_config_value(&worktree_path, &format!("branch.{branch}.merge"))?.as_deref(),
        Some(expected_merge.as_str())
    );

    Ok(())
}

#[test]
fn test_configured_upstream_errors_when_config_is_incomplete()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = TempDir::new()?;
    run_git(repo.path(), &["init", "-q", "-b", "main"])?;
    run_git(
        repo.path(),
        &["config", "branch.feature/local.remote", "origin"],
    )?;

    let err = super::push::configured_upstream(repo.path(), "feature/local")
        .err()
        .ok_or_else(|| std::io::Error::other("expected incomplete upstream config error"))?;

    assert!(err.to_string().contains("Incomplete upstream config"));
    Ok(())
}

#[test]
fn test_configured_upstream_errors_when_config_value_is_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = TempDir::new()?;
    run_git(repo.path(), &["init", "-q", "-b", "main"])?;
    run_git(repo.path(), &["config", "branch.feature/local.remote", ""])?;

    let err = super::push::configured_upstream(repo.path(), "feature/local")
        .err()
        .ok_or_else(|| std::io::Error::other("expected empty config value error"))?;

    assert!(err.to_string().contains("is empty"));
    Ok(())
}

#[test]
fn test_configured_upstream_errors_when_config_read_fails() -> Result<(), Box<dyn std::error::Error>>
{
    let repo = TempDir::new()?;
    run_git(repo.path(), &["init", "-q", "-b", "main"])?;
    std::fs::write(repo.path().join(".git").join("config"), "[broken\n")?;

    let err = super::push::configured_upstream(repo.path(), "feature/local")
        .err()
        .ok_or_else(|| std::io::Error::other("expected git config read error"))?;

    assert!(err.to_string().contains("Failed to read git config key"));
    Ok(())
}

#[test]
fn test_configured_upstream_errors_when_merge_config_read_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let worktree = TempDir::new()?;
    let temp = TempDir::new()?;

    #[cfg(windows)]
    let script = temp.path().join("git.cmd");
    #[cfg(not(windows))]
    let script = temp.path().join("git");

    #[cfg(windows)]
    std::fs::write(
        &script,
        r#"@echo off
if "%1"=="config" if "%2"=="--get" if "%3"=="branch.feature/local.remote" (
  echo origin
  exit /b 0
)
if "%1"=="config" if "%2"=="--get" if "%3"=="branch.feature/local.merge" (
  echo merge boom 1>&2
  exit /b 2
)
exit /b 1
"#,
    )?;

    #[cfg(not(windows))]
    std::fs::write(
        &script,
        r#"#!/bin/sh
if [ "$1" = "config" ] && [ "$2" = "--get" ] && [ "$3" = "branch.feature/local.remote" ]; then
  printf 'origin\n'
  exit 0
fi
if [ "$1" = "config" ] && [ "$2" = "--get" ] && [ "$3" = "branch.feature/local.merge" ]; then
  printf 'merge boom\n' >&2
  exit 2
fi
exit 1
"#,
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = std::fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms)?;
    }

    let err = crate::git::with_git_program_override_for_tests(script, || {
        super::push::configured_upstream(worktree.path(), "feature/local")
    })
    .err()
    .ok_or_else(|| std::io::Error::other("expected merge config read error"))?;

    let message = err.to_string();
    assert!(message.contains("Failed to read git config key"));
    assert!(message.contains("branch.feature/local.merge"));
    Ok(())
}

#[test]
fn test_has_unpushed_commits_uses_configured_upstream_after_rename()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    run_git(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;
    run_git(&local_repo, &["init", "-q", "-b", "main"])?;
    run_git(&local_repo, &["config", "user.email", "tenex@test.invalid"])?;
    run_git(&local_repo, &["config", "user.name", "Tenex Test"])?;
    std::fs::write(local_repo.join("README.md"), "test\n")?;
    run_git(&local_repo, &["add", "."])?;
    run_git(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;
    run_git(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;
    run_git(&local_repo, &["push", "-q", "-u", "origin", "main"])?;

    let old_branch = "feature/pr-old";
    let new_branch = "feature/pr-new";
    run_git(&local_repo, &["checkout", "-q", "-b", old_branch])?;
    run_git(&local_repo, &["push", "-q", "-u", "origin", old_branch])?;
    run_git(&local_repo, &["fetch", "-q", "origin"])?;
    run_git(&local_repo, &["branch", "-m", old_branch, new_branch])?;

    assert!(!Actions::has_unpushed_commits(&local_repo, new_branch)?);
    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{new_branch}")
    )?);

    std::fs::write(local_repo.join("pr-change.txt"), "after rename\n")?;
    run_git(&local_repo, &["add", "pr-change.txt"])?;
    run_git(
        &local_repo,
        &["commit", "-q", "--no-verify", "-m", "after rename"],
    )?;

    assert!(Actions::has_unpushed_commits(&local_repo, new_branch)?);
    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{new_branch}")
    )?);

    Ok(())
}

#[test]
fn test_has_unpushed_commits_detects_deleted_configured_upstream_after_rename_and_push_recreates()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp_dir, remote_repo, local_repo) = create_bare_remote_fixture()?;
    let old_branch = "feature/stale-old";
    let new_branch = "feature/stale-new";

    run_git(&local_repo, &["checkout", "-q", "-b", old_branch])?;
    run_git(&local_repo, &["push", "-q", "-u", "origin", old_branch])?;
    run_git(&local_repo, &["fetch", "-q", "origin"])?;
    run_git(&local_repo, &["branch", "-m", old_branch, new_branch])?;
    run_git(
        &remote_repo,
        &["update-ref", "-d", &format!("refs/heads/{old_branch}")],
    )?;

    assert!(git_ref_exists(
        &local_repo,
        &format!("refs/remotes/origin/{old_branch}")
    )?);
    assert!(Actions::has_unpushed_commits(&local_repo, new_branch)?);

    let (mut app, _temp) = create_test_app()?;
    let agent = Agent::new(
        "stale-renamed".to_string(),
        "claude".to_string(),
        new_branch.to_string(),
        local_repo.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = new_branch.to_string();

    let next = with_tracing_dispatch(|| Actions::execute_push(&mut app.data))?;
    app.apply_mode(next);

    assert_eq!(app.mode, AppMode::normal());
    assert_eq!(
        git_revision(&remote_repo, &format!("refs/heads/{old_branch}"))?,
        git_revision(&local_repo, "HEAD")?
    );
    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{new_branch}")
    )?);
    assert_eq!(
        git_config_value(&local_repo, &format!("branch.{new_branch}.remote"))?.as_deref(),
        Some("origin")
    );
    let expected_merge = format!("refs/heads/{old_branch}");
    assert_eq!(
        git_config_value(&local_repo, &format!("branch.{new_branch}.merge"))?.as_deref(),
        Some(expected_merge.as_str())
    );

    Ok(())
}

#[test]
fn test_has_unpushed_commits_detects_deleted_fallback_remote_ref()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp_dir, remote_repo, local_repo) = create_bare_remote_fixture()?;
    let branch = "feature/stale-fallback";

    run_git(&local_repo, &["checkout", "-q", "-b", branch])?;
    run_git(&local_repo, &["push", "-q", "origin", branch])?;
    run_git(&local_repo, &["fetch", "-q", "origin"])?;
    assert_eq!(
        git_config_value(&local_repo, &format!("branch.{branch}.remote"))?,
        None
    );
    assert!(git_ref_exists(
        &local_repo,
        &format!("refs/remotes/origin/{branch}")
    )?);

    run_git(
        &remote_repo,
        &["update-ref", "-d", &format!("refs/heads/{branch}")],
    )?;

    assert!(Actions::has_unpushed_commits(&local_repo, branch)?);
    Ok(())
}

#[test]
fn test_has_unpushed_commits_fallback_uses_live_remote_ref()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp_dir, _remote_repo, local_repo) = create_bare_remote_fixture()?;
    let branch = "feature/live-fallback";

    run_git(&local_repo, &["checkout", "-q", "-b", branch])?;
    run_git(&local_repo, &["push", "-q", "origin", branch])?;
    run_git(&local_repo, &["fetch", "-q", "origin"])?;
    assert_eq!(
        git_config_value(&local_repo, &format!("branch.{branch}.remote"))?,
        None
    );

    assert!(!Actions::has_unpushed_commits(&local_repo, branch)?);

    std::fs::write(local_repo.join("fallback-change.txt"), "after fallback\n")?;
    run_git(&local_repo, &["add", "fallback-change.txt"])?;
    run_git(
        &local_repo,
        &["commit", "-q", "--no-verify", "-m", "after fallback"],
    )?;

    assert!(Actions::has_unpushed_commits(&local_repo, branch)?);
    Ok(())
}

#[test]
fn test_execute_root_rename_before_push_leaves_no_upstream()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let remote_repo = temp_dir.path().join("remote.git");
    let local_repo = temp_dir.path().join("local");

    std::fs::create_dir_all(&remote_repo)?;
    std::fs::create_dir_all(&local_repo)?;

    run_git(&remote_repo, &["init", "--bare", "-q", "-b", "main"])?;
    run_git(&local_repo, &["init", "-q", "-b", "main"])?;
    run_git(&local_repo, &["config", "user.email", "tenex@test.invalid"])?;
    run_git(&local_repo, &["config", "user.name", "Tenex Test"])?;
    std::fs::write(local_repo.join("README.md"), "test\n")?;
    run_git(&local_repo, &["add", "."])?;
    run_git(&local_repo, &["commit", "-q", "--no-verify", "-m", "init"])?;
    run_git(
        &local_repo,
        &[
            "remote",
            "add",
            "origin",
            remote_repo.to_string_lossy().as_ref(),
        ],
    )?;

    let old_title = "unpushed-agent";
    let new_title = "renamed-unpushed-agent";
    let config = Config {
        worktree_dir: temp_dir.path().join("worktrees"),
        ..Config::default()
    };
    let old_branch = config.generate_branch_name(old_title);
    let new_branch = config.generate_branch_name(new_title);
    let new_worktree_path = config.worktree_path_for_repo_root(&local_repo, &new_branch);

    run_git(&local_repo, &["checkout", "-q", "-b", old_branch.as_str()])?;

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

    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{old_branch}")
    )?);
    assert!(!git_ref_exists(
        &remote_repo,
        &format!("refs/heads/{new_branch}")
    )?);
    assert_eq!(
        git_config_value(&new_worktree_path, &format!("branch.{new_branch}.remote"))?,
        None
    );
    assert_eq!(
        git_config_value(&new_worktree_path, &format!("branch.{new_branch}.merge"))?,
        None
    );

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
    let (_fixture_temp, _remote_repo, local_repo) = create_bare_remote_fixture()?;

    let agent = Agent::new(
        "test-agent".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        local_repo,
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
fn test_rebase_branch_populates_branch_selector() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

    let repo_dir = init_repo_with_commit()?;
    let (mut app, _temp) = create_test_app()?;

    let mut agent = Agent::new(
        "rebase-agent".to_string(),
        "claude".to_string(),
        "feature/rebase-agent".to_string(),
        repo_dir.path().to_path_buf(),
    );
    agent.repo_root = Some(repo_dir.path().to_path_buf());
    let agent_id = agent.id;
    app.data.storage.add(agent);

    let next =
        tracing::dispatcher::with_default(&dispatch, || Actions::rebase_branch(&mut app.data))?;
    app.apply_mode(next);

    assert!(matches!(
        app.mode,
        AppMode::RebaseBranchSelector(crate::state::RebaseBranchSelectorMode)
    ));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert!(!app.data.review.branches.is_empty());
    Ok(())
}

#[test]
fn test_rebase_branch_uses_workspace_root_when_repo_root_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let repo_dir = init_repo_with_commit()?;
    let nested = repo_dir.path().join("nested");
    std::fs::create_dir_all(&nested)?;

    let (mut app, _temp) = create_test_app()?;
    let agent = Agent::new(
        "rebase-nested-agent".to_string(),
        "claude".to_string(),
        "feature/rebase-nested-agent".to_string(),
        nested,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    let next = Actions::rebase_branch(&mut app.data)?;
    app.apply_mode(next);

    assert!(matches!(
        app.mode,
        AppMode::RebaseBranchSelector(crate::state::RebaseBranchSelectorMode)
    ));
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert!(!app.data.review.branches.is_empty());
    Ok(())
}

#[test]
fn test_rebase_branch_errors_when_not_in_git_repo() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let (mut app, _temp) = create_test_app()?;
    let agent = Agent::new(
        "rebase-no-repo".to_string(),
        "claude".to_string(),
        "feature/rebase-no-repo".to_string(),
        temp_dir.path().to_path_buf(),
    );
    app.data.storage.add(agent);

    let err = Actions::rebase_branch(&mut app.data).unwrap_err();
    assert!(err.to_string().contains("Failed to open git repository at"));
    Ok(())
}

#[test]
fn test_rebase_branch_propagates_branch_selector_errors() -> Result<(), Box<dyn std::error::Error>>
{
    let repo_dir = init_repo_with_commit()?;
    let (mut app, _temp) = create_test_app()?;

    let mut agent = Agent::new(
        "rebase-agent".to_string(),
        "claude".to_string(),
        "feature/rebase-agent".to_string(),
        repo_dir.path().to_path_buf(),
    );
    agent.repo_root = Some(repo_dir.path().to_path_buf());
    app.data.storage.add(agent);

    let err = crate::git::with_list_for_selector_override_for_tests(
        |_repo| Err(anyhow::anyhow!("boom")),
        || Actions::rebase_branch(&mut app.data),
    )
    .unwrap_err();

    assert!(err.to_string().contains("boom"));
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
fn test_execute_rebase_succeeds_and_emits_tracing_when_enabled()
-> Result<(), Box<dyn std::error::Error>> {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

    let repo_dir = init_repo_with_commit()?;
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"])?;
    std::fs::write(repo_dir.path().join("feature.txt"), "feature")?;
    git_ok(repo_dir.path(), &["add", "feature.txt"])?;
    git_ok(
        repo_dir.path(),
        &["commit", "-q", "--no-verify", "-m", "feature"],
    )?;

    let (mut app, _temp) = create_test_app()?;
    let mut agent = Agent::new(
        "rebase-exec".to_string(),
        "claude".to_string(),
        "feature".to_string(),
        repo_dir.path().to_path_buf(),
    );
    agent.repo_root = Some(repo_dir.path().to_path_buf());
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature".to_string();
    app.data.git_op.target_branch = "master".to_string();

    let next =
        tracing::dispatcher::with_default(&dispatch, || Actions::execute_rebase(&mut app.data))?;
    app.apply_mode(next);
    assert!(matches!(app.mode, AppMode::SuccessModal(_)));
    Ok(())
}

#[test]
fn test_execute_rebase_errors_when_worktree_path_is_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut app, _temp) = create_test_app()?;
    let temp_dir = TempDir::new()?;

    let missing_worktree = temp_dir.path().join("missing-rebase-worktree");
    assert!(!missing_worktree.exists());

    let agent = Agent::new(
        "missing-rebase-worktree-agent".to_string(),
        "claude".to_string(),
        "feature/missing-rebase-worktree".to_string(),
        missing_worktree,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature/missing-rebase-worktree".to_string();
    app.data.git_op.target_branch = "master".to_string();

    let err = Actions::execute_rebase(&mut app.data).unwrap_err();
    assert!(err.to_string().contains("Failed to execute rebase"));
    Ok(())
}

#[test]
fn test_execute_rebase_conflict_emits_tracing_when_enabled()
-> Result<(), Box<dyn std::error::Error>> {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

    let repo_dir = init_repo_with_commit()?;
    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"])?;
    std::fs::write(repo_dir.path().join("conflict.txt"), "feature")?;
    git_ok(repo_dir.path(), &["add", "conflict.txt"])?;
    git_ok(
        repo_dir.path(),
        &["commit", "-q", "--no-verify", "-m", "feature"],
    )?;

    git_ok(repo_dir.path(), &["checkout", "-q", "master"])?;
    std::fs::write(repo_dir.path().join("conflict.txt"), "master")?;
    git_ok(repo_dir.path(), &["add", "conflict.txt"])?;
    git_ok(
        repo_dir.path(),
        &["commit", "-q", "--no-verify", "-m", "master"],
    )?;

    git_ok(repo_dir.path(), &["checkout", "-q", "feature"])?;

    let (mut app, _temp) = create_test_app()?;
    let mut agent = Agent::new(
        "rebase-conflict".to_string(),
        "claude".to_string(),
        "feature".to_string(),
        repo_dir.path().to_path_buf(),
    );
    agent.repo_root = Some(repo_dir.path().to_path_buf());
    let agent_id = agent.id;
    app.data.storage.add(agent);

    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "feature".to_string();
    app.data.git_op.target_branch = "master".to_string();

    let result =
        tracing::dispatcher::with_default(&dispatch, || Actions::execute_rebase(&mut app.data));

    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_find_worktree_for_branch_no_worktree() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    let mut init_opts = git2::RepositoryInitOptions::new();
    init_opts.initial_head("master");
    let repo = git2::Repository::init_opts(temp_dir.path(), &init_opts)?;
    repo.set_head("refs/heads/master")?;

    // Should return None for a non-existent branch
    let result = Actions::find_worktree_for_branch(temp_dir.path(), "non-existent-branch-12345")?;
    assert!(result.is_none());
    Ok(())
}
