use anyhow::Result;
use clap::Parser;
use semver::Version;
use std::fmt::Display;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tenex::agent::Storage;
use tenex::app::Settings;
use tenex::cli::{
    Cli, CliDeps, Commands, InteractiveDeps, ResetDeps, ResetScope, cmd_default,
    cmd_reset_with_storage, cmd_reset_with_storage_with_prompts, confirm_reset,
    confirm_reset_with_io, current_dir_for_reset_deps_with, default_auto_connect_worktrees,
    default_respawn_missing_agents, delete_branch_for_reset_deps, ensure_instance_initialized,
    ensure_instance_initialized_with, env_mux_socket, env_mux_socket_from, find_installed_binary,
    find_installed_binary_with_cargo_home, init_logging_with, list_orphaned_sessions_with,
    load_storage_result_with_error, maybe_prompt_restart_mux_daemon_for_versions_with,
    maybe_prompt_restart_mux_daemon_with, maybe_queue_whats_new, maybe_queue_whats_new_with,
    mux_kill_session_for_reset_deps, mux_list_sessions_for_reset_deps, preserve_corrupt_state_file,
    preserve_corrupt_state_file_with, print_reset_plan, prompt_reset_scope_with_io,
    remove_worktree_for_reset_deps, restart_current_process, run_cli, run_interactive,
};
#[cfg(unix)]
use tenex::cli::{
    INSTALLED_BINARY_OVERRIDE, RESTART_EXEC_OVERRIDE, exec_restart, installed_binary_for_restart,
    restart_exec_override,
};
use tenex::config::Config;
use tenex::mux::SessionManager;
use tenex::state::{ConfirmAction, ConfirmingMode};
use tenex::{App, AppMode};

static RUN_INTERACTIVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static RESET_TEST_CURRENT_DIR: OnceLock<Mutex<Option<std::path::PathBuf>>> = OnceLock::new();
static INSTALL_LATEST_CALLS: AtomicUsize = AtomicUsize::new(0);
static RESTART_PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
static RUN_MUXD_CALLS: AtomicUsize = AtomicUsize::new(0);
static CMD_RESET_CALLS: AtomicUsize = AtomicUsize::new(0);
static CMD_RESET_FORCE: AtomicUsize = AtomicUsize::new(0);
static MIGRATE_CALLS: AtomicUsize = AtomicUsize::new(0);

fn ok<T>(value: T) -> Result<T> {
    std::io::sink().write_all(&[])?;
    Ok(value)
}

fn io_ok<T>(value: T) -> std::io::Result<T> {
    std::io::sink().write_all(&[])?;
    Ok(value)
}

fn err<T>(message: &str) -> Result<T> {
    std::io::sink().write_all(&[])?;
    Err(anyhow::anyhow!("{message}"))
}

fn missing<T>(value: Option<T>, message: &str) -> Result<T> {
    value.ok_or_else(|| anyhow::anyhow!("{message}"))
}

fn error_string<T, E: Display>(result: std::result::Result<T, E>) -> String {
    match result {
        Ok(_) => {
            let failed = std::hint::black_box(false);
            assert!(failed, "expected error");
            String::new()
        }
        Err(err) => err.to_string(),
    }
}

fn run_interactive_lock() -> &'static Mutex<()> {
    RUN_INTERACTIVE_LOCK.get_or_init(|| Mutex::new(()))
}

fn reset_test_current_dir_lock() -> &'static Mutex<Option<PathBuf>> {
    RESET_TEST_CURRENT_DIR.get_or_init(|| Mutex::new(None))
}

fn set_reset_test_current_dir(path: &Path) {
    *reset_test_current_dir_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path.to_path_buf());
}

fn clear_reset_test_current_dir() {
    *reset_test_current_dir_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

fn mux_version_ok() -> String {
    "expected".to_string()
}

fn socket_display_err() -> Result<String> {
    err("boom")
}

fn auto_connect_err(_app: &mut App) -> Result<()> {
    err("boom")
}

fn respawn_err(_app: &mut App) -> Result<()> {
    err("boom")
}

fn run_tui_err(app: App) -> Result<Option<tenex::update::UpdateInfo>> {
    let App { mode, .. } = app;
    let _ = mode;
    err("boom")
}

fn flush_stdout_ok() -> std::io::Result<()> {
    io_ok(())
}

fn flush_stdout_err() -> std::io::Result<()> {
    Err(std::io::Error::other("boom"))
}

fn read_line_err(_input: &mut String) -> std::io::Result<usize> {
    Err(std::io::Error::other("boom"))
}

fn running_daemon_none() -> Result<Option<String>> {
    ok(None)
}

fn running_daemon_err() -> Result<Option<String>> {
    err("boom")
}

fn socket_display_ok() -> Result<String> {
    ok("socket".to_string())
}

const fn env_mux_socket_none() -> Option<String> {
    None
}

fn fake_check_for_update_none() -> Result<Option<tenex::update::UpdateInfo>> {
    ok(None)
}

fn fake_check_for_update_available() -> Result<Option<tenex::update::UpdateInfo>> {
    ok(Some(tenex::update::UpdateInfo {
        current_version: Version::parse("1.0.0")?,
        latest_version: Version::parse("2.0.0")?,
    }))
}

fn fake_check_for_update_error() -> Result<Option<tenex::update::UpdateInfo>> {
    err("boom")
}

fn fake_run_tui_returns_update(app: App) -> Result<Option<tenex::update::UpdateInfo>> {
    let App { mode, .. } = app;
    let _ = mode;
    ok(Some(tenex::update::UpdateInfo {
        current_version: Version::parse("1.0.0")?,
        latest_version: Version::parse("2.0.0")?,
    }))
}

fn fake_run_tui_none(app: App) -> Result<Option<tenex::update::UpdateInfo>> {
    let App { mode, .. } = app;
    let _ = mode;
    ok(None)
}

fn fake_install_latest() -> Result<()> {
    INSTALL_LATEST_CALLS.fetch_add(1, Ordering::SeqCst);
    ok(())
}

fn fake_install_latest_err() -> Result<()> {
    err("boom")
}

fn fake_restart_process() -> Result<()> {
    RESTART_PROCESS_CALLS.fetch_add(1, Ordering::SeqCst);
    ok(())
}

fn fake_restart_process_err() -> Result<()> {
    err("boom")
}

fn fake_run_mux_daemon() -> Result<()> {
    RUN_MUXD_CALLS.fetch_add(1, Ordering::SeqCst);
    ok(())
}

fn fake_migrate_ok_counted() -> Result<()> {
    MIGRATE_CALLS.fetch_add(1, Ordering::SeqCst);
    ok(())
}

fn fake_migrate_err() -> Result<()> {
    err("boom")
}

fn fake_cmd_reset(force: bool) -> Result<()> {
    CMD_RESET_CALLS.fetch_add(1, Ordering::SeqCst);
    CMD_RESET_FORCE.store(usize::from(force), Ordering::SeqCst);
    ok(())
}

fn fake_load_settings() -> Settings {
    Settings::default()
}

fn fake_auto_connect_worktrees(app: &mut App) -> Result<()> {
    let _ = app;
    ok(())
}

fn fake_respawn_missing_agents(app: &mut App) -> Result<()> {
    let _ = app;
    ok(())
}

fn reset_test_current_dir() -> Result<std::path::PathBuf> {
    reset_test_current_dir_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Reset test current dir not set"))
}

#[test]
fn test_reset_test_current_dir_errors_when_missing() {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    clear_reset_test_current_dir();

    let err = reset_test_current_dir()
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(err.contains("Reset test current dir not set"));
}

#[test]
fn test_current_dir_for_reset_deps_with_propagates_get_current_dir_errors() {
    fn get_current_dir_err() -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("boom"))
    }

    let err = current_dir_for_reset_deps_with(get_current_dir_err)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(err.contains("boom"));
}

