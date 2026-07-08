//! CLI support for the `tenex` binary.
//!
//! This module contains the parser and command-flow glue used by the
//! production binary. It is public so the binary and Tenex's own integration
//! tests can exercise the same code, but it is not a stable automation API and
//! carries no backwards-compatibility promise.

use crate::App;
use crate::AppMode;
use crate::agent::Storage;
use crate::app::{MuxdVersionMismatchInfo, Settings};
use crate::config::Config;
use crate::mux::SessionManager;
use crate::state::{ChangelogMode, ConfirmAction, ConfirmingMode, UpdatePromptMode};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use semver::Version;

/// Terminal multiplexer for AI coding agents
#[derive(Debug, Clone, Copy, Parser)]
#[command(name = "tenex")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Optional CLI subcommand selected by the user.
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Top-level subcommands accepted by the `tenex` binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum Commands {
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

/// Reset breadth selected for the reset flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetScope {
    /// Reset only the current Tenex instance.
    ThisInstance,
    /// Reset all Tenex-owned mux sessions on this machine.
    AllInstances,
}

/// Runs the production Tenex CLI.
///
/// # Errors
///
/// Returns any error raised while initializing state, executing a command, or
/// running the interactive TUI.
pub fn run() -> Result<()> {
    init_logging();

    let cli = parse_cli();
    let config = Config::default();
    let state_path = Config::state_path();
    let env_mux_socket = env_mux_socket();

    let deps = CliDeps {
        migrate_default_state_dir: crate::migration::migrate_default_state_dir,
        run_mux_daemon: crate::mux::run_mux_daemon,
        cmd_reset,
        load_settings: Settings::load,
        interactive_deps: InteractiveDeps {
            check_for_update: crate::update::check_for_update,
            run_tui: crate::tui::run,
            install_latest: crate::update::install_latest,
            restart_current_process,
            auto_connect_worktrees: default_auto_connect_worktrees,
            respawn_missing_agents: default_respawn_missing_agents,
        },
    };

    run_cli(&cli, config, &state_path, env_mux_socket.as_deref(), &deps)
}

/// Migrates the legacy default state directory before command dispatch.
pub(crate) type MigrateDefaultStateDir = fn() -> Result<()>;
/// Starts the hidden mux daemon subcommand.
pub(crate) type RunMuxDaemon = fn() -> Result<()>;
/// Executes the reset command with the parsed force flag.
pub(crate) type CmdReset = fn(bool) -> Result<()>;
/// Loads persisted user settings for the interactive command path.
pub(crate) type LoadSettings = fn() -> Settings;

/// Checks whether a newer Tenex release is available.
pub(crate) type UpdateCheck = fn() -> Result<Option<crate::update::UpdateInfo>>;
/// Runs the TUI and optionally returns an update request.
pub(crate) type TuiRunner = fn(App) -> Result<Option<crate::update::UpdateInfo>>;
/// Installs the latest available Tenex release.
pub(crate) type InstallLatest = fn() -> Result<()>;
/// Restarts the current Tenex process after an update.
pub(crate) type RestartProcess = fn() -> Result<()>;
/// Auto-connects persisted worktrees before the TUI starts.
pub(crate) type AutoConnectWorktrees = fn(&mut App) -> Result<()>;
/// Restores missing mux-backed agents before the TUI starts.
pub(crate) type RespawnMissingAgents = fn(&mut App) -> Result<()>;

/// Dependencies injected into top-level CLI command dispatch.
#[derive(Debug, Clone, Copy)]
pub struct CliDeps {
    /// State-directory migration hook.
    pub migrate_default_state_dir: MigrateDefaultStateDir,
    /// Hidden mux daemon command hook.
    pub run_mux_daemon: RunMuxDaemon,
    /// Reset command hook.
    pub cmd_reset: CmdReset,
    /// Settings loader hook.
    pub load_settings: LoadSettings,
    /// Dependencies for the default interactive command path.
    pub interactive_deps: InteractiveDeps,
}

/// Dependencies injected into the default interactive command path.
#[derive(Debug, Clone, Copy)]
pub struct InteractiveDeps {
    /// Update check hook.
    pub check_for_update: UpdateCheck,
    /// TUI runner hook.
    pub run_tui: TuiRunner,
    /// Latest-release installer hook.
    pub install_latest: InstallLatest,
    /// Process restart hook.
    pub restart_current_process: RestartProcess,
    /// Worktree auto-connection hook.
    pub auto_connect_worktrees: AutoConnectWorktrees,
    /// Missing-agent respawn hook.
    pub respawn_missing_agents: RespawnMissingAgents,
}

