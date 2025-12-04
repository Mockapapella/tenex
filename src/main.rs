//! Muster - Terminal multiplexer for AI coding agents

use anyhow::Result;
use clap::{Parser, Subcommand};
use muster::App;
use muster::agent::Storage;
use muster::config::Config;

mod tui;

/// Terminal multiplexer for AI coding agents
#[derive(Parser)]
#[command(name = "muster")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Agent program to run
    #[arg(short, long, default_value = "claude")]
    program: String,

    /// Auto-accept prompts (experimental)
    #[arg(short = 'y', long)]
    auto_yes: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new agent session
    New {
        /// Initial prompt for the agent
        #[arg(short, long)]
        prompt: Option<String>,

        /// Name/title for the agent
        #[arg(short, long)]
        name: Option<String>,
    },

    /// List all agents
    List {
        /// Show only running agents
        #[arg(short, long)]
        running: bool,
    },

    /// Attach to an agent's terminal
    Attach {
        /// Agent ID or index
        id: String,
    },

    /// Terminate an agent
    Kill {
        /// Agent ID or index
        id: String,
    },

    /// Pause an agent and commit work
    Pause {
        /// Agent ID or index
        id: String,
    },

    /// Resume a paused agent
    Resume {
        /// Agent ID or index
        id: String,
    },

    /// Show or edit configuration
    Config {
        /// Set a configuration value
        #[arg(short, long)]
        set: Option<String>,

        /// Show the config file path
        #[arg(long)]
        path: bool,
    },

    /// Clear all agents and state
    Reset {
        /// Skip confirmation
        #[arg(short, long)]
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

    let cli = Cli::parse();

    let mut config = Config::load().unwrap_or_default();

    if cli.program != "claude" {
        config.default_program = cli.program;
    }
    if cli.auto_yes {
        config.auto_yes = true;
    }

    let storage = Storage::load().unwrap_or_default();

    match cli.command {
        Some(Commands::New { prompt, name }) => {
            cmd_new(&config, &storage, name.as_deref(), prompt.as_deref())
        }
        Some(Commands::List { running }) => {
            cmd_list(&storage, running);
            Ok(())
        }
        Some(Commands::Attach { id }) => cmd_attach(&storage, &id),
        Some(Commands::Kill { id }) => cmd_kill(&storage, &id),
        Some(Commands::Pause { id }) => cmd_pause(&storage, &id),
        Some(Commands::Resume { id }) => cmd_resume(&storage, &id),
        Some(Commands::Config { set, path }) => cmd_config(&config, set.as_deref(), path),
        Some(Commands::Reset { force }) => cmd_reset(force),
        None => {
            let app = App::new(config, storage);
            tui::run(app)
        }
    }
}

fn cmd_new(
    config: &Config,
    storage: &Storage,
    name: Option<&str>,
    prompt: Option<&str>,
) -> Result<()> {
    use muster::app::Actions;

    if storage.len() >= config.max_agents {
        anyhow::bail!(
            "Maximum agents ({}) reached. Kill some agents first.",
            config.max_agents
        );
    }

    let title = name.unwrap_or("new-agent");
    let storage = storage.clone();
    let mut app = App::new(config.clone(), storage);

    let handler = Actions::new();
    handler.create_agent(&mut app, title, prompt)?;

    if let Some(agent) = app.storage.iter().last() {
        println!("Created agent: {}", agent.title);
        println!("  Branch: {}", agent.branch);
        println!("  Session: {}", agent.tmux_session);
        println!("\nAttach with: muster attach {}", agent.short_id());
    }

    Ok(())
}

fn cmd_list(storage: &Storage, running_only: bool) {
    let agents: Vec<_> = if running_only {
        storage
            .iter()
            .filter(|a| a.status == muster::Status::Running)
            .collect()
    } else {
        storage.iter().collect()
    };

    if agents.is_empty() {
        println!("No agents found.");
        return;
    }

    println!(
        "{:<10} {:<20} {:<10} {:<10} BRANCH",
        "ID", "TITLE", "STATUS", "AGE"
    );
    println!("{}", "-".repeat(70));

    for agent in agents {
        println!(
            "{:<10} {:<20} {:<10} {:<10} {}",
            agent.short_id(),
            truncate(&agent.title, 18),
            agent.status,
            agent.age_string(),
            agent.branch
        );
    }
}

fn cmd_attach(storage: &Storage, id: &str) -> Result<()> {
    use muster::tmux::SessionManager;

    let agent = find_agent(storage, id)?;
    let manager = SessionManager::new();

    if manager.exists(&agent.tmux_session) {
        manager.attach(&agent.tmux_session)?;
    } else {
        anyhow::bail!("Session '{}' not found", agent.tmux_session);
    }

    Ok(())
}