fn set_last_seen_ok(settings: &mut Settings, version: &Version) -> std::io::Result<()> {
    settings.last_seen_version = Some(version.to_string());
    io_ok(())
}

fn set_last_seen_err(settings: &mut Settings, version: &Version) -> std::io::Result<()> {
    settings.last_seen_version = Some(version.to_string());
    Err(std::io::Error::other("boom"))
}

fn changelog_lines_empty(_version: &Version) -> Result<Vec<String>> {
    ok(Vec::new())
}

fn whats_new_lines_empty(_from: Option<&Version>, _to: &Version) -> Result<Vec<String>> {
    ok(Vec::new())
}

#[test]
fn test_cli_parsing() {
    let cli = Cli::parse_from(["tenex"]);
    assert!(cli.command.is_none());
}

#[test]
fn test_cli_reset_command() {
    let cli = Cli::parse_from(["tenex", "reset", "--force"]);
    assert!(matches!(cli.command, Some(Commands::Reset { force: true })));
}

#[test]
fn test_cli_muxd_command() {
    let cli = Cli::parse_from(["tenex", "muxd"]);
    assert!(matches!(cli.command, Some(Commands::Muxd)));
}

#[test]
fn test_run_cli_muxd_calls_run_mux_daemon() -> Result<()> {
    RUN_MUXD_CALLS.store(0, Ordering::SeqCst);

    let cli = Cli::parse_from(["tenex", "muxd"]);
    let dir = TempDir::new()?;
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let state_path = dir.path().join("state.json");

    let deps = CliDeps {
        migrate_default_state_dir: fake_migrate_ok_counted,
        run_mux_daemon: fake_run_mux_daemon,
        cmd_reset: fake_cmd_reset,
        load_settings: fake_load_settings,
        interactive_deps: InteractiveDeps {
            check_for_update: fake_check_for_update_none,
            run_tui: fake_run_tui_none,
            install_latest: fake_install_latest,
            restart_current_process: fake_restart_process,
            auto_connect_worktrees: fake_auto_connect_worktrees,
            respawn_missing_agents: fake_respawn_missing_agents,
        },
    };

    run_cli(&cli, config, &state_path, None, &deps)?;
    assert_eq!(RUN_MUXD_CALLS.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn test_run_cli_reset_warns_when_migration_fails() -> Result<()> {
    CMD_RESET_CALLS.store(0, Ordering::SeqCst);
    CMD_RESET_FORCE.store(0, Ordering::SeqCst);

    let cli = Cli::parse_from(["tenex", "reset", "--force"]);
    let dir = TempDir::new()?;
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let state_path = dir.path().join("state.json");

    let deps = CliDeps {
        migrate_default_state_dir: fake_migrate_err,
        run_mux_daemon: fake_run_mux_daemon,
        cmd_reset: fake_cmd_reset,
        load_settings: fake_load_settings,
        interactive_deps: InteractiveDeps {
            check_for_update: fake_check_for_update_none,
            run_tui: fake_run_tui_none,
            install_latest: fake_install_latest,
            restart_current_process: fake_restart_process,
            auto_connect_worktrees: fake_auto_connect_worktrees,
            respawn_missing_agents: fake_respawn_missing_agents,
        },
    };

    run_cli(&cli, config, &state_path, None, &deps)?;
    assert_eq!(CMD_RESET_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(CMD_RESET_FORCE.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn test_run_cli_reset_calls_migration_when_ok() -> Result<()> {
    MIGRATE_CALLS.store(0, Ordering::SeqCst);
    CMD_RESET_CALLS.store(0, Ordering::SeqCst);

    let cli = Cli::parse_from(["tenex", "reset", "--force"]);
    let dir = TempDir::new()?;
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let state_path = dir.path().join("state.json");

    let deps = CliDeps {
        migrate_default_state_dir: fake_migrate_ok_counted,
        run_mux_daemon: fake_run_mux_daemon,
        cmd_reset: fake_cmd_reset,
        load_settings: fake_load_settings,
        interactive_deps: InteractiveDeps {
            check_for_update: fake_check_for_update_none,
            run_tui: fake_run_tui_none,
            install_latest: fake_install_latest,
            restart_current_process: fake_restart_process,
            auto_connect_worktrees: fake_auto_connect_worktrees,
            respawn_missing_agents: fake_respawn_missing_agents,
        },
    };

    run_cli(&cli, config, &state_path, None, &deps)?;
    assert_eq!(MIGRATE_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(CMD_RESET_CALLS.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn test_run_cli_default_runs_cmd_default() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let cli = Cli::parse_from(["tenex"]);
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let state_path = dir.path().join("state.json");

    let deps = CliDeps {
        migrate_default_state_dir: fake_migrate_err,
        run_mux_daemon: fake_run_mux_daemon,
        cmd_reset: fake_cmd_reset,
        load_settings: fake_load_settings,
        interactive_deps: InteractiveDeps {
            check_for_update: fake_check_for_update_none,
            run_tui: fake_run_tui_none,
            install_latest: fake_install_latest,
            restart_current_process: fake_restart_process,
            auto_connect_worktrees: fake_auto_connect_worktrees,
            respawn_missing_agents: fake_respawn_missing_agents,
        },
    };

    let result = run_cli(&cli, config, &state_path, None, &deps);
    std::env::set_current_dir(cwd)?;
    result
}

#[test]
fn test_run_cli_default_calls_migration_when_ok() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    MIGRATE_CALLS.store(0, Ordering::SeqCst);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let cli = Cli::parse_from(["tenex"]);
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let state_path = dir.path().join("state.json");

    let deps = CliDeps {
        migrate_default_state_dir: fake_migrate_ok_counted,
        run_mux_daemon: fake_run_mux_daemon,
        cmd_reset: fake_cmd_reset,
        load_settings: fake_load_settings,
        interactive_deps: InteractiveDeps {
            check_for_update: fake_check_for_update_none,
            run_tui: fake_run_tui_none,
            install_latest: fake_install_latest,
            restart_current_process: fake_restart_process,
            auto_connect_worktrees: fake_auto_connect_worktrees,
            respawn_missing_agents: fake_respawn_missing_agents,
        },
    };

    let result = run_cli(&cli, config, &state_path, None, &deps);
    std::env::set_current_dir(cwd)?;
    result?;

    assert_eq!(MIGRATE_CALLS.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn test_cmd_default_saves_after_backfill() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let state_path = dir.path().join("state.json");
    let worktree_dir = dir.path().join("worktrees");
    let config = Config {
        worktree_dir,
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    let agent = tenex::Agent::new(
        "Agent".to_string(),
        "claude".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    );
    let agent_id = agent.id;
    storage.add(agent);
    storage.save()?;

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = cmd_default(config, &state_path, None, Settings::default(), &deps);
    std::env::set_current_dir(cwd)?;
    result?;

    let updated = Storage::load_from(&state_path)?;
    let agent = missing(updated.get_by_index(0), "missing agent after reload")?;
    assert_eq!(agent.id, agent_id);
    let expected = agent_id.to_string();
    assert_eq!(agent.conversation_id.as_deref(), Some(expected.as_str()));
    Ok(())
}

#[test]
fn test_cmd_default_errors_when_worktree_dir_is_file() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    let worktree_dir = dir.path().join("worktrees");

    std::fs::write(&worktree_dir, b"not a dir")?;

    let config = Config {
        worktree_dir,
        ..Config::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let err = cmd_default(config, &state_path, None, Settings::default(), &deps)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

    assert!(err.contains("Failed to create worktrees directory"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_cmd_default_reports_error_when_backfill_save_fails()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;

    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let dir = TempDir::new()?;
    let state_dir = dir.path().join("state");
    std::fs::create_dir_all(&state_dir)?;

    let state_path = state_dir.join("state.json");
    let worktree_dir = dir.path().join("worktrees");
    std::fs::create_dir_all(&worktree_dir)?;

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    let agent = tenex::Agent::new(
        "Agent".to_string(),
        "claude".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    );
    storage.add(agent);
    storage.save()?;

    let original_perms = std::fs::metadata(&state_dir)?.permissions();
    let mut perms = original_perms.clone();
    perms.set_mode(0o555);
    std::fs::set_permissions(&state_dir, perms)?;

    let config = Config {
        worktree_dir,
        ..Config::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let err = cmd_default(
        config,
        &state_path,
        Some("tenex-test-mux-socket"),
        Settings::default(),
        &deps,
    )
    .err()
    .map(|err| err.to_string())
    .unwrap_or_default();

    std::fs::set_permissions(&state_dir, original_perms)?;

    assert!(err.contains("Failed"));
    Ok(())
}

#[cfg(unix)]
fn fake_exec_restart(_installed: std::path::PathBuf, _args: Vec<String>) -> std::io::Error {
    std::io::Error::other("boom")
}

#[cfg(unix)]
#[test]
fn test_restart_current_process_can_use_exec_override() -> Result<()> {
    let _ = restart_exec_override();
    let lock = RESTART_EXEC_OVERRIDE
        .get()
        .ok_or_else(|| anyhow::anyhow!("restart exec override missing"))?;
    {
        let mut guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(fake_exec_restart);
    }

    let err = restart_current_process()
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

    {
        let mut guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = None;
    }

    assert!(err.contains("Failed to restart Tenex"));

    Ok(())
}

// Note: test_cmd_reset_force moved to tests/cli_binary_test.rs
// to properly isolate state via subprocess + TENEX_STATE_PATH env var.
// Running cmd_reset directly in a unit test would corrupt real state.

#[test]
fn test_init_logging_covers_warning_and_debug_branches() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let tmp_file = dir.path().join("not-a-directory");
    std::fs::write(&tmp_file, "x")?;
    let bad_log_path = tmp_file.join("tenex.log");

    init_logging_with(&bad_log_path, Some("not-a-number"));
    init_logging_with(&dir.path().join("tenex.log"), Some("1"));
    init_logging_with(&dir.path().join("tenex.log"), Some("2"));
    init_logging_with(&dir.path().join("tenex.log"), Some("3"));

    Ok(())
}

#[test]
fn test_init_logging_handles_log_path_without_parent() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let original_cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    init_logging_with(std::path::Path::new("/"), Some("1"));

    std::env::set_current_dir(original_cwd)?;
    Ok(())
}

#[test]
fn test_load_storage_with_error_returns_empty_when_state_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");

    let (storage, err) =
        load_storage_result_with_error(&state_path, Ok(Storage::with_path(state_path.clone())));
    assert!(storage.is_empty());
    assert!(err.is_none());

    Ok(())
}

#[test]
fn test_load_storage_with_error_does_not_report_preserve_failure_when_state_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");

    let (storage, err) = load_storage_result_with_error(&state_path, Err(anyhow::anyhow!("boom")));
    assert!(storage.is_empty());

    let err = missing(err, "expected load_storage_result_with_error message")?;
    assert!(err.contains("Failed to load state file"));
    assert!(!err.contains("Preserved unreadable state"));
    assert!(!err.contains("Failed to preserve unreadable state file"));

    Ok(())
}

#[test]
fn test_load_storage_with_error_preserves_corrupt_state_and_backup()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    let backup_path = dir.path().join("state.json.bak");
    std::fs::write(&state_path, "{")?;
    std::fs::write(&backup_path, "{")?;

    let (storage, err) =
        load_storage_result_with_error(&state_path, Storage::load_from(&state_path));
    assert!(storage.is_empty());
    let err = missing(err, "expected load_storage_result_with_error message")?;
    assert!(err.contains("Failed to load state file"));
    assert!(err.contains("Preserved unreadable state"));
    assert!(!state_path.exists());
    assert!(!backup_path.exists());

    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir.path())? {
        let entry = entry?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("state.json.corrupt-"))
    );
    assert!(
        names
            .iter()
            .any(|name| name.starts_with("state.json.bak.corrupt-"))
    );

    Ok(())
}

#[test]
fn test_load_storage_with_error_reports_failed_preserve_when_state_path_has_no_file_name() {
    let (_storage, err) =
        load_storage_result_with_error(std::path::Path::new("/"), Err(anyhow::anyhow!("boom")));
    let err = err.unwrap_or_default();
    assert!(err.contains("Failed to preserve unreadable state file"));
}

#[test]
fn test_ensure_instance_initialized_errors_when_worktree_dir_is_file() -> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;

    let worktree_dir = dir.path().join("not-a-dir");
    std::fs::write(&worktree_dir, "x")?;

    let config = Config {
        worktree_dir,
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    let err = error_string(ensure_instance_initialized(
        &config,
        &mut storage,
        &state_path,
        None,
    ));
    assert!(err.contains("Failed to create worktrees directory"));
    Ok(())
}

#[test]
fn test_ensure_instance_initialized_sets_mux_socket_when_agents_exist()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());

    let mut agent = tenex::Agent::new(
        "Agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    );
    agent.mux_session = format!("{}{}", storage.instance_session_prefix(), agent.short_id());
    storage.add(agent);

    ensure_instance_initialized(&config, &mut storage, &state_path, None)?;
    assert!(
        storage
            .mux_socket
            .as_deref()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(state_path.exists());
    Ok(())
}

#[test]
fn test_ensure_instance_initialized_reuses_stored_mux_socket_for_agents()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;
    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = Some("tenex-missing-socket".to_string());

    let mut agent = tenex::Agent::new(
        "Agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    );
    agent.mux_session = format!("{}{}", storage.instance_session_prefix(), agent.short_id());
    storage.add(agent);

    ensure_instance_initialized(&config, &mut storage, &state_path, None)?;
    assert_eq!(storage.mux_socket.as_deref(), Some("tenex-missing-socket"));
    assert!(state_path.exists());
    Ok(())
}

#[test]
fn test_preserve_corrupt_state_file_renames_file() -> Result<()> {
    let dir = TempDir::new()?;
    let path = dir.path().join("state.json");
    std::fs::write(&path, "boom")?;

    let preserved = missing(
        preserve_corrupt_state_file(&path),
        "expected preserve_corrupt_state_file to rename file",
    )?;

    assert!(!path.exists());
    assert!(preserved.exists());
    assert_eq!(std::fs::read_to_string(&preserved)?, "boom");

    Ok(())
}

#[test]
fn test_preserve_corrupt_state_file_returns_none_for_root_path() {
    assert!(preserve_corrupt_state_file(std::path::Path::new("/")).is_none());
}

#[test]
fn test_preserve_corrupt_state_file_returns_none_when_unix_timestamp_unavailable() -> Result<()> {
    fn now_before_unix_epoch() -> std::time::SystemTime {
        std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)
    }

    let dir = TempDir::new()?;
    let path = dir.path().join("state.json");
    std::fs::write(&path, "boom")?;

    assert!(preserve_corrupt_state_file_with(&path, now_before_unix_epoch).is_none());
    assert!(path.exists());

    Ok(())
}

#[test]
fn test_preserve_corrupt_state_file_returns_none_when_rename_fails() -> Result<()> {
    let dir = TempDir::new()?;
    let path = dir.path().join("missing.json");
    assert!(preserve_corrupt_state_file(&path).is_none());

    Ok(())
}

#[test]
fn test_print_reset_plan_with_agents_and_orphans_does_not_panic() {
    let mut storage = Storage::new();
    storage.add(tenex::Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        std::path::PathBuf::from("/tmp/tenex-test-reset-plan"),
    ));

    let orphaned = vec!["tenex-orphan-session".to_string()];
    print_reset_plan(&storage, &orphaned);
}

#[test]
fn test_print_reset_plan_with_empty_storage_and_orphans_does_not_panic() {
    let storage = Storage::new();
    let orphaned = Vec::new();
    print_reset_plan(&storage, &orphaned);
}

#[test]
fn test_ensure_instance_initialized_clears_mux_socket_when_no_agents() -> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");

    std::fs::write(&state_path, "{}")?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = Some("tenex-mux-stale.sock".to_string());

    ensure_instance_initialized(&config, &mut storage, &state_path, None)?;

    assert!(storage.is_empty());
    assert!(storage.mux_socket.is_none());

    let loaded = Storage::load_from(&state_path)?;
    assert!(loaded.is_empty());
    assert!(loaded.mux_socket.is_none());

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_does_not_write_state_when_empty_and_unchanged() -> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = None;

    ensure_instance_initialized(&config, &mut storage, &state_path, None)?;

    assert_eq!(storage.instance_id.as_deref(), Some("deadbeef"));
    assert_eq!(std::fs::read_to_string(&state_path)?, "{}");

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_skips_mux_socket_updates_when_env_mux_socket_provided()
-> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = None;

    ensure_instance_initialized(
        &config,
        &mut storage,
        &state_path,
        Some("tenex-test-socket"),
    )?;

    assert_eq!(storage.instance_id.as_deref(), Some("deadbeef"));
    assert_eq!(std::fs::read_to_string(&state_path)?, "{}");

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_keeps_mux_socket_none_when_socket_display_errors() -> Result<()>
{
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());

    let mut agent = tenex::Agent::new(
        "Agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    );
    agent.parent_id = Some(uuid::Uuid::new_v4());
    storage.add(agent);

    ensure_instance_initialized_with(&config, &mut storage, &state_path, None, socket_display_err)?;

    assert!(storage.mux_socket.is_none());
    assert_eq!(std::fs::read_to_string(&state_path)?, "{}");

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_saves_state_when_state_file_missing_in_final_save_path()
-> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());

    ensure_instance_initialized(
        &config,
        &mut storage,
        &state_path,
        Some("tenex-test-socket"),
    )?;

    assert!(state_path.exists());
    let persisted = std::fs::read_to_string(&state_path)?;
    assert!(persisted.contains("instance_id"));

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_saves_state_when_instance_id_changes_in_final_save_path()
-> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    assert!(storage.instance_id.is_none());

    ensure_instance_initialized(
        &config,
        &mut storage,
        &state_path,
        Some("tenex-test-socket"),
    )?;

    let persisted = std::fs::read_to_string(&state_path)?;
    assert!(persisted.contains("instance_id"));

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_saves_state_when_instance_id_was_missing() -> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::write(&state_path, "{}")?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    assert!(storage.instance_id.is_none());

    ensure_instance_initialized(&config, &mut storage, &state_path, None)?;

    let persisted = std::fs::read_to_string(&state_path)?;
    assert!(persisted.contains("instance_id"));

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_errors_when_save_to_fails_in_early_return_path() -> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::create_dir_all(&state_path)?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = Some("tenex-mux-stale.sock".to_string());

    let err = ensure_instance_initialized(&config, &mut storage, &state_path, None)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(!err.is_empty());

    Ok(())
}

#[test]
fn test_ensure_instance_initialized_errors_when_save_to_fails_in_final_save_path() -> Result<()> {
    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::create_dir_all(&state_path)?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };

    let mut storage = Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = Some("tenex-mux-stale.sock".to_string());

    let err = ensure_instance_initialized(
        &config,
        &mut storage,
        &state_path,
        Some("tenex-test-env-socket"),
    )
    .err()
    .map(|err| err.to_string())
    .unwrap_or_default();
    assert!(!err.is_empty());

    Ok(())
}

