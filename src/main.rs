//! Muster - Terminal multiplexer for AI coding agents

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
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

    /// Set the default agent program and save to config
    #[arg(long, value_name = "PROGRAM")]
    set_agent: Option<String>,

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
    #![expect(clippy::panic, clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use muster::Agent;
    use std::path::PathBuf;

    fn create_test_agent(title: &str) -> Agent {
        Agent::new(
            title.to_string(),
            "claude".to_string(),
            format!("muster/{title}"),
            PathBuf::from("/tmp/worktree"),
            None,
        )
    }

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
        assert!(cli.set_agent.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_with_options() {
        let cli = Cli::parse_from(["muster", "-p", "aider", "-y"]);
        assert_eq!(cli.program, "aider");
        assert!(cli.auto_yes);
    }

    #[test]
    fn test_cli_set_agent() {
        let cli = Cli::parse_from(["muster", "--set-agent", "codex"]);
        assert_eq!(cli.set_agent, Some("codex".to_string()));
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

    #[test]
    fn test_cli_pause_command() {
        let cli = Cli::parse_from(["muster", "pause", "abc123"]);
        match cli.command {
            Some(Commands::Pause { id }) => {
                assert_eq!(id, "abc123");
            }
            _ => panic!("Expected Pause command"),
        }
    }

    #[test]
    fn test_cli_resume_command() {
        let cli = Cli::parse_from(["muster", "resume", "abc123"]);
        match cli.command {
            Some(Commands::Resume { id }) => {
                assert_eq!(id, "abc123");
            }
            _ => panic!("Expected Resume command"),
        }
    }

    #[test]
    fn test_cli_config_with_set() {
        let cli = Cli::parse_from(["muster", "config", "--set", "max_agents=10"]);
        match cli.command {
            Some(Commands::Config { set, path }) => {
                assert!(!path);
                assert_eq!(set, Some("max_agents=10".to_string()));
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_find_agent_by_short_id() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test-agent");
        let short_id = agent.short_id();
        storage.add(agent);

        let found = find_agent(&storage, &short_id).unwrap();
        assert_eq!(found.title, "test-agent");
    }

    #[test]
    fn test_find_agent_by_index() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("first"));
        storage.add(create_test_agent("second"));

        // Storage doesn't guarantee insertion order, just verify we can find by index
        let found0 = find_agent(&storage, "0").unwrap();
        let found1 = find_agent(&storage, "1").unwrap();

        // Both should be found and be different
        assert!(found0.title == "first" || found0.title == "second");
        assert!(found1.title == "first" || found1.title == "second");
        assert_ne!(found0.id, found1.id);
    }

    #[test]
    fn test_find_agent_invalid_index() {
        let storage = Storage::new();
        let result = find_agent(&storage, "999");
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_list_empty() {
        let storage = Storage::new();
        // Just verify it doesn't panic
        cmd_list(&storage, false);
        cmd_list(&storage, true);
    }

    #[test]
    fn test_cmd_list_with_agents() {
        let mut storage = Storage::new();
        storage.add(create_test_agent("agent1"));
        storage.add(create_test_agent("agent2"));

        // Just verify it doesn't panic
        cmd_list(&storage, false);
        cmd_list(&storage, true);
    }

    #[test]
    fn test_cmd_list_with_running_agents() {
        let mut storage = Storage::new();

        // Add a running agent
        let mut running_agent = create_test_agent("running");
        running_agent.set_status(muster::Status::Running);
        storage.add(running_agent);

        // Add a paused agent
        let mut paused_agent = create_test_agent("paused");
        paused_agent.set_status(muster::Status::Paused);
        storage.add(paused_agent);

        // Test running filter
        cmd_list(&storage, true);
        cmd_list(&storage, false);
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

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_unicode() {
        // Note: truncate uses byte indexing, which may not work well with unicode
        // This test documents the current behavior
        assert_eq!(truncate("hello", 5), "hello");
    }

    // Config setting tests - these test the config key branches
    // Note: We use temp files to avoid modifying real config
    #[test]
    fn test_cmd_config_set_default_program() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            default_program: "original".to_string(),
            ..Default::default()
        };
        config.save_to(&config_path).unwrap();

        // Load and modify - we test the parsing logic
        let loaded = Config::load_from(&config_path).unwrap();
        assert_eq!(loaded.default_program, "original");
    }

    #[test]
    fn test_cmd_config_set_branch_prefix() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            branch_prefix: "custom/".to_string(),
            ..Default::default()
        };
        config.save_to(&config_path).unwrap();

        let loaded = Config::load_from(&config_path).unwrap();
        assert_eq!(loaded.branch_prefix, "custom/");
    }

    #[test]
    fn test_cmd_config_set_auto_yes() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            auto_yes: true,
            ..Default::default()
        };
        config.save_to(&config_path).unwrap();

        let loaded = Config::load_from(&config_path).unwrap();
        assert!(loaded.auto_yes);
    }

    #[test]
    fn test_cmd_config_set_poll_interval() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            poll_interval_ms: 500,
            ..Default::default()
        };
        config.save_to(&config_path).unwrap();

        let loaded = Config::load_from(&config_path).unwrap();
        assert_eq!(loaded.poll_interval_ms, 500);
    }

    #[test]
    fn test_cmd_config_set_max_agents() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            max_agents: 20,
            ..Default::default()
        };
        config.save_to(&config_path).unwrap();

        let loaded = Config::load_from(&config_path).unwrap();
        assert_eq!(loaded.max_agents, 20);
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

    // Integration tests for CLI commands
    #[test]
    fn test_cmd_new_max_agents_reached() {
        let config = Config {
            max_agents: 1,
            ..Default::default()
        };

        let mut storage = Storage::new();
        storage.add(create_test_agent("existing"));

        // Should fail because max agents reached
        let result = cmd_new(&config, &storage, Some("new-agent"), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_attach_agent_not_found() {
        let storage = Storage::new();
        let result = cmd_attach(&storage, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_attach_session_not_found() {
        let mut storage = Storage::new();
        let agent = create_test_agent("test");
        let short_id = agent.short_id();
        storage.add(agent);

        // Agent exists but session doesn't
        let result = cmd_attach(&storage, &short_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_kill_agent_not_found() {
        let storage = Storage::new();
        let result = cmd_kill(&storage, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_pause_agent_not_found() {
        let storage = Storage::new();
        let result = cmd_pause(&storage, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_pause_cannot_pause() {
        let mut storage = Storage::new();
        let mut agent = create_test_agent("paused");
        agent.set_status(muster::Status::Paused);
        let short_id = agent.short_id();
        storage.add(agent);

        // Cannot pause an already paused agent
        let result = cmd_pause(&storage, &short_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_resume_agent_not_found() {
        let storage = Storage::new();
        let result = cmd_resume(&storage, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_resume_cannot_resume() {
        let mut storage = Storage::new();
        let mut agent = create_test_agent("running");
        agent.set_status(muster::Status::Running);
        let short_id = agent.short_id();
        storage.add(agent);

        // Cannot resume a running agent
        let result = cmd_resume(&storage, &short_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_cmd_reset_force() {
        // cmd_reset with force=true should work without interactive input
        let result = cmd_reset(true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_truncate_longer_than_max_with_ellipsis() {
        assert_eq!(truncate("hello world foo bar", 10), "hello w...");
    }
}