fn cmd_kill(storage: &Storage, id: &str) -> Result<()> {
    use muster::git::WorktreeManager;
    use muster::tmux::SessionManager;

    let agent = find_agent(storage, id)?;
    let mut storage = storage.clone();

    let manager = SessionManager::new();
    let _ = manager.kill(&agent.tmux_session);

    let repo_path = std::env::current_dir()?;
    if let Ok(repo) = muster::git::open_repository(&repo_path) {
        let worktree_mgr = WorktreeManager::new(&repo);
        let _ = worktree_mgr.remove(&agent.branch);
    }

    storage.remove(agent.id);
    storage.save()?;

    println!("Killed agent: {}", agent.title);
    Ok(())
}

fn cmd_pause(storage: &Storage, id: &str) -> Result<()> {
    use muster::tmux::SessionManager;

    let agent = find_agent(storage, id)?;

    if !agent.status.can_pause() {
        anyhow::bail!("Agent '{}' cannot be paused", agent.title);
    }

    let mut storage = storage.clone();
    let manager = SessionManager::new();

    let _ = manager.kill(&agent.tmux_session);

    if let Some(agent) = storage.get_mut(agent.id) {
        agent.set_status(muster::Status::Paused);
    }
    storage.save()?;

    println!("Paused agent: {}", agent.title);
    Ok(())
}

fn cmd_resume(storage: &Storage, id: &str) -> Result<()> {
    use muster::tmux::SessionManager;

    let agent = find_agent(storage, id)?;

    if !agent.status.can_resume() {
        anyhow::bail!("Agent '{}' cannot be resumed", agent.title);
    }

    let mut storage = storage.clone();
    let manager = SessionManager::new();

    manager.create(
        &agent.tmux_session,
        &agent.worktree_path,
        Some(&agent.program),
    )?;

    if let Some(agent) = storage.get_mut(agent.id) {
        agent.set_status(muster::Status::Running);
    }
    storage.save()?;

    println!("Resumed agent: {}", agent.title);
    Ok(())
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
    use muster::tmux::SessionManager;
    use std::io::{self, Write};

    if !force {
        print!("This will delete all agents and their worktrees. Continue? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let storage = Storage::load().unwrap_or_default();
    let manager = SessionManager::new();

    for agent in storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    let mut storage = storage;
    storage.clear();
    storage.save()?;

    println!("Reset complete. All agents removed.");
    Ok(())
}

fn find_agent<'a>(storage: &'a Storage, id: &str) -> Result<&'a muster::Agent> {
    if let Some(agent) = storage.find_by_short_id(id) {
        return Ok(agent);
    }

    if let Ok(index) = id.parse::<usize>()
        && let Some(agent) = storage.get_by_index(index)
    {
        return Ok(agent);
    }

    anyhow::bail!("Agent not found: {id}")
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn test_find_agent_not_found() {
        let storage = Storage::new();
        let result = find_agent(&storage, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(["muster"]);
        assert_eq!(cli.program, "claude");
        assert!(!cli.auto_yes);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_with_options() {
        let cli = Cli::parse_from(["muster", "-p", "aider", "-y"]);
        assert_eq!(cli.program, "aider");
        assert!(cli.auto_yes);
    }

    #[test]
    fn test_cli_new_command() {
        let cli = Cli::parse_from(["muster", "new", "-p", "fix bug", "-n", "bugfix"]);
        match cli.command {
            Some(Commands::New { prompt, name }) => {
                assert_eq!(prompt, Some("fix bug".to_string()));
                assert_eq!(name, Some("bugfix".to_string()));
            }
            _ => panic!("Expected New command"),
        }
    }

    #[test]
    fn test_cli_list_command() {
        let cli = Cli::parse_from(["muster", "list", "--running"]);
        match cli.command {
            Some(Commands::List { running }) => {
                assert!(running);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_cli_attach_command() {
        let cli = Cli::parse_from(["muster", "attach", "abc123"]);
        match cli.command {
            Some(Commands::Attach { id }) => {
                assert_eq!(id, "abc123");
            }
            _ => panic!("Expected Attach command"),
        }
    }

    #[test]
    fn test_cli_kill_command() {
        let cli = Cli::parse_from(["muster", "kill", "abc123"]);
        match cli.command {
            Some(Commands::Kill { id }) => {
                assert_eq!(id, "abc123");
            }
            _ => panic!("Expected Kill command"),
        }
    }

    #[test]
    fn test_cli_config_command() {
        let cli = Cli::parse_from(["muster", "config", "--path"]);
        match cli.command {
            Some(Commands::Config { set, path }) => {
                assert!(path);
                assert!(set.is_none());
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_cli_reset_command() {
        let cli = Cli::parse_from(["muster", "reset", "--force"]);
        match cli.command {
            Some(Commands::Reset { force }) => {
                assert!(force);
            }
            _ => panic!("Expected Reset command"),
        }
    }
}