#[test]
fn test_run_interactive_installs_and_restarts_after_update_prompt() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
    RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings::default();

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_available,
        run_tui: fake_run_tui_returns_update,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(cwd)?;

    result?;
    assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 1);

    Ok(())
}

#[test]
fn test_run_interactive_continues_when_update_check_fails() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
    RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings::default();

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_error,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(cwd)?;

    result?;
    assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 0);

    Ok(())
}

#[test]
fn test_run_interactive_installs_and_restarts_when_tui_requests_update() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
    RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings::default();

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_returns_update,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(cwd)?;

    result?;
    assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 1);

    Ok(())
}

#[test]
fn test_run_interactive_propagates_install_latest_errors_when_tui_requests_update() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
    RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_returns_update,
        install_latest: fake_install_latest_err,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let err = run_interactive(config, storage, settings, None, &deps)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

    std::env::set_current_dir(cwd)?;
    assert!(err.contains("boom"));
    assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 0);

    Ok(())
}

#[test]
fn test_run_interactive_propagates_restart_process_errors_when_tui_requests_update() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
    RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_returns_update,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process_err,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let err = run_interactive(config, storage, settings, None, &deps)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

    std::env::set_current_dir(cwd)?;
    assert!(err.contains("boom"));
    assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 0);

    Ok(())
}

