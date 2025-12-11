//! Configuration management for Tenex

mod keys;

pub use keys::{Action, ActionGroup, get_action, status_hints};

use std::path::PathBuf;

/// Application configuration (uses hardcoded defaults)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Default program to run for agents (e.g., "claude", "aider")
    pub default_program: String,

    /// Prefix for branch names created by tenex
    pub branch_prefix: String,

    /// Auto-accept prompts (experimental)
    pub auto_yes: bool,

    /// Poll interval in milliseconds for updating agent output
    pub poll_interval_ms: u64,

    /// Directory for worktrees
    pub worktree_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_program: "claude --allow-dangerously-skip-permissions".to_string(),
            branch_prefix: "tenex/".to_string(),
            auto_yes: false,
            poll_interval_ms: 100,
            worktree_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".tenex")
                .join("worktrees"),
        }
    }
}

impl Config {
    /// Get the state file path (for agent persistence)
    ///
    /// Respects the `TENEX_STATE_PATH` environment variable if set,
    /// allowing tests to use an isolated state file.
    #[must_use]
    pub fn state_path() -> PathBuf {
        if let Ok(path) = std::env::var("TENEX_STATE_PATH") {
            return PathBuf::from(path);
        }
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tenex")
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

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(
            config.default_program,
            "claude --allow-dangerously-skip-permissions"
        );
        assert_eq!(config.branch_prefix, "tenex/");
        assert!(!config.auto_yes);
        assert_eq!(config.poll_interval_ms, 100);
    }

    #[test]
    fn test_generate_branch_name() {
        let config = Config::default();

        assert_eq!(
            config.generate_branch_name("Fix Auth Bug"),
            "tenex/fix-auth-bug"
        );
        assert_eq!(
            config.generate_branch_name("hello_world"),
            "tenex/hello-world"
        );
        assert_eq!(config.generate_branch_name("  spaces  "), "tenex/spaces");
    }

    #[test]
    fn test_generate_branch_name_truncation() {
        let config = Config::default();
        let long_title = "a".repeat(100);
        let branch = config.generate_branch_name(&long_title);
        assert!(branch.len() <= 57);
    }

    #[test]
    fn test_state_path() {
        let state_path = Config::state_path();
        assert!(state_path.to_string_lossy().contains("tenex"));
    }

    #[test]
    fn test_generate_branch_name_special_chars() {
        let config = Config::default();

        // Test various special characters
        assert_eq!(
            config.generate_branch_name("fix@#$%bug"),
            "tenex/fix----bug"
        );
        assert_eq!(
            config.generate_branch_name("hello/world"),
            "tenex/hello-world"
        );
    }

    #[test]
    fn test_default_worktree_dir_has_path() {
        let config = Config::default();
        assert!(config.worktree_dir.to_string_lossy().contains("worktrees"));
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
