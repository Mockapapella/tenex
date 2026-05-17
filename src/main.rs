//! Tenex - Terminal multiplexer for AI coding agents
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use semver::Version;
use tenex::App;
use tenex::AppMode;
use tenex::agent::Storage;
use tenex::app::{MuxdVersionMismatchInfo, Settings};
use tenex::config::Config;
use tenex::mux::SessionManager;
use tenex::state::{ChangelogMode, ConfirmAction, ConfirmingMode, UpdatePromptMode};

/// Terminal multiplexer for AI coding agents
#[derive(Parser)]
#[command(name = "tenex")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Kill all agents and clear state
    Reset {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
    /// Run the mux daemon (internal).
    #[command(hide = true)]
    Muxd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetScope {
    ThisInstance,
    AllInstances,
}

fn main() -> Result<()> {
    init_logging();

    let cli = parse_cli();
    let config = Config::default();
    let state_path = Config::state_path();
    let env_mux_socket = env_mux_socket();

    let deps = CliDeps {
        migrate_default_state_dir: tenex::migration::migrate_default_state_dir,
        run_mux_daemon: tenex::mux::run_mux_daemon,
        cmd_reset,
        load_settings: Settings::load,
        interactive_deps: InteractiveDeps {
            check_for_update: tenex::update::check_for_update,
            run_tui: tenex::tui::run,
            install_latest: tenex::update::install_latest,
            restart_current_process,
            auto_connect_worktrees: default_auto_connect_worktrees,
            respawn_missing_agents: default_respawn_missing_agents,
        },
    };

    run_cli(&cli, config, &state_path, env_mux_socket.as_deref(), &deps)
}

type MigrateDefaultStateDir = fn() -> Result<()>;
type RunMuxDaemon = fn() -> Result<()>;
type CmdReset = fn(bool) -> Result<()>;
type LoadSettings = fn() -> Settings;

type UpdateCheck = fn() -> Result<Option<tenex::update::UpdateInfo>>;
type TuiRunner = fn(App) -> Result<Option<tenex::update::UpdateInfo>>;
type InstallLatest = fn() -> Result<()>;
type RestartProcess = fn() -> Result<()>;
type AutoConnectWorktrees = fn(&mut App) -> Result<()>;
type RespawnMissingAgents = fn(&mut App) -> Result<()>;

struct CliDeps {
    migrate_default_state_dir: MigrateDefaultStateDir,
    run_mux_daemon: RunMuxDaemon,
    cmd_reset: CmdReset,
    load_settings: LoadSettings,
    interactive_deps: InteractiveDeps,
}

struct InteractiveDeps {
    check_for_update: UpdateCheck,
    run_tui: TuiRunner,
    install_latest: InstallLatest,
    restart_current_process: RestartProcess,
    auto_connect_worktrees: AutoConnectWorktrees,
    respawn_missing_agents: RespawnMissingAgents,
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn run_cli(
    cli: &Cli,
    config: Config,
    state_path: &std::path::Path,
    env_mux_socket: Option<&str>,
    deps: &CliDeps,
) -> Result<()> {
    match &cli.command {
        Some(Commands::Reset { force }) => {
            (deps.migrate_default_state_dir)().unwrap_or_else(|err| warn_migration_failure(&err));
            (deps.cmd_reset)(*force)
        }
        Some(Commands::Muxd) => (deps.run_mux_daemon)(),
        None => {
            (deps.migrate_default_state_dir)().unwrap_or_else(|err| warn_migration_failure(&err));

            let settings = (deps.load_settings)();
            cmd_default(
                config,
                state_path,
                env_mux_socket,
                settings,
                &deps.interactive_deps,
            )
        }
    }
}

fn warn_migration_failure(err: &anyhow::Error) {
    eprintln!("Warning: Failed to migrate Tenex state directory: {err}");
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn cmd_default(
    config: Config,
    state_path: &std::path::Path,
    env_mux_socket: Option<&str>,
    settings: Settings,
    deps: &InteractiveDeps,
) -> Result<()> {
    let (mut storage, storage_load_error) =
        load_storage_result_with_error(state_path, Storage::load_from(state_path));
    ensure_instance_initialized(&config, &mut storage, state_path, env_mux_socket)?;

    let mut did_backfill = storage.backfill_workspace_kinds();
    did_backfill |= storage.backfill_child_titles();
    did_backfill |= storage.backfill_repo_roots();
    did_backfill |= storage.backfill_conversation_ids();
    if did_backfill {
        storage.save_to(state_path)?;
    }

    run_interactive(config, storage, settings, storage_load_error, deps)
}

fn init_logging() {
    let log_path = tenex::paths::log_path();
    let debug = std::env::var("DEBUG").ok();
    init_logging_with(&log_path, debug.as_deref());
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn init_logging_with(log_path: &std::path::Path, debug: Option<&str>) {
    if let Some(parent) = log_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "Warning: Failed to create log directory {}: {e}",
            parent.display()
        );
    }

    // Clear the log file on startup
    if let Err(e) = std::fs::write(log_path, "") {
        eprintln!(
            "Warning: Failed to clear log file {}: {e}",
            log_path.display()
        );
    }

    // Log to tenex.log in the OS temp directory
    // Set DEBUG=0-3 to control verbosity (0=off, 1=warn, 2=info, 3=debug)
    let debug_level = debug.and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);

    if debug_level == 0 {
        return;
    }

    let level = match debug_level {
        1 => tracing::Level::WARN,
        2 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    };

    let log_dir = log_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    let file_appender = tracing_appender::rolling::never(log_dir, "tenex.log");
    let _ = tracing_subscriber::fmt()
        .with_writer(file_appender)
        .with_max_level(level)
        .with_ansi(false)
        .try_init();
}

fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            if e.kind() == clap::error::ErrorKind::DisplayHelp
                || e.kind() == clap::error::ErrorKind::DisplayVersion
            {
                e.exit();
            }

            eprintln!("error: {}\n", e.kind());
            if let Err(err) = Cli::command().print_help() {
                eprintln!("Warning: Failed to print help: {err}");
            }
            std::process::exit(1);
        }
    }
}

fn load_storage_result_with_error(
    state_path: &std::path::Path,
    result: Result<Storage>,
) -> (Storage, Option<String>) {
    match result {
        Ok(storage) => (storage, None),
        Err(err) => {
            let mut message = format!("Failed to load state file {}: {err}", state_path.display());

            if let Some(preserved) = preserve_corrupt_state_file(state_path) {
                use std::fmt::Write as _;
                write!(
                    &mut message,
                    "\nPreserved unreadable state at {}",
                    preserved.display()
                )
                .unwrap_or_default();
            } else if state_path.exists() {
                message.push_str("\nFailed to preserve unreadable state file");
            }

            let file_name = state_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("state.json");
            let backup_path = state_path.with_file_name(format!("{file_name}.bak"));
            if backup_path.exists() {
                let _ = preserve_corrupt_state_file(&backup_path);
            }

            (Storage::new(), Some(message))
        }
    }
}

fn ensure_instance_initialized(
    config: &Config,
    storage: &mut Storage,
    state_path: &std::path::Path,
    env_mux_socket: Option<&str>,
) -> Result<()> {
    ensure_instance_initialized_with(
        config,
        storage,
        state_path,
        env_mux_socket,
        tenex::mux::socket_display,
    )
}

