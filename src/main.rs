//! Tenex - Terminal multiplexer for AI coding agents

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use tenex::App;
use tenex::agent::Storage;
use tenex::config::Config;

mod tui;

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
}

fn main() -> Result<()> {
    // Clear the log file on startup
    let _ = std::fs::write("/tmp/tenex.log", "");

    // Log to /tmp/tenex.log - tail with: tail -f /tmp/tenex.log
    // Set DEBUG=0-3 to control verbosity (0=off, 1=warn, 2=info, 3=debug)
    let debug_level = std::env::var("DEBUG")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(0);

    if debug_level > 0 {
        let level = match debug_level {
            1 => tracing::Level::WARN,
            2 => tracing::Level::INFO,
            _ => tracing::Level::DEBUG,
        };

        let file_appender = tracing_appender::rolling::never("/tmp", "tenex.log");
        tracing_subscriber::fmt()
            .with_writer(file_appender)
            .with_max_level(level)
            .with_ansi(false)
            .init();
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
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
    };

    let config = Config::default();
    let storage = Storage::load().unwrap_or_default();

    match cli.command {
        Some(Commands::Reset { force }) => cmd_reset(force),
        None => {
            // Ensure .tenex/ is excluded from git tracking
            if let Ok(cwd) = std::env::current_dir() {
                let _ = tenex::git::ensure_tenex_excluded(&cwd);
            }

            let mut app = App::new(config, storage);

            // Auto-connect to any existing worktrees
            let action_handler = tenex::app::Actions::new();
            if let Err(e) = action_handler.auto_connect_worktrees(&mut app) {
                eprintln!("Warning: Failed to auto-connect to worktrees: {e}");
            }

            tui::run(app)
        }
    }
}

fn cmd_reset(force: bool) -> Result<()> {
    use std::collections::HashSet;
    use std::io::{self, Write};
    use tenex::git::WorktreeManager;
    use tenex::tmux::SessionManager;

    let storage = Storage::load().unwrap_or_default();
    let tmux = SessionManager::new();

    // Skip orphan detection when using isolated state (TENEX_STATE_PATH set).
    // Otherwise we'd kill real tenex sessions that aren't in the isolated state.
    let using_isolated_state = std::env::var("TENEX_STATE_PATH").is_ok();

    // Find orphaned muster tmux sessions (not in storage)
    let storage_sessions: HashSet<_> = storage.iter().map(|a| a.tmux_session.clone()).collect();
    let orphaned_sessions: Vec<_> = if using_isolated_state {
        Vec::new() // Don't scan for orphans when using isolated state
    } else {
        tmux.list()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.name.starts_with("tenex-") && !storage_sessions.contains(&s.name))
            .collect()
    };

    if storage.is_empty() && orphaned_sessions.is_empty() {
        println!("No agents to reset.");
        return Ok(());
    }

    // List what will be reset
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
        println!("Orphaned tmux sessions to kill:\n");
        for session in &orphaned_sessions {
            println!("  - {}", session.name);
        }
        println!();
    }

    if !force {
        print!("Continue? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Kill tmux sessions and remove worktrees/branches
    let repo_path = std::env::current_dir()?;
    let repo = tenex::git::open_repository(&repo_path).ok();
    let worktree_mgr = repo.as_ref().map(WorktreeManager::new);
    let branch_mgr = repo.as_ref().map(tenex::git::BranchManager::new);

    for agent in storage.iter() {
        let _ = tmux.kill(&agent.tmux_session);
        if let Some(ref mgr) = worktree_mgr {
            let _ = mgr.remove(&agent.branch);
        }
        // Also try to delete branch directly in case worktree was already gone
        if let Some(ref mgr) = branch_mgr {
            let _ = mgr.delete(&agent.branch);
        }
    }

    // Kill orphaned sessions
    for session in &orphaned_sessions {
        let _ = tmux.kill(&session.name);
    }

    // Clear storage
    let mut storage = storage;
    storage.clear();
    storage.save()?;

    println!("Reset complete.");
    Ok(())
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

    // Note: test_cmd_reset_force moved to tests/cli_binary_test.rs
    // to properly isolate state via subprocess + TENEX_STATE_PATH env var.
    // Running cmd_reset directly in a unit test would corrupt real state.
}
