//! CLI support for the `tenex` binary.
//!
//! This module contains the parser and command-flow glue used by the production binary. It is not
//! a stable automation API and carries no backwards-compatibility promise.

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
    match &cli.command {
        Some(Commands::Reset { force }) => {
            crate::migration::migrate_default_state_dir()
                .unwrap_or_else(|err| warn_migration_failure(&err));
            cmd_reset(*force)
        }
        Some(Commands::Muxd) => crate::mux::run_mux_daemon(),
        None => {
            crate::migration::migrate_default_state_dir()
                .unwrap_or_else(|err| warn_migration_failure(&err));
            cmd_default()
        }
    }
}

fn warn_migration_failure(err: &anyhow::Error) {
    eprintln!("Warning: Failed to migrate Tenex state directory: {err}");
}

/// Runs the default interactive CLI path.
///
/// # Errors
///
/// Returns an error if state initialization, state persistence, update
/// installation, process restart, or the TUI runner fails.
fn cmd_default() -> Result<()> {
    let config = Config::default();
    let state_path = Config::state_path();
    let settings = Settings::load();
    let (mut storage, storage_load_error) = load_storage(&state_path);
    let env_mux_socket = env_mux_socket();
    ensure_instance_initialized(
        &config,
        &mut storage,
        &state_path,
        env_mux_socket.as_deref(),
    )?;

    let mut did_backfill = storage.backfill_workspace_kinds();
    did_backfill |= storage.backfill_child_titles();
    did_backfill |= storage.backfill_repo_roots();
    did_backfill |= storage.backfill_conversation_ids();
    if did_backfill {
        storage.save_to(&state_path)?;
    }

    run_interactive(config, storage, settings, storage_load_error)
}