fn ensure_instance_initialized_with(
    config: &Config,
    storage: &mut Storage,
    state_path: &std::path::Path,
    env_mux_socket: Option<&str>,
    socket_display: fn() -> Result<String>,
) -> Result<()> {
    std::fs::create_dir_all(&config.worktree_dir).with_context(|| {
        format!(
            "Failed to create worktrees directory {}",
            config.worktree_dir.display()
        )
    })?;

    let state_existed = state_path.exists();
    let previous_instance_id = storage.instance_id.clone();
    let previous_mux_socket = storage.mux_socket.clone();

    let _ = storage.ensure_instance_id();

    // Persist and reuse a stable mux socket per instance so agents can survive restarts even if
    // the Tenex binary (and thus the default socket fingerprint) changes across rebuilds/upgrades.
    //
    // Allow users to override via TENEX_MUX_SOCKET without mutating the saved configuration.
    if storage.is_empty() {
        storage.mux_socket = None;
    }

    if env_mux_socket.is_none() {
        if storage.is_empty() {
            // No sessions to preserve, so keep the state file free of a pinned mux socket.
            // The mux endpoint will be re-derived when the first agent is created.
            if !state_existed
                || storage.instance_id != previous_instance_id
                || storage.mux_socket != previous_mux_socket
            {
                storage.save_to(state_path)?;
            }
            return Ok(());
        }

        let wanted_sessions: std::collections::HashSet<String> = storage
            .root_agents()
            .into_iter()
            .map(|agent| agent.mux_session.clone())
            .collect();

        let preferred = storage.mux_socket.as_deref();
        let discovered = tenex::mux::discover_socket_for_sessions(&wanted_sessions, preferred);
        let chosen = discovered
            .or_else(|| preferred.map(ToString::to_string))
            .or_else(|| socket_display().ok());

        if storage.mux_socket != chosen {
            storage.mux_socket = chosen;
        }

        if let Some(socket) = storage.mux_socket.as_deref() {
            let _ = tenex::mux::set_socket_override(socket);
        }
    }

    if !state_existed
        || storage.instance_id != previous_instance_id
        || storage.mux_socket != previous_mux_socket
    {
        storage.save_to(state_path)?;
    }

    Ok(())
}

fn default_auto_connect_worktrees(app: &mut App) -> Result<()> {
    tenex::app::Actions::new().auto_connect_worktrees(app)
}

fn default_respawn_missing_agents(app: &mut App) -> Result<()> {
    tenex::app::Actions::new().respawn_missing_agents(app)
}

fn run_interactive(
    config: Config,
    storage: Storage,
    settings: Settings,
    storage_load_error: Option<String>,
    deps: &InteractiveDeps,
) -> Result<()> {
    let cwd = std::env::current_dir().ok();

    let cwd_project_root = cwd
        .as_ref()
        .map(|cwd| tenex::git::repository_workspace_root(cwd).unwrap_or_else(|_| cwd.clone()));

    // Ensure .tenex/ is excluded from git tracking
    if let Some(cwd) = cwd.as_ref()
        && tenex::git::is_git_repository(cwd)
        && let Err(e) = tenex::git::ensure_tenex_excluded(cwd)
    {
        eprintln!("Warning: Failed to exclude .tenex from git: {e}");
    }

    // keyboard_enhancement_supported will be set in tui::run after terminal setup
    let mut app = App::new(config, storage, settings, false);
    if let Some(message) = storage_load_error {
        app.set_error(message);
    }
    app.set_cwd_project_root(cwd_project_root);

    maybe_queue_whats_new(&mut app);

    if matches!(&app.mode, AppMode::Normal(_)) {
        maybe_prompt_restart_mux_daemon(&mut app);
    }

    if matches!(&app.mode, AppMode::Normal(_)) {
        match (deps.check_for_update)() {
            Ok(Some(info)) => {
                app.apply_mode(UpdatePromptMode { info }.into());
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Warning: Failed to check for updates: {e}");
            }
        }
    }

    // Auto-connect to any existing worktrees
    if let Err(e) = (deps.auto_connect_worktrees)(&mut app) {
        eprintln!("Warning: Failed to auto-connect to worktrees: {e}");
    }

    // After reboot/crash, stored agents may outlive the mux daemon. Attempt to restore missing
    // mux sessions and windows from persisted state.
    if let Err(e) = (deps.respawn_missing_agents)(&mut app) {
        eprintln!("Warning: Failed to respawn agents: {e}");
    }

    if let Some(info) = (deps.run_tui)(app)? {
        println!(
            "Updating Tenex from {} to {}...",
            info.current_version, info.latest_version
        );
        (deps.install_latest)()?;
        (deps.restart_current_process)()?;
    }

    Ok(())
}

fn maybe_queue_whats_new(app: &mut App) {
    maybe_queue_whats_new_with(
        app,
        tenex::release_notes::current_version,
        tenex::release_notes::changelog_lines_for_version,
        tenex::release_notes::whats_new_lines,
        Settings::set_last_seen_version,
    );
}

type CurrentVersion = fn() -> Result<Version>;
type ChangelogLinesForVersion = fn(&Version) -> Result<Vec<String>>;
type WhatsNewLines = fn(Option<&Version>, &Version) -> Result<Vec<String>>;
type SetLastSeenVersion = fn(&mut Settings, &Version) -> std::io::Result<()>;

fn maybe_queue_whats_new_with(
    app: &mut App,
    current_version: CurrentVersion,
    changelog_lines_for_version: ChangelogLinesForVersion,
    whats_new_lines: WhatsNewLines,
    set_last_seen_version: SetLastSeenVersion,
) {
    let Ok(current_version) = current_version() else {
        return;
    };

    let raw_last_seen = app.data.settings.last_seen_version.clone();
    let parsed_last_seen = raw_last_seen
        .as_deref()
        .and_then(|raw| Version::parse(raw).ok());

    match parsed_last_seen {
        None => {
            if raw_last_seen.is_none() {
                if let Err(e) = set_last_seen_version(&mut app.data.settings, &current_version) {
                    eprintln!("Warning: Failed to save settings: {e}");
                }
                return;
            }

            // Corrupt or non-semver value: show current release notes once, then overwrite.
            match changelog_lines_for_version(&current_version) {
                Ok(lines) => {
                    app.data.pending_changelog = Some(ChangelogMode {
                        title: "What's New".to_string(),
                        lines,
                        mark_seen_version: Some(current_version),
                    });
                }
                Err(e) => {
                    eprintln!("Warning: Failed to prepare release notes: {e}");
                }
            }
        }
        Some(last_seen) => {
            if last_seen > current_version {
                if let Err(e) = set_last_seen_version(&mut app.data.settings, &current_version) {
                    eprintln!("Warning: Failed to save settings: {e}");
                }
                return;
            }

            if last_seen == current_version {
                return;
            }

            match whats_new_lines(Some(&last_seen), &current_version) {
                Ok(lines) => {
                    app.data.pending_changelog = Some(ChangelogMode {
                        title: "What's New".to_string(),
                        lines,
                        mark_seen_version: Some(current_version),
                    });
                }
                Err(e) => {
                    eprintln!("Warning: Failed to prepare release notes: {e}");
                }
            }
        }
    }
}

fn maybe_prompt_restart_mux_daemon(app: &mut App) {
    maybe_prompt_restart_mux_daemon_with(
        app,
        tenex::mux::version,
        tenex::mux::running_daemon_version,
        tenex::mux::socket_display,
        env_mux_socket,
    );
}

type MuxVersion = fn() -> String;
type RunningDaemonVersion = fn() -> Result<Option<String>>;
type SocketDisplay = fn() -> Result<String>;
type EnvMuxSocket = fn() -> Option<String>;

fn maybe_prompt_restart_mux_daemon_with(
    app: &mut App,
    version: MuxVersion,
    running_daemon_version: RunningDaemonVersion,
    socket_display: SocketDisplay,
    env_mux_socket: EnvMuxSocket,
) {
    let expected_version = version();
    let Ok(Some(daemon_version)) = running_daemon_version() else {
        return;
    };

    maybe_prompt_restart_mux_daemon_for_versions_with(
        app,
        daemon_version,
        expected_version,
        socket_display,
        env_mux_socket,
    );
}