/// Dispatches a parsed CLI command using injected production or test hooks.
///
/// # Errors
///
/// Returns any error raised by the selected command path.
pub fn run_cli(
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

/// Runs the default interactive CLI path with explicit settings and hooks.
///
/// # Errors
///
/// Returns an error if state initialization, state persistence, update
/// installation, process restart, or the TUI runner fails.
pub fn cmd_default(
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
    let log_path = crate::paths::log_path();
    let debug = std::env::var("DEBUG").ok();
    init_logging_with(&log_path, debug.as_deref());
}

/// Initializes file logging using an explicit log path and debug setting.
pub fn init_logging_with(log_path: &std::path::Path, debug: Option<&str>) {
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

/// Converts a storage load result into usable storage plus an optional error.
#[must_use]
pub fn load_storage_result_with_error(
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

/// Ensures the current Tenex instance has stable state before the TUI starts.
///
/// # Errors
///
/// Returns an error if the worktree directory or state file cannot be created
/// or updated.
pub fn ensure_instance_initialized(
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
        crate::mux::socket_display,
    )
}

/// Ensures the current Tenex instance has stable state with an injected socket display hook.
///
/// # Errors
///
/// Returns an error if the worktree directory or state file cannot be created
/// or updated.
pub fn ensure_instance_initialized_with(
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
        let discovered = crate::mux::discover_socket_for_sessions(&wanted_sessions, preferred);
        let chosen = discovered
            .or_else(|| preferred.map(ToString::to_string))
            .or_else(|| socket_display().ok());

        if storage.mux_socket != chosen {
            storage.mux_socket = chosen;
        }

        if let Some(socket) = storage.mux_socket.as_deref() {
            let _ = crate::mux::set_socket_override(socket);
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

/// Auto-connects worktrees using the production app action implementation.
///
/// # Errors
///
/// Returns an error if the app action fails while connecting worktrees.
pub fn default_auto_connect_worktrees(app: &mut App) -> Result<()> {
    crate::app::Actions::new().auto_connect_worktrees(app)
}

/// Respawns missing agents using the production app action implementation.
///
/// # Errors
///
/// Returns an error if the app action fails while restoring agents.
pub fn default_respawn_missing_agents(app: &mut App) -> Result<()> {
    crate::app::Actions::new().respawn_missing_agents(app)
}

/// Runs the initialized interactive TUI flow.
///
/// # Errors
///
/// Returns an error if the TUI, update installer, or restart hook fails.
pub fn run_interactive(
    config: Config,
    storage: Storage,
    settings: Settings,
    storage_load_error: Option<String>,
    deps: &InteractiveDeps,
) -> Result<()> {
    let cwd = std::env::current_dir().ok();

    let cwd_project_root = cwd
        .as_ref()
        .map(|cwd| crate::git::repository_workspace_root(cwd).unwrap_or_else(|_| cwd.clone()));

    // Ensure .tenex/ is excluded from git tracking
    if let Some(cwd) = cwd.as_ref()
        && crate::git::is_git_repository(cwd)
        && let Err(e) = crate::git::ensure_tenex_excluded(cwd)
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

/// Queues "What's New" release notes when settings show they have not been seen.
pub fn maybe_queue_whats_new(app: &mut App) {
    maybe_queue_whats_new_with(
        app,
        crate::release_notes::current_version,
        crate::release_notes::changelog_lines_for_version,
        crate::release_notes::whats_new_lines,
        Settings::set_last_seen_version,
    );
}

/// Returns the current Tenex release version.
pub(crate) type CurrentVersion = fn() -> Result<Version>;
/// Loads changelog lines for a specific release version.
pub(crate) type ChangelogLinesForVersion = fn(&Version) -> Result<Vec<String>>;
/// Loads "What's New" lines between two release versions.
pub(crate) type WhatsNewLines = fn(Option<&Version>, &Version) -> Result<Vec<String>>;
/// Persists the last release version shown to the user.
pub(crate) type SetLastSeenVersion = fn(&mut Settings, &Version) -> std::io::Result<()>;

/// Queues "What's New" release notes using injected release-note hooks.
pub fn maybe_queue_whats_new_with(
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

/// Prompts to restart the mux daemon when the running daemon version is stale.
pub fn maybe_prompt_restart_mux_daemon(app: &mut App) {
    maybe_prompt_restart_mux_daemon_with(
        app,
        crate::mux::version,
        crate::mux::running_daemon_version,
        crate::mux::socket_display,
        env_mux_socket,
    );
}

/// Returns the expected mux daemon version.
pub(crate) type MuxVersion = fn() -> String;
/// Returns the version reported by the running mux daemon, when present.
pub(crate) type RunningDaemonVersion = fn() -> Result<Option<String>>;
/// Returns the human-readable mux socket path.
pub(crate) type SocketDisplay = fn() -> Result<String>;
/// Returns the `TENEX_MUX_SOCKET` value, when present.
pub(crate) type EnvMuxSocket = fn() -> Option<String>;

/// Prompts to restart the mux daemon using injected version and socket hooks.
pub fn maybe_prompt_restart_mux_daemon_with(
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

/// Reads the `TENEX_MUX_SOCKET` environment override when it is non-empty.
#[must_use]
pub fn env_mux_socket() -> Option<String> {
    env_mux_socket_from(|| std::env::var("TENEX_MUX_SOCKET").ok())
}

/// Reads, trims, and filters an injected `TENEX_MUX_SOCKET` value.
#[must_use]
pub fn env_mux_socket_from(get_env: EnvMuxSocket) -> Option<String> {
    let value = get_env()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Prompts to restart the mux daemon for explicit daemon and expected versions.
pub fn maybe_prompt_restart_mux_daemon_for_versions_with(
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

/// Renames an unreadable state file to a timestamped corrupt-state backup.
#[must_use]
pub fn preserve_corrupt_state_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    preserve_corrupt_state_file_with(path, std::time::SystemTime::now)
}

/// Supplies the current system time for corrupt-state backup names.
pub(crate) type SystemTimeNow = fn() -> std::time::SystemTime;

fn unix_timestamp_seconds(now: std::time::SystemTime) -> Option<u64> {
    now.duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|dur| dur.as_secs())
}

/// Renames an unreadable state file using an injected clock.
#[must_use]
pub fn preserve_corrupt_state_file_with(
    path: &std::path::Path,
    now: SystemTimeNow,
) -> Option<std::path::PathBuf> {
    let file_name = path.file_name()?.to_string_lossy();

    let timestamp = unix_timestamp_seconds(now())?;
    let preserved = path.with_file_name(format!("{file_name}.corrupt-{timestamp}"));

    std::fs::rename(path, &preserved).ok()?;
    Some(preserved)
}

/// Finds the installed binary path used for process restart.
#[must_use]
pub fn find_installed_binary(name: &str) -> std::path::PathBuf {
    use crate::paths;

    let cargo_home = std::env::var("CARGO_HOME")
        .ok()
        .map(std::path::PathBuf::from);
    let home_dir = paths::home_dir();

    find_installed_binary_with_dirs(name, cargo_home.as_deref(), home_dir.as_deref())
}

fn find_installed_binary_with_dirs(
    name: &str,
    cargo_home: Option<&std::path::Path>,
    home_dir: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let candidates = [
        cargo_home.map(|h| h.join("bin").join(name)),
        home_dir.map(|h| h.join(".cargo").join("bin").join(name)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return candidate;
        }
    }

    std::path::PathBuf::from(name)
}

#[cfg(any(test, feature = "test-support", coverage))]
/// Finds the installed binary using an explicit `CARGO_HOME` candidate.
///
/// This is exposed for Tenex's test suite so coverage does not depend on
/// whether the host machine happens to have Tenex installed.
#[must_use]
pub fn find_installed_binary_with_cargo_home(
    name: &str,
    cargo_home: &std::path::Path,
) -> std::path::PathBuf {
    find_installed_binary_with_dirs(name, Some(cargo_home), None)
}

#[cfg(unix)]
/// Unix `exec` hook used when restarting the current process.
pub(crate) type RestartExec = fn(std::path::PathBuf, Vec<String>) -> std::io::Error;

#[cfg(unix)]
/// Test override for the Unix restart `exec` hook.
pub static RESTART_EXEC_OVERRIDE: std::sync::OnceLock<std::sync::Mutex<Option<RestartExec>>> =
    std::sync::OnceLock::new();

#[cfg(all(any(test, feature = "test-support", coverage), unix))]
/// Test override for the installed binary path used by Unix restarts.
pub static INSTALLED_BINARY_OVERRIDE: std::sync::OnceLock<
    std::sync::Mutex<Option<std::path::PathBuf>>,
> = std::sync::OnceLock::new();

#[cfg(unix)]
/// Returns the test override for the Unix restart `exec` hook, when set.
#[must_use]
pub fn restart_exec_override() -> Option<RestartExec> {
    let lock = RESTART_EXEC_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None));
    let guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard
}

#[cfg(unix)]
/// Finds the binary path to use for process restart.
#[must_use]
pub fn installed_binary_for_restart(name: &str) -> std::path::PathBuf {
    #[cfg(any(test, feature = "test-support", coverage))]
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
/// Executes the installed binary in place on Unix.
#[must_use]
pub fn exec_restart(installed: std::path::PathBuf, args: Vec<String>) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(installed).args(args).exec()
}

#[cfg(unix)]
/// Restarts Tenex by replacing the current Unix process with the installed binary.
///
/// # Errors
///
/// Always returns an error if `exec` fails. On success this function does not
/// return because the process image has been replaced.
pub fn restart_current_process() -> Result<()> {
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
/// Restarts Tenex by spawning the installed binary and exiting the current process.
///
/// # Errors
///
/// Returns an error if spawning the replacement process fails.
pub fn restart_current_process() -> Result<()> {
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

/// Reports whether the mux daemon is currently running.
pub(crate) type MuxIsRunning = fn() -> bool;
/// Lists mux sessions for reset cleanup.
pub(crate) type MuxListSessions = fn(SessionManager) -> Result<Vec<crate::mux::Session>>;
/// Kills a single mux session during reset cleanup.
pub(crate) type MuxKillSession = fn(SessionManager, &str) -> Result<()>;
/// Removes runtime resources owned by an agent during reset cleanup.
pub(crate) type CleanupAgentRuntime = fn(&crate::Agent) -> Result<()>;
/// Returns the repository path used by the reset flow.
pub(crate) type CurrentDir = fn() -> Result<std::path::PathBuf>;
/// Opens the repository used by the reset flow.
pub(crate) type OpenRepository = fn(&std::path::Path) -> Result<crate::git::Repository>;
/// Removes a git worktree during reset cleanup.
pub(crate) type RemoveWorktree =
    for<'repo> fn(&crate::git::WorktreeManager<'repo>, &str) -> Result<()>;
/// Deletes a git branch during reset cleanup.
pub(crate) type DeleteBranch = for<'repo> fn(&crate::git::BranchManager<'repo>, &str) -> Result<()>;
/// Prompts for the reset scope.
pub(crate) type PromptResetScope = fn(bool) -> Result<ResetScope>;
/// Prompts for final reset confirmation.
pub(crate) type ConfirmReset = fn(bool) -> Result<bool>;

/// Lists mux sessions using the production session manager.
///
/// # Errors
///
/// Returns an error if the session manager cannot list sessions.
pub fn mux_list_sessions_for_reset_deps(mux: SessionManager) -> Result<Vec<crate::mux::Session>> {
    mux.list()
}

/// Kills a mux session using the production session manager.
///
/// # Errors
///
/// Returns an error if the session manager cannot kill the named session.
pub fn mux_kill_session_for_reset_deps(mux: SessionManager, name: &str) -> Result<()> {
    mux.kill(name)
}

fn current_dir_for_reset_deps() -> Result<std::path::PathBuf> {
    current_dir_for_reset_deps_with(std::env::current_dir)
}

/// Returns the process current directory for reset dependency injection.
pub(crate) type GetCurrentDir = fn() -> std::io::Result<std::path::PathBuf>;

/// Returns the current directory using an injected getter.
///
/// # Errors
///
/// Returns an error if the injected current-directory getter fails.
pub fn current_dir_for_reset_deps_with(
    get_current_dir: GetCurrentDir,
) -> Result<std::path::PathBuf> {
    Ok(get_current_dir()?)
}

/// Removes a git worktree using the production worktree manager.
///
/// # Errors
///
/// Returns an error if the worktree manager cannot remove the worktree.
pub fn remove_worktree_for_reset_deps(
    manager: &crate::git::WorktreeManager<'_>,
    name: &str,
) -> Result<()> {
    manager.remove(name)
}

/// Deletes a git branch using the production branch manager.
///
/// # Errors
///
/// Returns an error if the branch manager cannot delete the branch.
pub fn delete_branch_for_reset_deps(
    manager: &crate::git::BranchManager<'_>,
    name: &str,
) -> Result<()> {
    manager.delete(name)
}

#[derive(Clone, Copy, Debug)]
/// Dependencies injected into the reset command flow.
pub struct ResetDeps {
    /// Mux-daemon running-state hook.
    pub mux_is_running: MuxIsRunning,
    /// Mux session listing hook.
    pub mux_list_sessions: MuxListSessions,
    /// Mux session kill hook.
    pub mux_kill_session: MuxKillSession,
    /// Agent runtime cleanup hook.
    pub cleanup_agent_runtime: CleanupAgentRuntime,
    /// Current directory hook.
    pub current_dir: CurrentDir,
    /// Repository opening hook.
    pub open_repository: OpenRepository,
    /// Worktree removal hook.
    pub remove_worktree: RemoveWorktree,
    /// Branch deletion hook.
    pub delete_branch: DeleteBranch,
}

impl ResetDeps {
    /// Builds the reset dependency bundle used by the production CLI.
    #[must_use]
    pub fn production() -> Self {
        Self {
            mux_is_running: crate::mux::is_server_running,
            mux_list_sessions: mux_list_sessions_for_reset_deps,
            mux_kill_session: mux_kill_session_for_reset_deps,
            cleanup_agent_runtime: crate::cleanup_agent_runtime,
            current_dir: current_dir_for_reset_deps,
            open_repository: crate::git::open_repository,
            remove_worktree: remove_worktree_for_reset_deps,
            delete_branch: delete_branch_for_reset_deps,
        }
    }
}

/// Runs reset cleanup against explicit storage and reset dependencies.
///
/// # Errors
///
/// Returns an error if prompt handling, repository lookup, cleanup, or state
/// persistence fails.
pub fn cmd_reset_with_storage(
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

/// Runs reset cleanup with injected prompt handlers.
///
/// # Errors
///
/// Returns an error if prompt handling, repository lookup, cleanup, or state
/// persistence fails.
pub fn cmd_reset_with_storage_with_prompts(
    force: bool,
    storage: &mut Storage,
    mux: SessionManager,
    deps: ResetDeps,
    prompt_reset_scope: PromptResetScope,
    confirm_reset: ConfirmReset,
) -> Result<()> {
    use crate::git::WorktreeManager;
    use std::collections::HashSet;

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
    let branch_mgr = repo.as_ref().map(crate::git::BranchManager::new);

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

/// Flushes stdout before reading a prompt response.
pub(crate) type FlushStdout = fn() -> std::io::Result<()>;
/// Reads a line of prompt input from stdin.
pub(crate) type ReadLineStdin = fn(&mut String) -> std::io::Result<usize>;

fn flush_stdout() -> std::io::Result<()> {
    use std::io::Write;
    std::io::stdout().flush()
}

fn read_line_stdin(input: &mut String) -> std::io::Result<usize> {
    std::io::stdin().read_line(input)
}

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

/// Reads the reset scope using injected IO hooks.
///
/// # Errors
///
/// Returns an error if flushing stdout or reading stdin fails.
pub fn prompt_reset_scope_with_io(
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

/// Lists orphaned Tenex mux sessions for a reset scope.
#[must_use]
pub fn list_orphaned_sessions_with<S: std::hash::BuildHasher>(
    mux: SessionManager,
    mux_running: bool,
    scope: ResetScope,
    instance_prefix: &str,
    storage_sessions: &std::collections::HashSet<String, S>,
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

/// Prints the reset plan for persisted agents and orphaned mux sessions.
pub fn print_reset_plan(storage: &Storage, orphaned_sessions: &[String]) {
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

/// Prompts for final reset confirmation through standard IO.
///
/// # Errors
///
/// Returns an error if flushing stdout or reading stdin fails.
pub fn confirm_reset(force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }

    print!("Continue? [y/N] ");

    let mut input = String::new();
    confirm_reset_with_io(force, flush_stdout, read_line_stdin, &mut input)
}

/// Reads final reset confirmation using injected IO hooks.
///
/// # Errors
///
/// Returns an error if flushing stdout or reading stdin fails.
pub fn confirm_reset_with_io(
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