#[test]
fn test_run_interactive_passes_storage_load_error_through_to_app() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings::default();

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, Some("boom".to_string()), &deps);
    std::env::set_current_dir(cwd)?;

    result?;

    Ok(())
}

#[test]
fn test_run_interactive_warns_when_auto_connect_fails() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: auto_connect_err,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(cwd)?;
    result?;

    Ok(())
}

#[test]
fn test_run_interactive_warns_when_respawn_fails() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: respawn_err,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(cwd)?;
    result?;

    Ok(())
}

#[test]
fn test_run_interactive_warns_when_excluding_tenex_from_git_fails() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let original_cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    let repo = tenex::git::Repository::init(dir.path())?;

    let info_dir = repo.path().join("info");
    std::fs::remove_dir_all(&info_dir)?;
    std::fs::write(&info_dir, "not-a-dir")?;

    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(original_cwd)?;
    result?;

    Ok(())
}

#[test]
fn test_run_interactive_does_not_warn_when_excluding_tenex_from_git_succeeds() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let original_cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    let _repo = tenex::git::Repository::init(dir.path())?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(original_cwd)?;
    result?;

    Ok(())
}

#[test]
fn test_run_interactive_handles_current_dir_failure() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let original_cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    let gone = dir.path().join("gone");
    std::fs::create_dir_all(&gone)?;
    std::env::set_current_dir(&gone)?;
    std::fs::remove_dir_all(&gone)?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: fake_run_tui_none,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let result = run_interactive(config, storage, settings, None, &deps);
    std::env::set_current_dir(original_cwd)?;
    result?;

    Ok(())
}