fn env_mux_socket() -> Option<String> {
    env_mux_socket_from(|| std::env::var("TENEX_MUX_SOCKET").ok())
}

fn env_mux_socket_from(get_env: EnvMuxSocket) -> Option<String> {
    let value = get_env()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn maybe_prompt_restart_mux_daemon_for_versions_with(
    app: &mut App,
    daemon_version: String,
    expected_version: String,
    socket_display: SocketDisplay,
    env_mux_socket: EnvMuxSocket,
) {
    if daemon_version == expected_version {
        return;
    }

    let socket = socket_display().unwrap_or_else(|_| "<unknown>".to_string());
    let env_mux_socket = env_mux_socket();

    app.data.ui.muxd_version_mismatch = Some(MuxdVersionMismatchInfo {
        socket,
        daemon_version,
        expected_version,
        env_mux_socket,
    });

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::RestartMuxDaemon,
        }
        .into(),
    );
}

fn preserve_corrupt_state_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    preserve_corrupt_state_file_with(path, std::time::SystemTime::now)
}

type SystemTimeNow = fn() -> std::time::SystemTime;

fn unix_timestamp_seconds(now: std::time::SystemTime) -> Option<u64> {
    now.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|dur| dur.as_secs())
}

fn preserve_corrupt_state_file_with(
    path: &std::path::Path,
    now: SystemTimeNow,
) -> Option<std::path::PathBuf> {
    let file_name = path.file_name()?.to_string_lossy();

    let timestamp = unix_timestamp_seconds(now())?;
    let preserved = path.with_file_name(format!("{file_name}.corrupt-{timestamp}"));

    std::fs::rename(path, &preserved).ok()?;
    Some(preserved)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn find_installed_binary(name: &str) -> std::path::PathBuf {
    use tenex::paths;

    // Try CARGO_HOME first, then ~/.cargo, then just the binary name (PATH lookup)
    let candidates = [
        std::env::var("CARGO_HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join("bin").join(name)),
        paths::home_dir().map(|h| h.join(".cargo").join("bin").join(name)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return candidate;
        }
    }

    std::path::PathBuf::from(name)
}

#[cfg(unix)]
type RestartExec = fn(std::path::PathBuf, Vec<String>) -> std::io::Error;

#[cfg(unix)]
static RESTART_EXEC_OVERRIDE: std::sync::OnceLock<std::sync::Mutex<Option<RestartExec>>> =
    std::sync::OnceLock::new();

#[cfg(all(any(test, coverage), unix))]
static INSTALLED_BINARY_OVERRIDE: std::sync::OnceLock<
    std::sync::Mutex<Option<std::path::PathBuf>>,
> = std::sync::OnceLock::new();

#[cfg(unix)]
fn restart_exec_override() -> Option<RestartExec> {
    let lock = RESTART_EXEC_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None));
    let guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard
}

#[cfg(unix)]
fn installed_binary_for_restart(name: &str) -> std::path::PathBuf {
    #[cfg(any(test, coverage))]
    {
        let lock = INSTALLED_BINARY_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None));
        let guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(path) = guard.clone() {
            return path;
        }
    }

    find_installed_binary(name)
}

#[cfg(unix)]
fn exec_restart(installed: std::path::PathBuf, args: Vec<String>) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(installed).args(args).exec()
}

#[cfg(unix)]
fn restart_current_process() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // After `cargo install --force`, spawning a new process and exiting can leave the
    // restarted Tenex in the background (job control), causing terminal I/O errors.
    // Prefer `exec` to replace the current process in-place.

    let installed = installed_binary_for_restart(env!("CARGO_PKG_NAME"));

    // `exec` replaces the current process on success; on failure it returns an io::Error.
    let exec = match restart_exec_override() {
        Some(exec) => exec,
        None => exec_restart,
    };
    let err = exec(installed, args);
    Err(anyhow::Error::new(err).context("Failed to restart Tenex"))
}

#[cfg(windows)]
#[cfg_attr(coverage_nightly, coverage(off))]
fn restart_current_process() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let installed = find_installed_binary(env!("CARGO_PKG_NAME"));

    std::process::Command::new(installed)
        .args(&args)
        .spawn()
        .context("Failed to restart Tenex")?;

    std::process::exit(0);
}

fn cmd_reset(force: bool) -> Result<()> {
    let mut storage = Storage::load().unwrap_or_default();
    let mux = SessionManager::new();
    cmd_reset_with_storage(force, &mut storage, mux, ResetDeps::production())
}

type MuxIsRunning = fn() -> bool;
type MuxListSessions = fn(SessionManager) -> Result<Vec<tenex::mux::Session>>;
type MuxKillSession = fn(SessionManager, &str) -> Result<()>;
type CleanupAgentRuntime = fn(&tenex::Agent) -> Result<()>;
type CurrentDir = fn() -> Result<std::path::PathBuf>;
type OpenRepository = fn(&std::path::Path) -> Result<tenex::git::Repository>;
type RemoveWorktree = for<'repo> fn(&tenex::git::WorktreeManager<'repo>, &str) -> Result<()>;
type DeleteBranch = for<'repo> fn(&tenex::git::BranchManager<'repo>, &str) -> Result<()>;
type PromptResetScope = fn(bool) -> Result<ResetScope>;
type ConfirmReset = fn(bool) -> Result<bool>;

fn mux_list_sessions_for_reset_deps(mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
    mux.list()
}

fn mux_kill_session_for_reset_deps(mux: SessionManager, name: &str) -> Result<()> {
    mux.kill(name)
}

fn current_dir_for_reset_deps() -> Result<std::path::PathBuf> {
    current_dir_for_reset_deps_with(std::env::current_dir)
}

type GetCurrentDir = fn() -> std::io::Result<std::path::PathBuf>;

fn current_dir_for_reset_deps_with(get_current_dir: GetCurrentDir) -> Result<std::path::PathBuf> {
    Ok(get_current_dir()?)
}

fn remove_worktree_for_reset_deps(
    manager: &tenex::git::WorktreeManager<'_>,
    name: &str,
) -> Result<()> {
    manager.remove(name)
}

fn delete_branch_for_reset_deps(manager: &tenex::git::BranchManager<'_>, name: &str) -> Result<()> {
    manager.delete(name)
}

#[derive(Clone, Copy)]
struct ResetDeps {
    mux_is_running: MuxIsRunning,
    mux_list_sessions: MuxListSessions,
    mux_kill_session: MuxKillSession,
    cleanup_agent_runtime: CleanupAgentRuntime,
    current_dir: CurrentDir,
    open_repository: OpenRepository,
    remove_worktree: RemoveWorktree,
    delete_branch: DeleteBranch,
}

