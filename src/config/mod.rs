//! Configuration management for Tenex

mod keys;

pub use keys::{
    Action, ActionGroup, get_action, get_display_description, get_display_keys, status_hints,
};

use crate::paths;
use std::path::Path;
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
            branch_prefix: "agent/".to_string(),
            auto_yes: false,
            poll_interval_ms: 100,
            worktree_dir: Self::default_worktree_dir(),
        }
    }
}

impl Config {
    fn default_instance_root_from(home_dir: Option<PathBuf>) -> PathBuf {
        let home_dir = home_dir.unwrap_or_else(|| PathBuf::from("."));
        home_dir.join(".tenex")
    }

    fn resolve_state_path_override_with_cwd(candidate: PathBuf, cwd: Option<PathBuf>) -> PathBuf {
        if candidate.is_absolute() {
            return candidate;
        }

        if let Some(cwd) = cwd {
            return cwd.join(candidate);
        }

        candidate
    }

    fn resolve_state_path_override(raw: &str) -> PathBuf {
        let candidate = PathBuf::from(raw);
        Self::resolve_state_path_override_with_cwd(candidate, std::env::current_dir().ok())
    }

    fn state_path_from_env_value(raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(Self::resolve_state_path_override(trimmed))
    }

    fn state_path_from_env_var(raw: Result<String, std::env::VarError>) -> Option<PathBuf> {
        raw.ok()
            .and_then(|value| Self::state_path_from_env_value(&value))
    }

    /// Root directory for Tenex's default instance.
    #[must_use]
    pub fn default_instance_root() -> PathBuf {
        Self::default_instance_root_from(paths::home_dir())
    }

    /// Default location of Tenex's persistent state file.
    #[must_use]
    pub fn default_state_path() -> PathBuf {
        Self::default_instance_root().join("state.json")
    }

    /// Get the state file path (for agent persistence)
    ///
    /// Respects the `TENEX_STATE_PATH` environment variable if set. When it is set,
    /// Tenex derives all instance-specific paths (settings, worktrees, mux socket
    /// fallback) relative to the resulting state file path.
    #[must_use]
    pub fn state_path() -> PathBuf {
        Self::state_path_from_env_var(std::env::var("TENEX_STATE_PATH"))
            .unwrap_or_else(Self::default_state_path)
    }

    /// Root directory for the current Tenex instance.
    ///
    /// - Default: `~/.tenex/`
    /// - With `TENEX_STATE_PATH`: the parent directory of the resolved state path
    #[must_use]
    pub fn instance_root() -> PathBuf {
        Self::instance_root_from_state_path(&Self::state_path())
    }

    /// Path to the settings file for the current Tenex instance.
    ///
    /// - Default: `~/.tenex/settings.json`
    /// - With `TENEX_STATE_PATH`: `settings.json` next to the state file
    #[must_use]
    pub fn settings_path() -> PathBuf {
        Self::instance_root().join("settings.json")
    }

    /// Default worktrees directory for the current Tenex instance.
    ///
    /// - Default: `~/.tenex/worktrees/`
    /// - With `TENEX_STATE_PATH`: `worktrees/` under the instance root
    #[must_use]
    pub fn default_worktree_dir() -> PathBuf {
        Self::instance_root().join("worktrees")
    }

    fn instance_root_from_state_path(state_path: &Path) -> PathBuf {
        state_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    fn project_dir_name(repo_root: &Path) -> String {
        repo_root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map_or_else(|| "project".to_string(), str::to_string)
    }

    fn worktree_leaf_dir_name(branch: &str, branch_prefix: &str) -> String {
        let leaf = branch
            .strip_prefix(branch_prefix)
            .or_else(|| branch.strip_prefix("tenex/"))
            .unwrap_or(branch);
        leaf.replace('/', "-")
    }

    /// Returns the directory Tenex should store worktrees for a given repo root under.
    #[must_use]
    pub fn worktree_dir_for_repo_root(&self, repo_root: &Path) -> PathBuf {
        self.worktree_dir.join(Self::project_dir_name(repo_root))
    }

    /// Returns the worktree path for a given repo root and branch name.
    #[must_use]
    pub fn worktree_path_for_repo_root(&self, repo_root: &Path, branch: &str) -> PathBuf {
        self.worktree_dir_for_repo_root(repo_root)
            .join(Self::worktree_leaf_dir_name(branch, &self.branch_prefix))
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