#[test]
fn test_run_interactive_propagates_run_tui_errors() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings {
        last_seen_version: Some(tenex::release_notes::current_version()?.to_string()),
        ..Settings::default()
    };

    let deps = InteractiveDeps {
        check_for_update: fake_check_for_update_none,
        run_tui: run_tui_err,
        install_latest: fake_install_latest,
        restart_current_process: fake_restart_process,
        auto_connect_worktrees: fake_auto_connect_worktrees,
        respawn_missing_agents: fake_respawn_missing_agents,
    };

    let err = run_interactive(config, storage, settings, None, &deps)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    std::env::set_current_dir(cwd)?;
    assert!(err.contains("boom"));

    Ok(())
}

#[test]
fn test_find_installed_binary_falls_back_to_name_when_not_installed() {
    let name = format!("tenex-test-missing-binary-{}", uuid::Uuid::new_v4());
    assert_eq!(
        find_installed_binary(&name),
        std::path::PathBuf::from(&name)
    );
}

#[test]
fn test_find_installed_binary_uses_cargo_home_candidate_when_present() -> Result<()> {
    let dir = TempDir::new()?;
    let cargo_home = dir.path().join("cargo-home");
    let bin_dir = cargo_home.join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let name = format!("tenex-test-installed-binary-{}", uuid::Uuid::new_v4());
    let installed = bin_dir.join(&name);
    std::fs::write(&installed, "not-a-binary")?;

    assert_eq!(
        find_installed_binary_with_cargo_home(&name, &cargo_home),
        installed
    );

    Ok(())
}

#[test]
fn test_prompt_reset_scope_with_io_returns_this_instance_when_force_true() -> Result<()> {
    let mut input = String::new();
    let scope = prompt_reset_scope_with_io(true, flush_stdout_err, read_line_err, &mut input)?;
    assert_eq!(scope, ResetScope::ThisInstance);

    Ok(())
}

#[test]
fn test_prompt_reset_scope_with_io_propagates_flush_errors() {
    let mut input = String::new();
    let err = prompt_reset_scope_with_io(false, flush_stdout_err, read_line_err, &mut input)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(!err.is_empty());
}

#[test]
fn test_prompt_reset_scope_with_io_propagates_read_line_errors() {
    let mut input = String::new();
    let err = prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_err, &mut input)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(!err.is_empty());
}

#[test]
fn test_prompt_reset_scope_with_io_accepts_all_instances_numeric() -> Result<()> {
    fn read_line_two(input: &mut String) -> std::io::Result<usize> {
        input.clear();
        input.push_str("2\n");
        io_ok(2)
    }

    let mut input = String::new();
    let scope = prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_two, &mut input)?;
    assert_eq!(scope, ResetScope::AllInstances);

    Ok(())
}

#[test]
fn test_prompt_reset_scope_with_io_accepts_all_instances_text() -> Result<()> {
    fn read_line_all(input: &mut String) -> std::io::Result<usize> {
        input.clear();
        input.push_str("all\n");
        io_ok(4)
    }

    let mut input = String::new();
    let scope = prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_all, &mut input)?;
    assert_eq!(scope, ResetScope::AllInstances);

    Ok(())
}

#[test]
fn test_prompt_reset_scope_with_io_defaults_to_this_instance_for_blank_input() -> Result<()> {
    fn read_line_blank(input: &mut String) -> std::io::Result<usize> {
        input.clear();
        input.push('\n');
        io_ok(1)
    }

    let mut input = String::new();
    let scope = prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_blank, &mut input)?;
    assert_eq!(scope, ResetScope::ThisInstance);

    Ok(())
}

#[test]
fn test_confirm_reset_with_io_returns_true_when_force_true() -> Result<()> {
    let mut input = String::new();
    let result = confirm_reset_with_io(true, flush_stdout_err, read_line_err, &mut input)?;
    assert!(result);

    Ok(())
}

#[test]
fn test_confirm_reset_with_io_propagates_flush_errors() {
    let mut input = String::new();
    let err = confirm_reset_with_io(false, flush_stdout_err, read_line_err, &mut input)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(!err.is_empty());
}

#[test]
fn test_confirm_reset_with_io_propagates_read_line_errors() {
    let mut input = String::new();
    let err = confirm_reset_with_io(false, flush_stdout_ok, read_line_err, &mut input)
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();
    assert!(!err.is_empty());
}

#[test]
fn test_confirm_reset_with_io_returns_true_for_yes() -> Result<()> {
    fn read_line_yes(input: &mut String) -> std::io::Result<usize> {
        input.clear();
        input.push_str("y\n");
        io_ok(2)
    }

    let mut input = String::new();
    let result = confirm_reset_with_io(false, flush_stdout_ok, read_line_yes, &mut input)?;
    assert!(result);

    Ok(())
}