impl ResetDeps {
    fn production() -> Self {
        Self {
            mux_is_running: tenex::mux::is_server_running,
            mux_list_sessions: mux_list_sessions_for_reset_deps,
            mux_kill_session: mux_kill_session_for_reset_deps,
            cleanup_agent_runtime: tenex::cleanup_agent_runtime,
            current_dir: current_dir_for_reset_deps,
            open_repository: tenex::git::open_repository,
            remove_worktree: remove_worktree_for_reset_deps,
            delete_branch: delete_branch_for_reset_deps,
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn cmd_reset_with_storage(
    force: bool,
    storage: &mut Storage,
    mux: SessionManager,
    deps: ResetDeps,
) -> Result<()> {
    cmd_reset_with_storage_with_prompts(
        force,
        storage,
        mux,
        deps,
        prompt_reset_scope,
        confirm_reset,
    )
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn cmd_reset_with_storage_with_prompts(
    force: bool,
    storage: &mut Storage,
    mux: SessionManager,
    deps: ResetDeps,
    prompt_reset_scope: PromptResetScope,
    confirm_reset: ConfirmReset,
) -> Result<()> {
    use std::collections::HashSet;
    use tenex::git::WorktreeManager;

    let mux_running = (deps.mux_is_running)();

    let instance_prefix = storage.instance_session_prefix();
    let scope = prompt_reset_scope(force)?;

    // Find orphaned Tenex mux sessions (not in storage)
    let storage_sessions: HashSet<_> = storage
        .iter()
        .map(|agent| agent.mux_session.clone())
        .collect();
    let orphaned_sessions = list_orphaned_sessions_with(
        mux,
        mux_running,
        scope,
        &instance_prefix,
        &storage_sessions,
        deps.mux_list_sessions,
    );

    if storage.is_empty() && orphaned_sessions.is_empty() {
        if storage.mux_socket.take().is_some() {
            storage.save()?;
        }
        println!("No agents to reset.");
        return Ok(());
    }

    print_reset_plan(storage, &orphaned_sessions);

    if !confirm_reset(force)? {
        println!("Aborted.");
        return Ok(());
    }

    // Kill mux sessions and remove worktrees/branches
    let repo_path = (deps.current_dir)()?;
    let repo = (deps.open_repository)(&repo_path).ok();
    let worktree_mgr = repo.as_ref().map(WorktreeManager::new);
    let branch_mgr = repo.as_ref().map(tenex::git::BranchManager::new);

    if !mux_running {
        eprintln!("Warning: Mux daemon is not running; skipping session termination.");
    }

    for agent in storage.iter() {
        if mux_running && let Err(e) = (deps.mux_kill_session)(mux, &agent.mux_session) {
            eprintln!(
                "Warning: Failed to kill mux session {}: {e}",
                agent.mux_session
            );
        }
        if let Err(e) = (deps.cleanup_agent_runtime)(agent) {
            eprintln!(
                "Warning: Failed to clean up runtime for {} ({}): {e}",
                agent.title, agent.mux_session
            );
        }
        if let Some(ref mgr) = worktree_mgr
            && let Err(e) = (deps.remove_worktree)(mgr, &agent.branch)
        {
            eprintln!("Warning: Failed to remove worktree {}: {e}", agent.branch);
        }
        // Also try to delete branch directly in case worktree was already gone
        if let Some(ref mgr) = branch_mgr
            && let Err(e) = (deps.delete_branch)(mgr, &agent.branch)
        {
            eprintln!("Warning: Failed to delete branch {}: {e}", agent.branch);
        }
    }

    // Kill orphaned sessions
    if mux_running {
        for session in &orphaned_sessions {
            if let Err(e) = (deps.mux_kill_session)(mux, session) {
                eprintln!("Warning: Failed to kill orphaned mux session {session}: {e}");
            }
        }
    }

    // Clear storage
    storage.clear();
    storage.save()?;

    println!("Reset complete.");
    Ok(())
}

type FlushStdout = fn() -> std::io::Result<()>;
type ReadLineStdin = fn(&mut String) -> std::io::Result<usize>;

fn flush_stdout() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout().flush()
}

fn read_line_stdin(input: &mut String) -> std::io::Result<usize> {
    std::io::stdin().read_line(input)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn prompt_reset_scope(force: bool) -> Result<ResetScope> {
    if force {
        return Ok(ResetScope::ThisInstance);
    }

    println!("Reset scope:");
    println!(
        "  1) This Tenex instance only ({})",
        Config::state_path().display()
    );
    println!("  2) All Tenex sessions on this machine");
    print!("Select [1/2] (default 1): ");

    let mut input = String::new();
    prompt_reset_scope_with_io(force, flush_stdout, read_line_stdin, &mut input)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn prompt_reset_scope_with_io(
    force: bool,
    flush_stdout: FlushStdout,
    read_line_stdin: ReadLineStdin,
    input: &mut String,
) -> Result<ResetScope> {
    if force {
        return Ok(ResetScope::ThisInstance);
    }

    flush_stdout()?;
    read_line_stdin(input)?;

    let trimmed = input.trim();
    if trimmed == "2" || trimmed.eq_ignore_ascii_case("all") {
        return Ok(ResetScope::AllInstances);
    }

    Ok(ResetScope::ThisInstance)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn list_orphaned_sessions_with(
    mux: SessionManager,
    mux_running: bool,
    scope: ResetScope,
    instance_prefix: &str,
    storage_sessions: &std::collections::HashSet<String>,
    list_sessions: MuxListSessions,
) -> Vec<String> {
    if !mux_running {
        return Vec::new();
    }

    let sessions = {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(list_sessions(mux));
        });

        match rx.recv_timeout(Duration::from_millis(750)) {
            Ok(Ok(sessions)) => sessions,
            _ => return Vec::new(),
        }
    };

    let prefix = match scope {
        ResetScope::ThisInstance => instance_prefix,
        ResetScope::AllInstances => "tenex-",
    };

    sessions
        .into_iter()
        .filter(|s| s.name.starts_with(prefix))
        .filter(|s| !storage_sessions.contains(&s.name))
        .map(|s| s.name)
        .collect()
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn print_reset_plan(storage: &Storage, orphaned_sessions: &[String]) {
    if !storage.is_empty() {
        println!("Agents to kill:\n");
        for agent in storage.iter() {
            println!(
                "  - {} ({}) [{}]",
                agent.title,
                agent.short_id(),
                agent.branch
            );
        }
        println!();
    }

    if !orphaned_sessions.is_empty() {
        println!("Orphaned mux sessions to kill:\n");
        for session in orphaned_sessions {
            println!("  - {session}");
        }
        println!();
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn confirm_reset(force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }

    print!("Continue? [y/N] ");

    let mut input = String::new();
    confirm_reset_with_io(force, flush_stdout, read_line_stdin, &mut input)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn confirm_reset_with_io(
    force: bool,
    flush_stdout: FlushStdout,
    read_line_stdin: ReadLineStdin,
    input: &mut String,
) -> Result<bool> {
    if force {
        return Ok(true);
    }

    flush_stdout()?;
    read_line_stdin(input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unnecessary_wraps,
        reason = "Test helpers use Result-returning hooks to match injected signatures."
    )]
    #![expect(clippy::unwrap_used, reason = "Unit tests use unwrap for assertions.")]
    #![expect(clippy::expect_used, reason = "Unit tests use expect for assertions.")]

    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    static RUN_INTERACTIVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    static RESET_TEST_CURRENT_DIR: OnceLock<Mutex<Option<std::path::PathBuf>>> = OnceLock::new();
    static INSTALL_LATEST_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESTART_PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RUN_MUXD_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CMD_RESET_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CMD_RESET_FORCE: AtomicUsize = AtomicUsize::new(0);
    static MIGRATE_CALLS: AtomicUsize = AtomicUsize::new(0);

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
        Err(anyhow::anyhow!("boom"))
    }

    fn auto_connect_err(_app: &mut App) -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn respawn_err(_app: &mut App) -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn run_tui_err(app: App) -> Result<Option<tenex::update::UpdateInfo>> {
        let App { mode, .. } = app;
        let _ = mode;
        Err(anyhow::anyhow!("boom"))
    }

    fn flush_stdout_ok() -> std::io::Result<()> {
        Ok(())
    }

    fn flush_stdout_err() -> std::io::Result<()> {
        Err(std::io::Error::other("boom"))
    }

    fn read_line_err(_input: &mut String) -> std::io::Result<usize> {
        Err(std::io::Error::other("boom"))
    }

    fn running_daemon_none() -> Result<Option<String>> {
        Ok(None)
    }

    fn running_daemon_err() -> Result<Option<String>> {
        Err(anyhow::anyhow!("boom"))
    }

    fn socket_display_ok() -> Result<String> {
        Ok("socket".to_string())
    }

    fn env_mux_socket_none() -> Option<String> {
        None
    }

    fn fake_check_for_update_none() -> Result<Option<tenex::update::UpdateInfo>> {
        Ok(None)
    }

    fn fake_check_for_update_available() -> Result<Option<tenex::update::UpdateInfo>> {
        Ok(Some(tenex::update::UpdateInfo {
            current_version: Version::parse("1.0.0").unwrap(),
            latest_version: Version::parse("2.0.0").unwrap(),
        }))
    }

    fn fake_check_for_update_error() -> Result<Option<tenex::update::UpdateInfo>> {
        Err(anyhow::anyhow!("boom"))
    }

    fn fake_run_tui_returns_update(app: App) -> Result<Option<tenex::update::UpdateInfo>> {
        let App { mode, .. } = app;
        let _ = mode;
        Ok(Some(tenex::update::UpdateInfo {
            current_version: Version::parse("1.0.0").unwrap(),
            latest_version: Version::parse("2.0.0").unwrap(),
        }))
    }

    fn fake_run_tui_none(app: App) -> Result<Option<tenex::update::UpdateInfo>> {
        let App { mode, .. } = app;
        let _ = mode;
        Ok(None)
    }

    fn fake_install_latest() -> Result<()> {
        INSTALL_LATEST_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn fake_install_latest_err() -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn fake_restart_process() -> Result<()> {
        RESTART_PROCESS_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn fake_restart_process_err() -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn fake_run_mux_daemon() -> Result<()> {
        RUN_MUXD_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn fake_migrate_ok_counted() -> Result<()> {
        MIGRATE_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn fake_migrate_err() -> Result<()> {
        Err(anyhow::anyhow!("boom"))
    }

    fn fake_cmd_reset(force: bool) -> Result<()> {
        CMD_RESET_CALLS.fetch_add(1, Ordering::SeqCst);
        CMD_RESET_FORCE.store(usize::from(force), Ordering::SeqCst);
        Ok(())
    }

    fn fake_load_settings() -> Settings {
        Settings::default()
    }

    fn fake_auto_connect_worktrees(app: &mut App) -> Result<()> {
        let _ = app;
        Ok(())
    }

    fn fake_respawn_missing_agents(app: &mut App) -> Result<()> {
        let _ = app;
        Ok(())
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
        Ok(())
    }

    fn set_last_seen_err(settings: &mut Settings, version: &Version) -> std::io::Result<()> {
        settings.last_seen_version = Some(version.to_string());
        Err(std::io::Error::other("boom"))
    }

    fn changelog_lines_empty(_version: &Version) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn whats_new_lines_empty(_from: Option<&Version>, _to: &Version) -> Result<Vec<String>> {
        Ok(Vec::new())
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
        let command = cli.command.expect("Expected muxd command");
        assert_eq!(
            std::mem::discriminant(&command),
            std::mem::discriminant(&Commands::Muxd)
        );
    }

    #[test]
    fn test_run_cli_muxd_calls_run_mux_daemon() -> Result<()> {
        RUN_MUXD_CALLS.store(0, Ordering::SeqCst);

        let cli = Cli::parse_from(["tenex", "muxd"]);
        let dir = TempDir::new().unwrap();
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

        run_cli(&cli, config, &state_path, None, &deps).unwrap();
        assert_eq!(RUN_MUXD_CALLS.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn test_run_cli_reset_warns_when_migration_fails() -> Result<()> {
        CMD_RESET_CALLS.store(0, Ordering::SeqCst);
        CMD_RESET_FORCE.store(0, Ordering::SeqCst);

        let cli = Cli::parse_from(["tenex", "reset", "--force"]);
        let dir = TempDir::new().unwrap();
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

        run_cli(&cli, config, &state_path, None, &deps).unwrap();
        assert_eq!(CMD_RESET_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(CMD_RESET_FORCE.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn test_run_cli_reset_calls_migration_when_ok() -> Result<()> {
        MIGRATE_CALLS.store(0, Ordering::SeqCst);
        CMD_RESET_CALLS.store(0, Ordering::SeqCst);

        let cli = Cli::parse_from(["tenex", "reset", "--force"]);
        let dir = TempDir::new().unwrap();
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

        run_cli(&cli, config, &state_path, None, &deps).unwrap();
        assert_eq!(MIGRATE_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(CMD_RESET_CALLS.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn test_run_cli_default_runs_cmd_default() -> Result<()> {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        std::env::set_current_dir(cwd).unwrap();
        result
    }

    #[test]
    fn test_run_cli_default_calls_migration_when_ok() -> Result<()> {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        MIGRATE_CALLS.store(0, Ordering::SeqCst);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        std::env::set_current_dir(cwd).unwrap();
        result.unwrap();

        assert_eq!(MIGRATE_CALLS.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn test_cmd_default_saves_after_backfill() -> Result<()> {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        storage.save().unwrap();

        let deps = InteractiveDeps {
            check_for_update: fake_check_for_update_none,
            run_tui: fake_run_tui_none,
            install_latest: fake_install_latest,
            restart_current_process: fake_restart_process,
            auto_connect_worktrees: fake_auto_connect_worktrees,
            respawn_missing_agents: fake_respawn_missing_agents,
        };

        let result = cmd_default(config, &state_path, None, Settings::default(), &deps);
        std::env::set_current_dir(cwd).unwrap();
        result.unwrap();

        let updated = Storage::load_from(&state_path).unwrap();
        let agent = updated.get_by_index(0).expect("Missing agent after reload");
        assert_eq!(agent.id, agent_id);
        let expected = agent_id.to_string();
        assert_eq!(agent.conversation_id.as_deref(), Some(expected.as_str()));
        Ok(())
    }

    #[test]
    fn test_cmd_default_errors_when_worktree_dir_is_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let worktree_dir = dir.path().join("worktrees");

        std::fs::write(&worktree_dir, b"not a dir").unwrap();

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

        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();

        let state_path = state_dir.join("state.json");
        let worktree_dir = dir.path().join("worktrees");
        std::fs::create_dir_all(&worktree_dir).unwrap();

        let mut storage = Storage::with_path(state_path.clone());
        storage.instance_id = Some("deadbeef".to_string());
        let agent = tenex::Agent::new(
            "Agent".to_string(),
            "claude".to_string(),
            "tenex/test-branch".to_string(),
            dir.path().join("wt"),
        );
        storage.add(agent);
        storage.save().unwrap();

        let original_perms = std::fs::metadata(&state_dir).unwrap().permissions();
        let mut perms = original_perms.clone();
        perms.set_mode(0o555);
        std::fs::set_permissions(&state_dir, perms).unwrap();

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

        std::fs::set_permissions(&state_dir, original_perms).unwrap();

        assert!(err.contains("Failed"));
        Ok(())
    }

    #[cfg(unix)]
    fn fake_exec_restart(_installed: std::path::PathBuf, _args: Vec<String>) -> std::io::Error {
        std::io::Error::other("boom")
    }

    #[cfg(unix)]
    #[test]
    fn test_restart_current_process_can_use_exec_override() {
        let _ = restart_exec_override();
        let lock = RESTART_EXEC_OVERRIDE.get().unwrap();
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
    }

    // Note: test_cmd_reset_force moved to tests/cli_binary_test.rs
    // to properly isolate state via subprocess + TENEX_STATE_PATH env var.
    // Running cmd_reset directly in a unit test would corrupt real state.

    #[test]
    fn test_init_logging_covers_warning_and_debug_branches()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new().unwrap();
        let tmp_file = dir.path().join("not-a-directory");
        std::fs::write(&tmp_file, "x").unwrap();
        let bad_log_path = tmp_file.join("tenex.log");

        init_logging_with(&bad_log_path, Some("not-a-number"));
        init_logging_with(&dir.path().join("tenex.log"), Some("1"));
        init_logging_with(&dir.path().join("tenex.log"), Some("2"));
        init_logging_with(&dir.path().join("tenex.log"), Some("3"));

        Ok(())
    }

    #[test]
    fn test_init_logging_handles_log_path_without_parent() -> Result<(), Box<dyn std::error::Error>>
    {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let original_cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        init_logging_with(std::path::Path::new("/"), Some("1"));

        std::env::set_current_dir(original_cwd).unwrap();
        Ok(())
    }

    #[test]
    fn test_load_storage_with_error_returns_empty_when_state_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new().unwrap();
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
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");

        let (storage, err) =
            load_storage_result_with_error(&state_path, Err(anyhow::anyhow!("boom")));
        assert!(storage.is_empty());

        let err = err.expect("Expected load_storage_with_error to return message");
        assert!(err.contains("Failed to load state file"));
        assert!(!err.contains("Preserved unreadable state"));
        assert!(!err.contains("Failed to preserve unreadable state file"));

        Ok(())
    }

    #[test]
    fn test_load_storage_with_error_preserves_corrupt_state_and_backup()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let backup_path = dir.path().join("state.json.bak");
        std::fs::write(&state_path, "{").unwrap();
        std::fs::write(&backup_path, "{").unwrap();

        let (storage, err) =
            load_storage_result_with_error(&state_path, Storage::load_from(&state_path));
        assert!(storage.is_empty());
        let err = err.expect("Expected load_storage_with_error to return message");
        assert!(err.contains("Failed to load state file"));
        assert!(err.contains("Preserved unreadable state"));
        assert!(!state_path.exists());
        assert!(!backup_path.exists());

        let mut names = Vec::new();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
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
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();

        let worktree_dir = dir.path().join("not-a-dir");
        std::fs::write(&worktree_dir, "x").unwrap();

        let config = Config {
            worktree_dir,
            ..Config::default()
        };

        let mut storage = Storage::with_path(state_path.clone());
        let err = ensure_instance_initialized(&config, &mut storage, &state_path, None)
            .expect_err("Expected ensure_instance_initialized to return Err");
        assert!(
            err.to_string()
                .contains("Failed to create worktrees directory")
        );
        Ok(())
    }

    #[test]
    fn test_ensure_instance_initialized_sets_mux_socket_when_agents_exist()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();
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

        ensure_instance_initialized(&config, &mut storage, &state_path, None).unwrap();
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
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();
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

        ensure_instance_initialized(&config, &mut storage, &state_path, None).unwrap();
        assert_eq!(storage.mux_socket.as_deref(), Some("tenex-missing-socket"));
        assert!(state_path.exists());
        Ok(())
    }

    #[test]
    fn test_preserve_corrupt_state_file_renames_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "boom").unwrap();

        let preserved = preserve_corrupt_state_file(&path)
            .expect("Expected preserve_corrupt_state_file to rename file");

        assert!(!path.exists());
        assert!(preserved.exists());
        assert_eq!(std::fs::read_to_string(&preserved).unwrap(), "boom");
    }

    #[test]
    fn test_preserve_corrupt_state_file_returns_none_for_root_path() {
        assert!(preserve_corrupt_state_file(std::path::Path::new("/")).is_none());
    }

    #[test]
    fn test_preserve_corrupt_state_file_returns_none_when_unix_timestamp_unavailable() {
        fn now_before_unix_epoch() -> std::time::SystemTime {
            std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)
        }

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "boom").unwrap();

        assert!(preserve_corrupt_state_file_with(&path, now_before_unix_epoch).is_none());
        assert!(path.exists());
    }

    #[test]
    fn test_preserve_corrupt_state_file_returns_none_when_rename_fails() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.json");
        assert!(preserve_corrupt_state_file(&path).is_none());
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
    fn test_ensure_instance_initialized_clears_mux_socket_when_no_agents() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");

        std::fs::write(&state_path, "{}").unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };

        let mut storage = Storage::with_path(state_path.clone());
        storage.instance_id = Some("deadbeef".to_string());
        storage.mux_socket = Some("tenex-mux-stale.sock".to_string());

        ensure_instance_initialized(&config, &mut storage, &state_path, None).unwrap();

        assert!(storage.is_empty());
        assert!(storage.mux_socket.is_none());

        let loaded = Storage::load_from(&state_path).unwrap();
        assert!(loaded.is_empty());
        assert!(loaded.mux_socket.is_none());
    }

    #[test]
    fn test_ensure_instance_initialized_does_not_write_state_when_empty_and_unchanged() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };

        let mut storage = Storage::with_path(state_path.clone());
        storage.instance_id = Some("deadbeef".to_string());
        storage.mux_socket = None;

        ensure_instance_initialized(&config, &mut storage, &state_path, None).unwrap();

        assert_eq!(storage.instance_id.as_deref(), Some("deadbeef"));
        assert_eq!(std::fs::read_to_string(&state_path).unwrap(), "{}");
    }

    #[test]
    fn test_ensure_instance_initialized_skips_mux_socket_updates_when_env_mux_socket_provided() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();

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
        )
        .unwrap();

        assert_eq!(storage.instance_id.as_deref(), Some("deadbeef"));
        assert_eq!(std::fs::read_to_string(&state_path).unwrap(), "{}");
    }

    #[test]
    fn test_ensure_instance_initialized_keeps_mux_socket_none_when_socket_display_errors() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();

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

        ensure_instance_initialized_with(
            &config,
            &mut storage,
            &state_path,
            None,
            socket_display_err,
        )
        .unwrap();

        assert!(storage.mux_socket.is_none());
        assert_eq!(std::fs::read_to_string(&state_path).unwrap(), "{}");
    }

    #[test]
    fn test_ensure_instance_initialized_saves_state_when_state_file_missing_in_final_save_path() {
        let dir = TempDir::new().unwrap();
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
        )
        .unwrap();

        assert!(state_path.exists());
        let persisted = std::fs::read_to_string(&state_path).unwrap();
        assert!(persisted.contains("instance_id"));
    }

    #[test]
    fn test_ensure_instance_initialized_saves_state_when_instance_id_changes_in_final_save_path() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();

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
        )
        .unwrap();

        let persisted = std::fs::read_to_string(&state_path).unwrap();
        assert!(persisted.contains("instance_id"));
    }

