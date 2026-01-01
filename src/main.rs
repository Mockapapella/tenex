//! Tenex - Terminal multiplexer for AI coding agents

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use tenex::App;
use tenex::AppMode;
use tenex::agent::Storage;
use tenex::app::Settings;
use tenex::config::Config;
use tenex::mux::SessionManager;
use tenex::state::UpdatePromptMode;

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

    let cli = parse_cli()?;

    match cli.command {
        Some(Commands::Reset { force }) => {
            if let Err(err) = tenex::migration::migrate_default_state_dir() {
                eprintln!("Warning: Failed to migrate Tenex state directory: {err}");
            }
            cmd_reset(force)
        }
        Some(Commands::Muxd) => tenex::mux::run_mux_daemon(),
        None => {
            if let Err(err) = tenex::migration::migrate_default_state_dir() {
                eprintln!("Warning: Failed to migrate Tenex state directory: {err}");
            }

            let config = Config::default();
            let (mut storage, storage_load_error) = load_storage_with_error();
            ensure_instance_initialized(&config, &mut storage)?;
            let settings = Settings::load();

            run_interactive(config, storage, settings, storage_load_error)
        }
    }
}

fn init_logging() {
    let log_path = tenex::paths::log_path();
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
    let debug_level = std::env::var("DEBUG")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(0);

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
    tracing_subscriber::fmt()
        .with_writer(file_appender)
        .with_max_level(level)
        .with_ansi(false)
        .init();
}

fn parse_cli() -> Result<Cli> {
    match Cli::try_parse() {
        Ok(cli) => Ok(cli),
        Err(e) => {
            // Let --help and --version exit normally
            if e.kind() == clap::error::ErrorKind::DisplayHelp
                || e.kind() == clap::error::ErrorKind::DisplayVersion
            {
                e.exit();
            }
            // For actual errors, show error + help
            eprintln!("error: {}\n", e.kind());
            Cli::command().print_help()?;
            std::process::exit(1);
        }
    }
}

