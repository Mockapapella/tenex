//! Configuration management for Muster

mod keys;

pub use keys::{Action, ActionGroup, KeyBindings};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    /// Default program to run for agents (e.g., "claude", "aider")
    #[serde(default = "default_program")]
    pub default_program: String,

    /// Prefix for branch names created by muster
    #[serde(default = "default_branch_prefix")]
    pub branch_prefix: String,

    /// Auto-accept prompts (experimental)
    #[serde(default)]
    pub auto_yes: bool,

    /// Poll interval in milliseconds for updating agent output
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,

    /// Maximum number of concurrent agents
    #[serde(default = "default_max_agents")]
    pub max_agents: usize,

    /// Directory for worktrees
    #[serde(default = "default_worktree_dir")]
    pub worktree_dir: PathBuf,

    /// Keybindings configuration
    #[serde(default)]
    pub keys: KeyBindings,
}

fn default_program() -> String {
    "claude".to_string()
}

fn default_branch_prefix() -> String {
    "muster/".to_string()
}

const fn default_poll_interval() -> u64 {
    100
}

const fn default_max_agents() -> usize {
    10
}

fn default_worktree_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".muster")
        .join("worktrees")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_program: default_program(),
            branch_prefix: default_branch_prefix(),
            auto_yes: false,
            poll_interval_ms: default_poll_interval(),
            max_agents: default_max_agents(),
            worktree_dir: default_worktree_dir(),
            keys: KeyBindings::default(),
        }
    }
}

impl Config {
    /// Load configuration from the default location
    ///
    /// # Errors
    ///
    /// Returns an error if reading or parsing the config file fails
    pub fn load() -> Result<Self> {
        let path = Self::default_path();
        if path.exists() {
            Self::load_from(&path)
        } else {
            Ok(Self::default())
        }
    }

    /// Load configuration from a specific path
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let mut config: Self = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        // Ensure any new default keybindings are available
        config.keys.merge_defaults();
        Ok(config)
    }

    /// Save configuration to the default location
    ///
    /// # Errors
    ///
    /// Returns an error if the config directory cannot be created or the file cannot be written
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path();
        self.save_to(&path)
    }

    /// Save configuration to a specific path
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }
        let contents = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(path, contents)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }

    /// Get the default configuration file path
    #[must_use]
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("muster")
            .join("config.json")
    }

    /// Get the state file path (for agent persistence)
    #[must_use]
    pub fn state_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("muster")
            .join("state.json")
    }

    /// Generate a branch name for a new agent
    #[must_use]
    pub fn generate_branch_name(&self, title: &str) -> String {
        let sanitized: String = title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .to_lowercase();
        let truncated = if sanitized.len() > 50 {
            &sanitized[..50]
        } else {
            &sanitized
        };
        format!("{}{}", self.branch_prefix, truncated.trim_matches('-'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.default_program, "claude");
        assert_eq!(config.branch_prefix, "muster/");
        assert!(!config.auto_yes);
        assert_eq!(config.poll_interval_ms, 100);
        assert_eq!(config.max_agents, 10);
    }

    #[test]
    fn test_save_and_load() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.json");

        let config = Config {
            default_program: "aider".to_string(),
            branch_prefix: "test/".to_string(),
            auto_yes: true,
            poll_interval_ms: 200,
            max_agents: 5,
            worktree_dir: temp_dir.path().join("worktrees"),
            keys: KeyBindings::default(),
        };

        config.save_to(&config_path)?;
        let loaded = Config::load_from(&config_path)?;

        assert_eq!(config, loaded);
        Ok(())
    }

    #[test]
    fn test_load_nonexistent_returns_default() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("nonexistent.json");

        assert!(Config::load_from(&config_path).is_err());
        Ok(())
    }

    #[test]
    fn test_generate_branch_name() {
        let config = Config::default();

        assert_eq!(
            config.generate_branch_name("Fix Auth Bug"),
            "muster/fix-auth-bug"
        );
        assert_eq!(
            config.generate_branch_name("hello_world"),
            "muster/hello-world"
        );
        assert_eq!(config.generate_branch_name("  spaces  "), "muster/spaces");
    }

    #[test]
    fn test_generate_branch_name_truncation() {
        let config = Config::default();
        let long_title = "a".repeat(100);
        let branch = config.generate_branch_name(&long_title);
        assert!(branch.len() <= 57);
    }

    #[test]
    fn test_serde_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let json = r#"{"default_program": "codex"}"#;
        let config: Config = serde_json::from_str(json)?;

        assert_eq!(config.default_program, "codex");
        assert_eq!(config.branch_prefix, "muster/");
        assert!(!config.auto_yes);
        Ok(())
    }

    #[test]
    fn test_default_paths() {
        let config_path = Config::default_path();
        let state_path = Config::state_path();

        assert!(config_path.to_string_lossy().contains("muster"));
        assert!(state_path.to_string_lossy().contains("muster"));
    }

    #[test]
    fn test_generate_branch_name_special_chars() {
        let config = Config::default();

        // Test various special characters
        assert_eq!(
            config.generate_branch_name("fix@#$%bug"),
            "muster/fix----bug"
        );
        assert_eq!(
            config.generate_branch_name("hello/world"),
            "muster/hello-world"
        );
    }

    #[test]
    fn test_save_creates_parent_dirs() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let nested_path = temp_dir.path().join("deep/nested/dir/config.json");

        let config = Config::default();
        config.save_to(&nested_path)?;

        assert!(nested_path.exists());
        Ok(())
    }

    #[test]
    fn test_default_worktree_dir_has_path() {
        let dir = default_worktree_dir();
        assert!(dir.to_string_lossy().contains("worktrees"));
    }

    #[test]
    fn test_config_clone() {
        let config = Config::default();
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_config_debug() {
        let config = Config::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("Config"));
    }
}