#[test]
fn test_confirm_reset_with_io_returns_false_for_other_input() -> Result<()> {
    fn read_line_no(input: &mut String) -> std::io::Result<usize> {
        input.clear();
        input.push_str("n\n");
        io_ok(2)
    }

    let mut input = String::new();
    let result = confirm_reset_with_io(false, flush_stdout_ok, read_line_no, &mut input)?;
    assert!(!result);

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_prints_warnings_for_failed_cleanup_steps() -> Result<()> {
    fn mux_is_running_true() -> bool {
        true
    }

    fn mux_list_sessions_stub(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
        let prefix = "tenex-deadbeef-";
        ok(vec![
            tenex::mux::Session {
                name: format!("{prefix}orphan"),
                created: 0,
                attached: false,
            },
            tenex::mux::Session {
                name: format!("{prefix}deadbeef"),
                created: 0,
                attached: false,
            },
            tenex::mux::Session {
                name: "not-tenex".to_string(),
                created: 0,
                attached: false,
            },
        ])
    }

    fn mux_kill_session_err(_mux: SessionManager, _name: &str) -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn cleanup_agent_runtime_err(_agent: &tenex::Agent) -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn remove_worktree_err(_mgr: &tenex::git::WorktreeManager<'_>, _branch: &str) -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn delete_branch_err(_mgr: &tenex::git::BranchManager<'_>, _branch: &str) -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    let dir = TempDir::new()?;
    let repo_dir = dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir)?;
    let _repo = tenex::git::Repository::init(&repo_dir)?;

    set_reset_test_current_dir(&repo_dir);

    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());

    let mut agent = tenex::Agent::new(
        "Agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        repo_dir.join("wt"),
    );
    agent.mux_session = format!("{}{}", storage.instance_session_prefix(), agent.short_id());
    storage.add(agent);

    let deps = ResetDeps {
        mux_is_running: mux_is_running_true,
        mux_list_sessions: mux_list_sessions_stub,
        mux_kill_session: mux_kill_session_err,
        cleanup_agent_runtime: cleanup_agent_runtime_err,
        current_dir: reset_test_current_dir,
        open_repository: tenex::git::open_repository,
        remove_worktree: remove_worktree_err,
        delete_branch: delete_branch_err,
    };

    let mux = SessionManager::new();
    cmd_reset_with_storage(true, &mut storage, mux, deps)?;

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_covers_successful_cleanup_branches() -> Result<()> {
    fn mux_is_running_true() -> bool {
        true
    }

    fn mux_list_sessions_empty(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
        ok(Vec::new())
    }

    fn mux_kill_session_ok(_mux: SessionManager, _name: &str) -> Result<()> {
        ok(())
    }

    fn cleanup_agent_runtime_ok(_agent: &tenex::Agent) -> Result<()> {
        ok(())
    }

    fn remove_worktree_ok(_mgr: &tenex::git::WorktreeManager<'_>, _branch: &str) -> Result<()> {
        ok(())
    }

    fn delete_branch_ok(_mgr: &tenex::git::BranchManager<'_>, _branch: &str) -> Result<()> {
        ok(())
    }

    let dir = TempDir::new()?;
    let repo_dir = dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir)?;
    let _repo = tenex::git::Repository::init(&repo_dir)?;

    set_reset_test_current_dir(&repo_dir);

    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());

    let mut agent = tenex::Agent::new(
        "Agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        repo_dir.join("wt"),
    );
    agent.mux_session = format!("{}{}", storage.instance_session_prefix(), agent.short_id());
    storage.add(agent);

    let deps = ResetDeps {
        mux_is_running: mux_is_running_true,
        mux_list_sessions: mux_list_sessions_empty,
        mux_kill_session: mux_kill_session_ok,
        cleanup_agent_runtime: cleanup_agent_runtime_ok,
        current_dir: reset_test_current_dir,
        open_repository: tenex::git::open_repository,
        remove_worktree: remove_worktree_ok,
        delete_branch: delete_branch_ok,
    };

    let mux = SessionManager::new();
    cmd_reset_with_storage(true, &mut storage, mux, deps)?;
    assert!(storage.is_empty());

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_noops_when_empty_and_no_orphans_and_no_mux_socket() -> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        mux_list_sessions: mux_list_sessions_for_reset_deps,
        mux_kill_session: mux_kill_session_for_reset_deps,
        cleanup_agent_runtime: tenex::cleanup_agent_runtime,
        current_dir: reset_test_current_dir,
        open_repository: tenex::git::open_repository,
        remove_worktree: remove_worktree_for_reset_deps,
        delete_branch: delete_branch_for_reset_deps,
    };

    let mux = SessionManager::new();
    cmd_reset_with_storage(true, &mut storage, mux, deps)?;

    assert!(storage.is_empty());
    assert!(storage.mux_socket.is_none());

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_kills_orphaned_sessions_when_storage_empty() -> Result<()> {
    fn mux_is_running_true() -> bool {
        true
    }

    fn mux_list_sessions_stub(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
        ok(vec![tenex::mux::Session {
            name: "tenex-deadbeef-orphan".to_string(),
            created: 0,
            attached: false,
        }])
    }

    fn mux_kill_session_ok(_mux: SessionManager, _name: &str) -> Result<()> {
        ok(())
    }

    let dir = TempDir::new()?;
    let repo_dir = dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir)?;
    let _repo = tenex::git::Repository::init(&repo_dir)?;

    set_reset_test_current_dir(&repo_dir);

    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());

    let deps = ResetDeps {
        mux_is_running: mux_is_running_true,
        mux_list_sessions: mux_list_sessions_stub,
        mux_kill_session: mux_kill_session_ok,
        cleanup_agent_runtime: tenex::cleanup_agent_runtime,
        current_dir: reset_test_current_dir,
        open_repository: tenex::git::open_repository,
        remove_worktree: remove_worktree_for_reset_deps,
        delete_branch: delete_branch_for_reset_deps,
    };

    let mux = SessionManager::new();
    cmd_reset_with_storage(true, &mut storage, mux, deps)?;

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_skips_session_kills_when_mux_not_running() -> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    fn cleanup_agent_runtime_ok(_agent: &tenex::Agent) -> Result<()> {
        ok(())
    }

    fn open_repository_err(_path: &std::path::Path) -> Result<tenex::git::Repository> {
        Err(anyhow::anyhow!("boom"))
    }

    let dir = TempDir::new()?;
    let repo_dir = dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir)?;

    set_reset_test_current_dir(&repo_dir);

    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());
    storage.add(tenex::Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    ));

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        mux_list_sessions: mux_list_sessions_for_reset_deps,
        mux_kill_session: mux_kill_session_for_reset_deps,
        cleanup_agent_runtime: cleanup_agent_runtime_ok,
        current_dir: reset_test_current_dir,
        open_repository: open_repository_err,
        remove_worktree: remove_worktree_for_reset_deps,
        delete_branch: delete_branch_for_reset_deps,
    };

    let mux = SessionManager::new();
    cmd_reset_with_storage(true, &mut storage, mux, deps)?;
    assert!(storage.is_empty());

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_with_prompts_propagates_prompt_reset_scope_errors() -> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    fn prompt_reset_scope_err(_force: bool) -> Result<ResetScope> {
        Err(anyhow::anyhow!("boom"))
    }

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        ..ResetDeps::production()
    };

    let mux = SessionManager::new();
    let err = error_string(cmd_reset_with_storage_with_prompts(
        false,
        &mut storage,
        mux,
        deps,
        prompt_reset_scope_err,
        confirm_reset,
    ));
    assert!(err.contains("boom"));

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_with_prompts_propagates_confirm_reset_errors() -> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
        ok(ResetScope::ThisInstance)
    }

    fn confirm_reset_err(_force: bool) -> Result<bool> {
        Err(anyhow::anyhow!("boom"))
    }

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());
    storage.add(tenex::Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    ));

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        mux_list_sessions: mux_list_sessions_for_reset_deps,
        mux_kill_session: mux_kill_session_for_reset_deps,
        cleanup_agent_runtime: tenex::cleanup_agent_runtime,
        current_dir: reset_test_current_dir,
        open_repository: tenex::git::open_repository,
        remove_worktree: remove_worktree_for_reset_deps,
        delete_branch: delete_branch_for_reset_deps,
    };

    let mux = SessionManager::new();
    let err = error_string(cmd_reset_with_storage_with_prompts(
        false,
        &mut storage,
        mux,
        deps,
        prompt_reset_scope_ok,
        confirm_reset_err,
    ));
    assert!(err.contains("boom"));

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_with_prompts_propagates_current_dir_errors() -> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
        ok(ResetScope::ThisInstance)
    }

    fn confirm_reset_ok(_force: bool) -> Result<bool> {
        ok(true)
    }

    fn current_dir_err() -> Result<std::path::PathBuf> {
        Err(anyhow::anyhow!("boom"))
    }

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());
    storage.add(tenex::Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    ));

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        mux_list_sessions: mux_list_sessions_for_reset_deps,
        mux_kill_session: mux_kill_session_for_reset_deps,
        cleanup_agent_runtime: tenex::cleanup_agent_runtime,
        current_dir: current_dir_err,
        open_repository: tenex::git::open_repository,
        remove_worktree: remove_worktree_for_reset_deps,
        delete_branch: delete_branch_for_reset_deps,
    };

    let mux = SessionManager::new();
    let err = error_string(cmd_reset_with_storage_with_prompts(
        false,
        &mut storage,
        mux,
        deps,
        prompt_reset_scope_ok,
        confirm_reset_ok,
    ));
    assert!(err.contains("boom"));

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_with_prompts_noops_can_fail_when_storage_save_fails() -> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
        ok(ResetScope::ThisInstance)
    }

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::create_dir_all(&state_path)?;

    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());
    storage.mux_socket = Some("dummy".to_string());

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        ..ResetDeps::production()
    };

    let mux = SessionManager::new();
    let err = error_string(cmd_reset_with_storage_with_prompts(
        true,
        &mut storage,
        mux,
        deps,
        prompt_reset_scope_ok,
        confirm_reset,
    ));
    assert!(!err.is_empty());

    Ok(())
}