    #[test]
    fn test_ensure_instance_initialized_saves_state_when_instance_id_was_missing() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::write(&state_path, "{}").unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };

        let mut storage = Storage::with_path(state_path.clone());
        assert!(storage.instance_id.is_none());

        ensure_instance_initialized(&config, &mut storage, &state_path, None).unwrap();

        let persisted = std::fs::read_to_string(&state_path).unwrap();
        assert!(persisted.contains("instance_id"));
    }

    #[test]
    fn test_ensure_instance_initialized_errors_when_save_to_fails_in_early_return_path() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::create_dir_all(&state_path).unwrap();

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
    }

    #[test]
    fn test_ensure_instance_initialized_errors_when_save_to_fails_in_final_save_path() {
        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::create_dir_all(&state_path).unwrap();

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
    }

    #[test]
    fn test_run_interactive_installs_and_restarts_after_update_prompt() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
        RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        std::env::set_current_dir(cwd).unwrap();

        result.unwrap();
        assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_run_interactive_continues_when_update_check_fails() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
        RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        std::env::set_current_dir(cwd).unwrap();

        result.unwrap();
        assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_run_interactive_installs_and_restarts_when_tui_requests_update() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
        RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        std::env::set_current_dir(cwd).unwrap();

        result.unwrap();
        assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_run_interactive_propagates_install_latest_errors_when_tui_requests_update() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
        RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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

        std::env::set_current_dir(cwd).unwrap();
        assert!(err.contains("boom"));
        assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 0);
        assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_run_interactive_propagates_restart_process_errors_when_tui_requests_update() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        INSTALL_LATEST_CALLS.store(0, Ordering::SeqCst);
        RESTART_PROCESS_CALLS.store(0, Ordering::SeqCst);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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

        std::env::set_current_dir(cwd).unwrap();
        assert!(err.contains("boom"));
        assert_eq!(INSTALL_LATEST_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(RESTART_PROCESS_CALLS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_run_interactive_passes_storage_load_error_through_to_app() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

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
        std::env::set_current_dir(cwd).unwrap();

        result.unwrap();
    }

    #[test]
    fn test_run_interactive_warns_when_auto_connect_fails() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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
        std::env::set_current_dir(cwd).unwrap();
        result.unwrap();
    }

    #[test]
    fn test_run_interactive_warns_when_respawn_fails() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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
        std::env::set_current_dir(cwd).unwrap();
        result.unwrap();
    }

    #[test]
    fn test_run_interactive_warns_when_excluding_tenex_from_git_fails() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let original_cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        let repo = tenex::git::Repository::init(dir.path()).unwrap();

        let info_dir = repo.path().join("info");
        std::fs::remove_dir_all(&info_dir).unwrap();
        std::fs::write(&info_dir, "not-a-dir").unwrap();

        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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
        std::env::set_current_dir(original_cwd).unwrap();
        result.unwrap();
    }

    #[test]
    fn test_run_interactive_does_not_warn_when_excluding_tenex_from_git_succeeds() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let original_cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        let _repo = tenex::git::Repository::init(dir.path()).unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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
        std::env::set_current_dir(original_cwd).unwrap();
        result.unwrap();
    }

    #[test]
    fn test_run_interactive_handles_current_dir_failure() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let original_cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        let gone = dir.path().join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        std::env::set_current_dir(&gone).unwrap();
        std::fs::remove_dir_all(&gone).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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
        std::env::set_current_dir(original_cwd).unwrap();
        result.unwrap();
    }

    #[test]
    fn test_run_interactive_propagates_run_tui_errors() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings {
            last_seen_version: Some(tenex::release_notes::current_version().unwrap().to_string()),
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
        std::env::set_current_dir(cwd).unwrap();
        assert!(err.contains("boom"));
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
    fn test_prompt_reset_scope_with_io_returns_this_instance_when_force_true() {
        let mut input = String::new();
        let scope =
            prompt_reset_scope_with_io(true, flush_stdout_err, read_line_err, &mut input).unwrap();
        assert_eq!(scope, ResetScope::ThisInstance);
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
    fn test_prompt_reset_scope_with_io_accepts_all_instances_numeric() {
        fn read_line_two(input: &mut String) -> std::io::Result<usize> {
            input.clear();
            input.push_str("2\n");
            Ok(2)
        }

        let mut input = String::new();
        let scope =
            prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_two, &mut input).unwrap();
        assert_eq!(scope, ResetScope::AllInstances);
    }

    #[test]
    fn test_prompt_reset_scope_with_io_accepts_all_instances_text() {
        fn read_line_all(input: &mut String) -> std::io::Result<usize> {
            input.clear();
            input.push_str("all\n");
            Ok(4)
        }

        let mut input = String::new();
        let scope =
            prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_all, &mut input).unwrap();
        assert_eq!(scope, ResetScope::AllInstances);
    }

    #[test]
    fn test_prompt_reset_scope_with_io_defaults_to_this_instance_for_blank_input() {
        fn read_line_blank(input: &mut String) -> std::io::Result<usize> {
            input.clear();
            input.push('\n');
            Ok(1)
        }

        let mut input = String::new();
        let scope = prompt_reset_scope_with_io(false, flush_stdout_ok, read_line_blank, &mut input)
            .unwrap();
        assert_eq!(scope, ResetScope::ThisInstance);
    }

    #[test]
    fn test_confirm_reset_with_io_returns_true_when_force_true() {
        let mut input = String::new();
        let result =
            confirm_reset_with_io(true, flush_stdout_err, read_line_err, &mut input).unwrap();
        assert!(result);
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
    fn test_confirm_reset_with_io_returns_true_for_yes() {
        fn read_line_yes(input: &mut String) -> std::io::Result<usize> {
            input.clear();
            input.push_str("y\n");
            Ok(2)
        }

        let mut input = String::new();
        let result =
            confirm_reset_with_io(false, flush_stdout_ok, read_line_yes, &mut input).unwrap();
        assert!(result);
    }

    #[test]
    fn test_confirm_reset_with_io_returns_false_for_other_input() {
        fn read_line_no(input: &mut String) -> std::io::Result<usize> {
            input.clear();
            input.push_str("n\n");
            Ok(2)
        }

        let mut input = String::new();
        let result =
            confirm_reset_with_io(false, flush_stdout_ok, read_line_no, &mut input).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_cmd_reset_with_storage_prints_warnings_for_failed_cleanup_steps() {
        fn mux_is_running_true() -> bool {
            true
        }

        fn mux_list_sessions_stub(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
            let prefix = "tenex-deadbeef-";
            Ok(vec![
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

        fn remove_worktree_err(
            _mgr: &tenex::git::WorktreeManager<'_>,
            _branch: &str,
        ) -> Result<()> {
            Err(anyhow::anyhow!("boom"))
        }

        fn delete_branch_err(_mgr: &tenex::git::BranchManager<'_>, _branch: &str) -> Result<()> {
            Err(anyhow::anyhow!("boom"))
        }

        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        let _repo = tenex::git::Repository::init(&repo_dir).unwrap();

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
        cmd_reset_with_storage(true, &mut storage, mux, deps).unwrap();
    }

    #[test]
    fn test_cmd_reset_with_storage_covers_successful_cleanup_branches() {
        fn mux_is_running_true() -> bool {
            true
        }

        fn mux_list_sessions_empty(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
            Ok(Vec::new())
        }

        fn mux_kill_session_ok(_mux: SessionManager, _name: &str) -> Result<()> {
            Ok(())
        }

        fn cleanup_agent_runtime_ok(_agent: &tenex::Agent) -> Result<()> {
            Ok(())
        }

        fn remove_worktree_ok(_mgr: &tenex::git::WorktreeManager<'_>, _branch: &str) -> Result<()> {
            Ok(())
        }

        fn delete_branch_ok(_mgr: &tenex::git::BranchManager<'_>, _branch: &str) -> Result<()> {
            Ok(())
        }

        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        let _repo = tenex::git::Repository::init(&repo_dir).unwrap();

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
        cmd_reset_with_storage(true, &mut storage, mux, deps).unwrap();
        assert!(storage.is_empty());
    }

    #[test]
    fn test_cmd_reset_with_storage_noops_when_empty_and_no_orphans_and_no_mux_socket() {
        fn mux_is_running_false() -> bool {
            false
        }

        let dir = TempDir::new().unwrap();
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
        cmd_reset_with_storage(true, &mut storage, mux, deps).unwrap();

        assert!(storage.is_empty());
        assert!(storage.mux_socket.is_none());
    }

    #[test]
    fn test_cmd_reset_with_storage_kills_orphaned_sessions_when_storage_empty() {
        fn mux_is_running_true() -> bool {
            true
        }

        fn mux_list_sessions_stub(_mux: SessionManager) -> Result<Vec<tenex::mux::Session>> {
            Ok(vec![tenex::mux::Session {
                name: "tenex-deadbeef-orphan".to_string(),
                created: 0,
                attached: false,
            }])
        }

        fn mux_kill_session_ok(_mux: SessionManager, _name: &str) -> Result<()> {
            Ok(())
        }

        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();
        let _repo = tenex::git::Repository::init(&repo_dir).unwrap();

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
        cmd_reset_with_storage(true, &mut storage, mux, deps).unwrap();
    }

    #[test]
    fn test_cmd_reset_with_storage_skips_session_kills_when_mux_not_running() {
        fn mux_is_running_false() -> bool {
            false
        }

        fn cleanup_agent_runtime_ok(_agent: &tenex::Agent) -> Result<()> {
            Ok(())
        }

        fn open_repository_err(_path: &std::path::Path) -> Result<tenex::git::Repository> {
            Err(anyhow::anyhow!("boom"))
        }

        let dir = TempDir::new().unwrap();
        let repo_dir = dir.path().join("repo");
        std::fs::create_dir_all(&repo_dir).unwrap();

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
        cmd_reset_with_storage(true, &mut storage, mux, deps).unwrap();
        assert!(storage.is_empty());
    }

    #[test]
    fn test_cmd_reset_with_storage_with_prompts_propagates_prompt_reset_scope_errors() {
        fn mux_is_running_false() -> bool {
            false
        }

        fn prompt_reset_scope_err(_force: bool) -> Result<ResetScope> {
            Err(anyhow::anyhow!("boom"))
        }

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let mut storage = Storage::with_path(state_path);
        storage.instance_id = Some("deadbeef".to_string());

        let deps = ResetDeps {
            mux_is_running: mux_is_running_false,
            ..ResetDeps::production()
        };

        let mux = SessionManager::new();
        let err = cmd_reset_with_storage_with_prompts(
            false,
            &mut storage,
            mux,
            deps,
            prompt_reset_scope_err,
            confirm_reset,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("boom"));
    }

    #[test]
    fn test_cmd_reset_with_storage_with_prompts_propagates_confirm_reset_errors() {
        fn mux_is_running_false() -> bool {
            false
        }

        fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
            Ok(ResetScope::ThisInstance)
        }

        fn confirm_reset_err(_force: bool) -> Result<bool> {
            Err(anyhow::anyhow!("boom"))
        }

        let dir = TempDir::new().unwrap();
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
        let err = cmd_reset_with_storage_with_prompts(
            false,
            &mut storage,
            mux,
            deps,
            prompt_reset_scope_ok,
            confirm_reset_err,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("boom"));
    }

    #[test]
    fn test_cmd_reset_with_storage_with_prompts_propagates_current_dir_errors() {
        fn mux_is_running_false() -> bool {
            false
        }

        fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
            Ok(ResetScope::ThisInstance)
        }

        fn confirm_reset_ok(_force: bool) -> Result<bool> {
            Ok(true)
        }

        fn current_dir_err() -> Result<std::path::PathBuf> {
            Err(anyhow::anyhow!("boom"))
        }

        let dir = TempDir::new().unwrap();
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
        let err = cmd_reset_with_storage_with_prompts(
            false,
            &mut storage,
            mux,
            deps,
            prompt_reset_scope_ok,
            confirm_reset_ok,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("boom"));
    }

    #[test]
    fn test_cmd_reset_with_storage_with_prompts_noops_can_fail_when_storage_save_fails() {
        fn mux_is_running_false() -> bool {
            false
        }

        fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
            Ok(ResetScope::ThisInstance)
        }

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::create_dir_all(&state_path).unwrap();

        let mut storage = Storage::with_path(state_path);
        storage.instance_id = Some("deadbeef".to_string());
        storage.mux_socket = Some("dummy".to_string());

        let deps = ResetDeps {
            mux_is_running: mux_is_running_false,
            ..ResetDeps::production()
        };

        let mux = SessionManager::new();
        let err = cmd_reset_with_storage_with_prompts(
            true,
            &mut storage,
            mux,
            deps,
            prompt_reset_scope_ok,
            confirm_reset,
        )
        .unwrap_err()
        .to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_cmd_reset_with_storage_with_prompts_propagates_storage_save_errors_after_clear() {
        fn mux_is_running_false() -> bool {
            false
        }

        fn prompt_reset_scope_ok(_force: bool) -> Result<ResetScope> {
            Ok(ResetScope::ThisInstance)
        }

        fn confirm_reset_ok(_force: bool) -> Result<bool> {
            Ok(true)
        }

        fn cleanup_agent_runtime_ok(_agent: &tenex::Agent) -> Result<()> {
            Ok(())
        }

        fn current_dir_ok() -> Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/tmp"))
        }

        fn open_repository_err(_path: &std::path::Path) -> Result<tenex::git::Repository> {
            Err(anyhow::anyhow!("boom"))
        }

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        std::fs::create_dir_all(&state_path).unwrap();

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
        let err = cmd_reset_with_storage_with_prompts(
            true,
            &mut storage,
            mux,
            deps,
            prompt_reset_scope_ok,
            confirm_reset_ok,
        )
        .unwrap_err()
        .to_string();
        assert!(!err.is_empty());
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
            Ok(vec![
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
    fn test_maybe_queue_whats_new_returns_when_already_seen_current() {
        let current = tenex::release_notes::current_version().unwrap();
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
    }

    #[test]
    fn test_maybe_queue_whats_new_with_uses_changelog_lines_for_corrupt_last_seen() {
        fn current_version_ok() -> Result<Version> {
            Ok(Version::parse("1.0.0").unwrap())
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
            Ok(Version::parse("1.0.0").unwrap())
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
            Ok(Version::parse("1.0.0").unwrap())
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
    fn test_maybe_queue_whats_new_warns_when_release_notes_generation_fails_for_corrupt_last_seen()
    {
        fn current_version_ok() -> Result<Version> {
            Ok(Version::parse("1.0.0").unwrap())
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
            Ok(Version::parse("1.0.0").unwrap())
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
            Ok(Version::parse("1.0.0").unwrap())
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
            Ok(Version::parse("1.0.0").unwrap())
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
    fn test_default_action_helpers_noop_on_empty_app() {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let cwd = std::env::current_dir().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let config = Config {
            worktree_dir: dir.path().join("worktrees"),
            ..Config::default()
        };
        let storage = Storage::with_path(dir.path().join("state.json"));
        let settings = Settings::default();
        let mut app = App::new(config, storage, settings, false);

        let auto_connect_result = default_auto_connect_worktrees(&mut app);
        let respawn_result = default_respawn_missing_agents(&mut app);

        std::env::set_current_dir(cwd).unwrap();

        auto_connect_result.unwrap();
        respawn_result.unwrap();
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
    fn test_maybe_prompt_restart_mux_daemon_uses_unknown_socket_when_socket_display_fails() {
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

        let mismatch = app
            .data
            .ui
            .muxd_version_mismatch
            .expect("expected muxd mismatch info");
        assert_eq!(mismatch.socket, "<unknown>");
    }

    #[cfg(unix)]
    #[test]
    fn test_exec_restart_reports_errors_without_execing() {
        let err = exec_restart(std::path::PathBuf::from("/nonexistent/tenex"), Vec::new());
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn test_restart_current_process_can_fall_back_to_exec_restart_without_execing()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = run_interactive_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let _ = restart_exec_override();
        let lock = RESTART_EXEC_OVERRIDE.get().unwrap();
        {
            let mut guard = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = None;
        }

        let dir = TempDir::new().unwrap();
        let cargo_home = dir.path().join("cargo-home");
        let bin_dir = cargo_home.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        let installed = bin_dir.join(env!("CARGO_PKG_NAME"));
        std::fs::write(&installed, "not-a-binary").unwrap();

        let _ = installed_binary_for_restart(env!("CARGO_PKG_NAME"));
        let installed_override = INSTALLED_BINARY_OVERRIDE.get().unwrap();
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
}