fn load_storage_with_error() -> (Storage, Option<String>) {
    match Storage::load() {
        Ok(storage) => (storage, None),
        Err(err) => {
            let state_path = Config::state_path();
            let mut message = format!("Failed to load state file {}: {err}", state_path.display());

            if let Some(preserved) = preserve_corrupt_state_file(&state_path) {
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

fn ensure_instance_initialized(config: &Config, storage: &mut Storage) -> Result<()> {
    std::fs::create_dir_all(&config.worktree_dir).with_context(|| {
        format!(
            "Failed to create worktrees directory {}",
            config.worktree_dir.display()
        )
    })?;

    let state_path = Config::state_path();
    let state_existed = state_path.exists();
    let previous_instance_id = storage.instance_id.clone();

    let _ = storage.ensure_instance_id();

    if !state_existed || storage.instance_id != previous_instance_id {
        storage.save()?;
    }

    Ok(())
}

fn run_interactive(
    config: Config,
    storage: Storage,
    settings: Settings,
    storage_load_error: Option<String>,
) -> Result<()> {
    // Ensure .tenex/ is excluded from git tracking
    if let Ok(cwd) = std::env::current_dir()
        && let Err(e) = tenex::git::ensure_tenex_excluded(&cwd)
    {
        eprintln!("Warning: Failed to exclude .tenex from git: {e}");
    }

    // keyboard_enhancement_supported will be set in tui::run after terminal setup
    let mut app = App::new(config, storage, settings, false);
    if let Some(message) = storage_load_error {
        app.set_error(message);
    }

    if matches!(&app.mode, AppMode::Normal(_)) {
        match tenex::update::check_for_update() {
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
    let action_handler = tenex::app::Actions::new();
    if let Err(e) = action_handler.auto_connect_worktrees(&mut app) {
        eprintln!("Warning: Failed to auto-connect to worktrees: {e}");
    }

    if let Some(info) = tenex::tui::run(app)? {
        println!(
            "Updating Tenex from {} to {}...",
            info.current_version, info.latest_version
        );
        tenex::update::install_latest()?;
        restart_current_process()?;
    }

    Ok(())
}

fn preserve_corrupt_state_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let file_name = path.file_name()?.to_string_lossy();
    if file_name.is_empty() {
        return None;
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let preserved = path.with_file_name(format!("{file_name}.corrupt-{timestamp}"));

    std::fs::rename(path, &preserved).ok()?;
    Some(preserved)
}

fn restart_current_process() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // After `cargo install --force`, spawning a new process and exiting can leave the
    // restarted Tenex in the background (job control), causing terminal I/O errors.
    // On Unix, prefer `exec` to replace the current process in-place.

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::path::PathBuf;
        use tenex::paths;

        fn find_installed_binary(name: &str) -> PathBuf {
            // Try CARGO_HOME first, then ~/.cargo, then just the binary name (PATH lookup)
            let candidates = [
                std::env::var("CARGO_HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join("bin").join(name)),
                paths::home_dir().map(|h| h.join(".cargo").join("bin").join(name)),
            ];

            for candidate in candidates.into_iter().flatten() {
                if candidate.exists() {
                    return candidate;
                }
            }

            PathBuf::from(name)
        }

        let installed = find_installed_binary(env!("CARGO_PKG_NAME"));

        // `exec` replaces the current process on success; on failure it returns an io::Error.
        let err = std::process::Command::new(installed).args(&args).exec();
        Err(anyhow::Error::new(err).context("Failed to restart Tenex"))
    }

    #[cfg(not(unix))]
    {
        use anyhow::Context;
        // On non-Unix platforms, fall back to spawning via PATH.
        std::process::Command::new(env!("CARGO_PKG_NAME"))
            .args(&args)
            .spawn()
            .context("Failed to restart Tenex")?;
        std::process::exit(0);
    }
}

fn cmd_reset(force: bool) -> Result<()> {
    use std::collections::HashSet;
    use tenex::git::WorktreeManager;

    let mut storage = Storage::load().unwrap_or_default();
    let mux = SessionManager::new();

    let instance_prefix = storage.instance_session_prefix();
    let scope = prompt_reset_scope(force)?;

    // Find orphaned Tenex mux sessions (not in storage)
    let storage_sessions: HashSet<_> = storage.iter().map(|a| a.mux_session.clone()).collect();
    let orphaned_sessions = list_orphaned_sessions(mux, scope, &instance_prefix, &storage_sessions);

    if storage.is_empty() && orphaned_sessions.is_empty() {
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
    let repo = tenex::git::open_repository(&repo_path).ok();
    let worktree_mgr = repo.as_ref().map(WorktreeManager::new);
    let branch_mgr = repo.as_ref().map(tenex::git::BranchManager::new);

    for agent in storage.iter() {
        if let Err(e) = mux.kill(&agent.mux_session) {
            eprintln!(
                "Warning: Failed to kill mux session {}: {e}",
                agent.mux_session
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
    for session in &orphaned_sessions {
        if let Err(e) = mux.kill(session) {
            eprintln!("Warning: Failed to kill orphaned mux session {session}: {e}");
        }
    }

    // Clear storage
    storage.clear();
    storage.save()?;

    println!("Reset complete.");
    Ok(())
}

fn prompt_reset_scope(force: bool) -> Result<ResetScope> {
    use std::io::{self, Write};

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
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let trimmed = input.trim();
    if trimmed == "2" || trimmed.eq_ignore_ascii_case("all") {
        return Ok(ResetScope::AllInstances);
    }

    Ok(ResetScope::ThisInstance)
}

fn list_orphaned_sessions(
    mux: SessionManager,
    scope: ResetScope,
    instance_prefix: &str,
    storage_sessions: &std::collections::HashSet<String>,
) -> Vec<String> {
    let prefix = match scope {
        ResetScope::ThisInstance => instance_prefix,
        ResetScope::AllInstances => "tenex-",
    };

    mux.list()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.name.starts_with(prefix))
        .filter(|s| !storage_sessions.contains(&s.name))
        .map(|s| s.name)
        .collect()
}

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

fn confirm_reset(force: bool) -> Result<bool> {
    use std::io::{self, Write};

    if force {
        return Ok(true);
    }

    print!("Continue? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(["tenex"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_reset_command() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::parse_from(["tenex", "reset", "--force"]);
        match cli.command {
            Some(Commands::Reset { force }) => {
                assert!(force);
            }
            _ => return Err("Expected Reset command".into()),
        }
        Ok(())
    }

    #[test]
    fn test_cli_muxd_command() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::parse_from(["tenex", "muxd"]);
        match cli.command {
            Some(Commands::Muxd) => Ok(()),
            _ => Err("Expected Muxd command".into()),
        }
    }

    // Note: test_cmd_reset_force moved to tests/cli_binary_test.rs
    // to properly isolate state via subprocess + TENEX_STATE_PATH env var.
    // Running cmd_reset directly in a unit test would corrupt real state.
}