fn init_logging() {
    let log_path = crate::paths::log_path();
    let debug = std::env::var("DEBUG").ok();
    if let Some(parent) = log_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "Warning: Failed to create log directory {}: {e}",
            parent.display()
        );
    }

    // Clear the log file on startup
    if let Err(e) = std::fs::write(&log_path, "") {
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
fn load_storage(state_path: &std::path::Path) -> (Storage, Option<String>) {
    match Storage::load_from(state_path) {
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
            .or_else(|| crate::mux::socket_display().ok());

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

/// Runs the initialized interactive TUI flow.
///
/// # Errors
///
/// Returns an error if the TUI, update installer, or process restart fails.
fn run_interactive(
    config: Config,
    storage: Storage,
    settings: Settings,
    storage_load_error: Option<String>,
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
        match crate::update::check_for_update() {
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
    if let Err(e) = crate::app::Actions::new().auto_connect_worktrees(&mut app) {
        eprintln!("Warning: Failed to auto-connect to worktrees: {e}");
    }

    // After reboot/crash, stored agents may outlive the mux daemon. Attempt to restore missing
    // mux sessions and windows from persisted state.
    if let Err(e) = crate::app::Actions::new().respawn_missing_agents(&mut app) {
        eprintln!("Warning: Failed to respawn agents: {e}");
    }

    if let Some(info) = crate::tui::run(app)? {
        println!(
            "Updating Tenex from {} to {}...",
            info.current_version, info.latest_version
        );
        crate::update::install_latest()?;
        restart_current_process()?;
    }

    Ok(())
}

/// Queues "What's New" release notes when settings show they have not been seen.
pub fn maybe_queue_whats_new(app: &mut App) {
    let Ok(current_version) = crate::release_notes::current_version() else {
        return;
    };

    let raw_last_seen = app.data.settings.last_seen_version.clone();
    let parsed_last_seen = raw_last_seen
        .as_deref()
        .and_then(|raw| Version::parse(raw).ok());

    match parsed_last_seen {
        None => {
            if raw_last_seen.is_none() {
                if let Err(e) =
                    Settings::set_last_seen_version(&mut app.data.settings, &current_version)
                {
                    eprintln!("Warning: Failed to save settings: {e}");
                }
                return;
            }

            // Corrupt or non-semver value: show current release notes once, then overwrite.
            match crate::release_notes::changelog_lines_for_version(&current_version) {
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
                if let Err(e) =
                    Settings::set_last_seen_version(&mut app.data.settings, &current_version)
                {
                    eprintln!("Warning: Failed to save settings: {e}");
                }
                return;
            }

            if last_seen == current_version {
                return;
            }

            match crate::release_notes::whats_new_lines(Some(&last_seen), &current_version) {
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
    let expected_version = crate::mux::version();
    let Ok(Some(daemon_version)) = crate::mux::running_daemon_version() else {
        return;
    };

    if daemon_version == expected_version {
        return;
    }

    let socket = crate::mux::socket_display().unwrap_or_else(|_| "<unknown>".to_string());

    app.data.ui.muxd_version_mismatch = Some(MuxdVersionMismatchInfo {
        socket,
        daemon_version,
        expected_version,
        env_mux_socket: env_mux_socket(),
    });

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::RestartMuxDaemon,
        }
        .into(),
    );
}

/// Reads the `TENEX_MUX_SOCKET` environment override when it is non-empty.
#[must_use]
pub fn env_mux_socket() -> Option<String> {
    let value = std::env::var("TENEX_MUX_SOCKET").ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Renames an unreadable state file to a timestamped corrupt-state backup.
#[must_use]
pub fn preserve_corrupt_state_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let file_name = path.file_name()?.to_string_lossy();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())?;
    let preserved = path.with_file_name(format!("{file_name}.corrupt-{timestamp}"));

    std::fs::rename(path, &preserved).ok()?;
    Some(preserved)
}

/// Finds the installed binary path used for process restart.
#[must_use]
pub fn find_installed_binary(name: &str) -> std::path::PathBuf {
    use crate::paths;

    let candidates = [
        std::env::var("CARGO_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .map(|path| path.join("bin").join(name)),
        paths::home_dir().map(|path| path.join(".cargo").join("bin").join(name)),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return candidate;
        }
    }

    std::path::PathBuf::from(name)
}

#[cfg(unix)]
/// Restarts Tenex by replacing the current Unix process with the installed binary.
///
/// # Errors
///
/// Always returns an error if `exec` fails. On success this function does not
/// return because the process image has been replaced.
pub fn restart_current_process() -> Result<()> {
    use std::os::unix::process::CommandExt;

    let args: Vec<String> = std::env::args().skip(1).collect();
    // After `cargo install --force`, spawning a new process and exiting can leave the
    // restarted Tenex in the background (job control), causing terminal I/O errors.
    // Prefer `exec` to replace the current process in-place.

    let installed = find_installed_binary(env!("CARGO_PKG_NAME"));

    // `exec` replaces the current process on success; on failure it returns an io::Error.
    let err = std::process::Command::new(installed).args(args).exec();
    Err(anyhow::Error::new(err).context("Failed to restart Tenex"))
}

#[cfg(windows)]
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
    use crate::git::WorktreeManager;
    use std::collections::HashSet;

    let mut storage = Storage::load().unwrap_or_default();
    let mux = SessionManager::new();
    let mux_running = crate::mux::is_server_running();

    let instance_prefix = storage.instance_session_prefix();
    let scope = prompt_reset_scope(force)?;

    // Find orphaned Tenex mux sessions (not in storage)
    let storage_sessions: HashSet<_> = storage
        .iter()
        .map(|agent| agent.mux_session.clone())
        .collect();
    let orphaned_sessions =
        list_orphaned_sessions(mux, mux_running, scope, &instance_prefix, &storage_sessions);

    if storage.is_empty() && orphaned_sessions.is_empty() {
        if storage.mux_socket.take().is_some() {
            storage.save()?;
        }
        println!("No agents to reset.");
        return Ok(());
    }

    print_reset_plan(&storage, &orphaned_sessions);

    if !confirm_reset(force)? {
        println!("Aborted.");
        return Ok(());
    }

    // Kill mux sessions and remove worktrees/branches
    let repo_path = std::env::current_dir()?;
    let repo = crate::git::open_repository(&repo_path).ok();
    let worktree_mgr = repo.as_ref().map(WorktreeManager::new);
    let branch_mgr = repo.as_ref().map(crate::git::BranchManager::new);

    if !mux_running {
        eprintln!("Warning: Mux daemon is not running; skipping session termination.");
    }

    for agent in storage.iter() {
        if mux_running && let Err(e) = mux.kill(&agent.mux_session) {
            eprintln!(
                "Warning: Failed to kill mux session {}: {e}",
                agent.mux_session
            );
        }
        if let Err(e) = crate::cleanup_agent_runtime(agent) {
            eprintln!(
                "Warning: Failed to clean up runtime for {} ({}): {e}",
                agent.title, agent.mux_session
            );
        }
        if let Some(ref mgr) = worktree_mgr
            && let Err(e) = mgr.remove(&agent.branch)
        {
            eprintln!("Warning: Failed to remove worktree {}: {e}", agent.branch);
        }
        // Also try to delete branch directly in case worktree was already gone
        if let Some(ref mgr) = branch_mgr
            && let Err(e) = mgr.delete(&agent.branch)
        {
            eprintln!("Warning: Failed to delete branch {}: {e}", agent.branch);
        }
    }

    // Kill orphaned sessions
    if mux_running {
        for session in &orphaned_sessions {
            if let Err(e) = mux.kill(session) {
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

fn prompt_reset_scope(force: bool) -> Result<ResetScope> {
    use std::io::Write;

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
    std::io::stdout().flush()?;
    std::io::stdin().read_line(&mut input)?;

    let trimmed = input.trim();
    if trimmed == "2" || trimmed.eq_ignore_ascii_case("all") {
        return Ok(ResetScope::AllInstances);
    }

    Ok(ResetScope::ThisInstance)
}

/// Lists orphaned Tenex mux sessions for a reset scope.
#[must_use]
fn list_orphaned_sessions<S: std::hash::BuildHasher>(
    mux: SessionManager,
    mux_running: bool,
    scope: ResetScope,
    instance_prefix: &str,
    storage_sessions: &std::collections::HashSet<String, S>,
) -> Vec<String> {
    if !mux_running {
        return Vec::new();
    }

    let sessions = {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(mux.list());
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
    use std::io::Write;

    if force {
        return Ok(true);
    }

    print!("Continue? [y/N] ");

    let mut input = String::new();
    std::io::stdout().flush()?;
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