#[test]
fn test_cmd_reset_with_storage_with_prompts_propagates_storage_save_errors_after_clear()
-> Result<()> {
    fn mux_is_running_false() -> bool {
        false
    }

    fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
        ok(ResetScope::ThisInstance)
    }

    fn confirm_reset_ok(_force: bool) -> Result<bool> {
        ok(true)
    }

    fn cleanup_agent_runtime_ok(_agent: &tenex::Agent) -> Result<()> {
        ok(())
    }

    fn current_dir_ok() -> Result<std::path::PathBuf> {
        ok(std::path::PathBuf::from("/tmp"))
    }

    fn open_repository_err(_path: &std::path::Path) -> Result<tenex::git::Repository> {
        Err(anyhow::anyhow!("boom"))
    }

    let dir = TempDir::new()?;
    let state_path = dir.path().join("state.json");
    std::fs::create_dir_all(&state_path)?;

    let mut storage = Storage::with_path(state_path);
    storage.instance_id = Some("deadbeef".to_string());
    storage.add(tenex::Agent::new(
        "agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        dir.path().join("wt"),
    ));

    let deps = ResetDeps {
        mux_is_running: mux_is_running_false,
        mux_list_sessions: mux_list_sessions_for_reset_deps,
        mux_kill_session: mux_kill_session_for_reset_deps,
        cleanup_agent_runtime: cleanup_agent_runtime_ok,
        current_dir: current_dir_ok,
        open_repository: open_repository_err,
        remove_worktree: remove_worktree_for_reset_deps,
        delete_branch: delete_branch_for_reset_deps,
    };

    let mux = SessionManager::new();
    let err = error_string(cmd_reset_with_storage_with_prompts(
        true,
        &mut storage,
        mux,
        deps,
        prompt_reset_scope_ok,
        confirm_reset_ok,
    ));
    assert!(!err.is_empty());

    Ok(())
}

#[test]
fn test_list_orphaned_sessions_with_can_return_sessions_when_listing_errors() {
    fn list_sessions_err(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
        Err(anyhow::anyhow!("boom"))
    }

    let mux = SessionManager::new();
    let storage_sessions = std::collections::HashSet::new();
    let orphaned = list_orphaned_sessions_with(
        mux,
        true,
        ResetScope::ThisInstance,
        "tenex-deadbeef-",
        &storage_sessions,
        list_sessions_err,
    );
    assert!(orphaned.is_empty());
}

#[test]
fn test_list_orphaned_sessions_with_all_instances_filters_sessions() {
    fn list_sessions_ok(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
        ok(vec![
            tenex::mux::Session {
                name: "tenex-orphan".to_string(),
                created: 0,
                attached: false,
            },
            tenex::mux::Session {
                name: "other".to_string(),
                created: 0,
                attached: false,
            },
        ])
    }

    let mux = SessionManager::new();
    let storage_sessions = std::collections::HashSet::new();
    let orphaned = list_orphaned_sessions_with(
        mux,
        true,
        ResetScope::AllInstances,
        "unused",
        &storage_sessions,
        list_sessions_ok,
    );
    assert_eq!(orphaned, vec!["tenex-orphan".to_string()]);
}

#[test]
fn test_reset_deps_production_helpers_smoke() {
    let _ = mux_list_sessions_for_reset_deps(SessionManager::new());
    let _ = mux_kill_session_for_reset_deps(SessionManager::new(), "missing-session");
}

#[test]
fn test_maybe_queue_whats_new_handles_corrupt_last_seen_version() {
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("not-semver".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new(&mut app);
    assert!(app.data.pending_changelog.is_some());
}

#[test]
fn test_maybe_queue_whats_new_queues_release_notes_when_outdated() {
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("0.0.0".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new(&mut app);
    assert!(app.data.pending_changelog.is_some());
}

#[test]
fn test_maybe_queue_whats_new_returns_when_already_seen_current() -> Result<()> {
    let current = tenex::release_notes::current_version()?;
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some(current.to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new(&mut app);
    assert!(app.data.pending_changelog.is_none());

    Ok(())
}

#[test]
fn test_maybe_queue_whats_new_with_uses_changelog_lines_for_corrupt_last_seen() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("not-semver".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_lines_empty,
        whats_new_lines_empty,
        set_last_seen_ok,
    );

    assert!(app.data.pending_changelog.is_some());
}

#[test]
fn test_maybe_queue_whats_new_with_uses_whats_new_lines_for_outdated_last_seen() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("0.0.0".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_lines_empty,
        whats_new_lines_empty,
        set_last_seen_ok,
    );

    assert!(app.data.pending_changelog.is_some());
}

#[test]
fn test_maybe_queue_whats_new_returns_when_current_version_errors() {
    fn current_version_err() -> Result<Version> {
        Err(anyhow::anyhow!("boom"))
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    maybe_queue_whats_new_with(
        &mut app,
        current_version_err,
        changelog_lines_empty,
        whats_new_lines_empty,
        set_last_seen_ok,
    );
    assert!(app.data.pending_changelog.is_none());
}

#[test]
fn test_maybe_queue_whats_new_warns_when_settings_save_fails_for_missing_last_seen() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_lines_empty,
        whats_new_lines_empty,
        set_last_seen_err,
    );

    assert_eq!(
        app.data.settings.last_seen_version.as_deref(),
        Some("1.0.0")
    );
    assert!(app.data.pending_changelog.is_none());
}

