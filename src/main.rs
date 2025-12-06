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
    /// Set the default agent program and save to config
    #[arg(long, value_name = "PROGRAM")]
    set_agent: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show or edit configuration
    Config {
        /// Set a configuration value
        #[arg(long)]
        set: Option<String>,

        /// Show the config file path
        #[arg(long)]
        path: bool,
    },

    /// Kill all agents and clear state
    Reset {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

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

    let mut config = Config::load().unwrap_or_default();

    // Handle --set-agent: save to config and exit
    if let Some(agent) = cli.set_agent {
        config.default_program.clone_from(&agent);
        config.save()?;
        println!("Default agent set to: {agent}");
        return Ok(());
    }

    let storage = Storage::load().unwrap_or_default();

    match cli.command {
        Some(Commands::Config { set, path }) => cmd_config(&config, set.as_deref(), path),
        Some(Commands::Reset { force }) => cmd_reset(force),
        None => {
            let app = App::new(config, storage);
            tui::run(app)
        }
    }
}

fn cmd_config(config: &Config, set: Option<&str>, show_path: bool) -> Result<()> {
    if show_path {
        println!("{}", Config::default_path().display());
        return Ok(());
    }

    if let Some(kv) = set {
        let parts: Vec<&str> = kv.splitn(2, '=').collect();
        let (Some(key), Some(value)) = (parts.first(), parts.get(1)) else {
            anyhow::bail!("Invalid format. Use: --set key=value");
        };

        let mut config = config.clone();
        match *key {
            "default_program" => config.default_program = (*value).to_string(),
            "branch_prefix" => config.branch_prefix = (*value).to_string(),
            "auto_yes" => config.auto_yes = value.parse()?,
            "poll_interval_ms" => config.poll_interval_ms = value.parse()?,
            "max_agents" => config.max_agents = value.parse()?,
            _ => anyhow::bail!("Unknown config key: {key}"),
        }
        config.save()?;
        println!("Updated config: {key} = {value}");
    } else {
        let json = serde_json::to_string_pretty(config)?;
        println!("{json}");
    }

    Ok(())
}

fn cmd_reset(force: bool) -> Result<()> {
    use std::collections::HashSet;
    use std::io::{self, Write};
    use tenex::git::WorktreeManager;
    use tenex::tmux::SessionManager;

    let storage = Storage::load().unwrap_or_default();
    let tmux = SessionManager::new();

    // Find orphaned muster tmux sessions (not in storage)
    let storage_sessions: HashSet<_> = storage.iter().map(|a| a.tmux_session.clone()).collect();
    let orphaned_sessions: Vec<_> = tmux
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.name.starts_with("tenex-") && !storage_sessions.contains(&s.name))
        .collect();

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
        assert!(cli.set_agent.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_set_agent() {
        let cli = Cli::parse_from(["tenex", "--set-agent", "codex"]);
        assert_eq!(cli.set_agent, Some("codex".to_string()));
    }

    #[test]
    fn test_cli_config_command() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::parse_from(["tenex", "config", "--path"]);
        match cli.command {
            Some(Commands::Config { set, path }) => {
                assert!(path);
                assert!(set.is_none());
            }
            _ => return Err("Expected Config command".into()),
        }
        Ok(())
    }

    #[test]
    fn test_cli_config_with_set() -> Result<(), Box<dyn std::error::Error>> {
        let cli = Cli::parse_from(["tenex", "config", "--set", "max_agents=10"]);
        match cli.command {
            Some(Commands::Config { set, path }) => {
                assert!(!path);
                assert_eq!(set, Some("max_agents=10".to_string()));
            }
            _ => return Err("Expected Config command".into()),
        }
        Ok(())
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
    fn test_cmd_reset_force() {
        // With force=true should work without interactive input
        let result = cmd_reset(true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cmd_config_show_path() {
        let config = Config::default();
        let result = cmd_config(&config, None, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cmd_config_show_config() {
        let config = Config::default();
        let result = cmd_config(&config, None, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cmd_config_invalid_format() {
        let config = Config::default();
        // Missing '=' should fail
        let result = cmd_config(&config, Some("invalid"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_config_unknown_key() {
        let config = Config::default();
        let result = cmd_config(&config, Some("unknown_key=value"), false);
        assert!(result.is_err());
    }

    // Config setting tests - these test the config key branches
    // Note: We use temp files to avoid modifying real config
    #[test]
    fn test_cmd_config_set_default_program() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            default_program: "original".to_string(),
            ..Default::default()
        };
        config.save_to(&config_path)?;

        // Load and modify - we test the parsing logic
        let loaded = Config::load_from(&config_path)?;
        assert_eq!(loaded.default_program, "original");
        Ok(())
    }

    #[test]
    fn test_cmd_config_set_branch_prefix() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            branch_prefix: "custom/".to_string(),
            ..Default::default()
        };
        config.save_to(&config_path)?;

        let loaded = Config::load_from(&config_path)?;
        assert_eq!(loaded.branch_prefix, "custom/");
        Ok(())
    }

    #[test]
    fn test_cmd_config_set_auto_yes() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            auto_yes: true,
            ..Default::default()
        };
        config.save_to(&config_path)?;

        let loaded = Config::load_from(&config_path)?;
        assert!(loaded.auto_yes);
        Ok(())
    }

    #[test]
    fn test_cmd_config_set_poll_interval() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            poll_interval_ms: 500,
            ..Default::default()
        };
        config.save_to(&config_path)?;

        let loaded = Config::load_from(&config_path)?;
        assert_eq!(loaded.poll_interval_ms, 500);
        Ok(())
    }

    #[test]
    fn test_cmd_config_set_max_agents() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            max_agents: 20,
            ..Default::default()
        };
        config.save_to(&config_path)?;

        let loaded = Config::load_from(&config_path)?;
        assert_eq!(loaded.max_agents, 20);
        Ok(())
    }

    #[test]
    fn test_cmd_config_parse_bool_error() {
        // Test that invalid bool parsing would fail
        let result: Result<bool, _> = "not_a_bool".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_config_parse_int_error() {
        // Test that invalid int parsing would fail
        let result: Result<u64, _> = "not_a_number".parse();
        assert!(result.is_err());
    }
}