#[test]
fn test_maybe_queue_whats_new_warns_when_release_notes_generation_fails_for_corrupt_last_seen() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    fn changelog_err(_version: &Version) -> Result<Vec<String>> {
        Err(anyhow::anyhow!("boom"))
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("not-semver".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_err,
        whats_new_lines_empty,
        set_last_seen_ok,
    );
    assert!(app.data.pending_changelog.is_none());
}

#[test]
fn test_maybe_queue_whats_new_warns_when_settings_save_fails_for_future_last_seen() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("2.0.0".to_string()),
            ..Settings::default()
        },
        false,
    );
    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_lines_empty,
        whats_new_lines_empty,
        set_last_seen_err,
    );

    assert_eq!(
        app.data.settings.last_seen_version.as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn test_maybe_queue_whats_new_overwrites_future_last_seen_when_settings_writable() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("2.0.0".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_lines_empty,
        whats_new_lines_empty,
        set_last_seen_ok,
    );

    assert_eq!(
        app.data.settings.last_seen_version.as_deref(),
        Some("1.0.0")
    );
    assert!(app.data.pending_changelog.is_none());
}

#[test]
fn test_maybe_queue_whats_new_warns_when_whats_new_lines_fail() {
    fn current_version_ok() -> Result<Version> {
        ok(Version::parse("1.0.0")?)
    }

    fn whats_new_err(_from: Option<&Version>, _to: &Version) -> Result<Vec<String>> {
        Err(anyhow::anyhow!("boom"))
    }

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings {
            last_seen_version: Some("0.0.0".to_string()),
            ..Settings::default()
        },
        false,
    );

    maybe_queue_whats_new_with(
        &mut app,
        current_version_ok,
        changelog_lines_empty,
        whats_new_err,
        set_last_seen_ok,
    );
    assert!(app.data.pending_changelog.is_none());
}

#[test]
fn test_maybe_prompt_restart_mux_daemon_returns_when_daemon_missing() {
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    maybe_prompt_restart_mux_daemon_with(
        &mut app,
        mux_version_ok,
        running_daemon_none,
        socket_display_ok,
        env_mux_socket_none,
    );
    assert!(app.data.ui.muxd_version_mismatch.is_none());
}

#[test]
fn test_maybe_prompt_restart_mux_daemon_prompts_on_version_mismatch() {
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    maybe_prompt_restart_mux_daemon_for_versions_with(
        &mut app,
        "daemon-version".to_string(),
        "expected-version".to_string(),
        socket_display_ok,
        env_mux_socket,
    );

    assert!(app.data.ui.muxd_version_mismatch.is_some());
    assert_eq!(
        std::mem::discriminant(&app.mode),
        std::mem::discriminant(&AppMode::Confirming(ConfirmingMode {
            action: ConfirmAction::Quit,
        }))
    );
}

#[test]
fn test_env_mux_socket_trims_and_filters_empty() {
    assert_eq!(
        env_mux_socket_from(|| Some("  tenex.sock  ".to_string())),
        Some("tenex.sock".to_string())
    );
    assert!(env_mux_socket_from(|| Some("   ".to_string())).is_none());
    assert!(env_mux_socket_from(|| None).is_none());
}

#[test]
fn test_default_action_helpers_noop_on_empty_app() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let cwd = std::env::current_dir()?;
    let dir = TempDir::new()?;
    std::env::set_current_dir(dir.path())?;

    let config = Config {
        worktree_dir: dir.path().join("worktrees"),
        ..Config::default()
    };
    let storage = Storage::with_path(dir.path().join("state.json"));
    let settings = Settings::default();
    let mut app = App::new(config, storage, settings, false);

    let auto_connect_result = default_auto_connect_worktrees(&mut app);
    let respawn_result = default_respawn_missing_agents(&mut app);

    std::env::set_current_dir(cwd)?;

    auto_connect_result?;
    respawn_result?;

    Ok(())
}

#[test]
fn test_maybe_prompt_restart_mux_daemon_returns_when_running_daemon_version_fails() {
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    maybe_prompt_restart_mux_daemon_with(
        &mut app,
        mux_version_ok,
        running_daemon_err,
        socket_display_ok,
        env_mux_socket_none,
    );

    assert!(app.data.ui.muxd_version_mismatch.is_none());
}

#[test]
fn test_maybe_prompt_restart_mux_daemon_uses_unknown_socket_when_socket_display_fails() -> Result<()>
{
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    maybe_prompt_restart_mux_daemon_for_versions_with(
        &mut app,
        "daemon-version".to_string(),
        "expected-version".to_string(),
        socket_display_err,
        env_mux_socket_none,
    );

    let mismatch = missing(
        app.data.ui.muxd_version_mismatch,
        "expected muxd mismatch info",
    )?;
    assert_eq!(mismatch.socket, "<unknown>");
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_exec_restart_reports_errors_without_execing() {
    let err = exec_restart(std::path::PathBuf::from("/nonexistent/tenex"), Vec::new());
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn test_restart_current_process_can_fall_back_to_exec_restart_without_execing() -> Result<()> {
    let _guard = run_interactive_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let _ = restart_exec_override();
    let lock = RESTART_EXEC_OVERRIDE
        .get()
        .ok_or_else(|| anyhow::anyhow!("restart exec override missing"))?;
    {
        let mut guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = None;
    }

    let dir = TempDir::new()?;
    let cargo_home = dir.path().join("cargo-home");
    let bin_dir = cargo_home.join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let installed = bin_dir.join(env!("CARGO_PKG_NAME"));
    std::fs::write(&installed, "not-a-binary")?;

    let _ = installed_binary_for_restart(env!("CARGO_PKG_NAME"));
    let installed_override = INSTALLED_BINARY_OVERRIDE
        .get()
        .ok_or_else(|| anyhow::anyhow!("installed binary override missing"))?;
    {
        let mut guard = installed_override
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(installed);
    }

    let err = restart_current_process()
        .err()
        .map(|err| err.to_string())
        .unwrap_or_default();

    {
        let mut guard = installed_override
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = None;
    }

    assert!(err.contains("Failed to restart Tenex"));
    Ok(())
}
