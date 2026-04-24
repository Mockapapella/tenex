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
            // Unit tests should never depend on the presence of an external agent binary (like
            // `claude`) on the host machine. Using a long-running shell command keeps mux sessions
            // alive long enough for follow-up operations in tests.
            default_program: default_agent_program(cfg!(test)),
            branch_prefix: "agent/".to_string(),
            auto_yes: false,
            poll_interval_ms: 100,
            worktree_dir: Self::default_worktree_dir(),
        }
    }
}

fn default_agent_program(test_mode: bool) -> String {
    if test_mode {
        #[cfg(windows)]
        {
            return "powershell -NoProfile -Command \"Start-Sleep -Seconds 3600\"".to_string();
        }
        #[cfg(not(windows))]
        {
            return "sh -c 'sleep 3600'".to_string();
        }
    }

    "claude --allow-dangerously-skip-permissions".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        #[cfg(windows)]
        assert_eq!(
            config.default_program,
            "powershell -NoProfile -Command \"Start-Sleep -Seconds 3600\""
        );
        #[cfg(not(windows))]
        assert_eq!(config.default_program, "sh -c 'sleep 3600'");
        assert_eq!(config.branch_prefix, "agent/");
        assert!(!config.auto_yes);
        assert_eq!(config.poll_interval_ms, 100);
    }

    #[test]
    fn test_default_agent_program_non_test_returns_claude() {
        assert_eq!(
            default_agent_program(false),
            "claude --allow-dangerously-skip-permissions"
        );
    }

    #[test]
    fn test_generate_branch_name() {
        let config = Config::default();

        assert_eq!(
            config.generate_branch_name("Fix Auth Bug"),
            "agent/fix-auth-bug"
        );
        assert_eq!(
            config.generate_branch_name("hello_world"),
            "agent/hello-world"
        );
        assert_eq!(config.generate_branch_name("  spaces  "), "agent/spaces");
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
        let state_path = Config::default_state_path();
        assert_eq!(
            state_path.file_name().and_then(|p| p.to_str()),
            Some("state.json")
        );
        assert!(state_path.to_string_lossy().contains(".tenex"));
    }

    #[test]
    fn test_state_path_relative_env_resolves_from_cwd() {
        let expected = std::env::current_dir().unwrap().join("state.json");
        assert_eq!(
            Config::resolve_state_path_override_with_cwd(
                PathBuf::from("state.json"),
                Some(expected.parent().unwrap().to_path_buf())
            ),
            expected
        );
    }

    #[test]
    fn test_state_path_relative_env_falls_back_when_cwd_is_missing() {
        assert_eq!(
            Config::resolve_state_path_override_with_cwd(PathBuf::from("state.json"), None),
            PathBuf::from("state.json")
        );
    }

    #[test]
    fn test_state_path_from_env_value_ignores_blank_value() {
        assert!(Config::state_path_from_env_value("   ").is_none());
    }

    #[test]
    fn test_state_path_from_env_var_returns_none_when_env_missing() {
        assert!(Config::state_path_from_env_var(Err(std::env::VarError::NotPresent)).is_none());
    }

    #[test]
    fn test_state_path_from_env_var_returns_none_for_blank_value() {
        assert!(Config::state_path_from_env_var(Ok("   ".to_string())).is_none());
    }

    #[test]
    fn test_state_path_from_env_var_accepts_absolute_path() {
        let expected = std::env::temp_dir().join("state.json");
        assert_eq!(
            Config::state_path_from_env_var(Ok(expected.to_string_lossy().to_string())),
            Some(expected)
        );
    }

    #[test]
    fn test_resolve_state_path_override_with_cwd_prefers_absolute_candidate() {
        let absolute = std::env::temp_dir().join("state.json");
        let cwd = std::env::temp_dir().join("other");
        assert_eq!(
            Config::resolve_state_path_override_with_cwd(absolute.clone(), Some(cwd)),
            absolute
        );
    }

    #[test]
    fn test_resolve_state_path_override_joins_current_dir_for_relative_path() {
        let expected = std::env::current_dir().unwrap().join("state.json");
        assert_eq!(Config::resolve_state_path_override("state.json"), expected);
    }

    #[test]
    fn test_default_instance_root_from_none_falls_back_to_dot_tenex() {
        assert_eq!(
            Config::default_instance_root_from(None),
            PathBuf::from(".").join(".tenex")
        );
    }

    #[test]
    fn test_default_instance_root_from_some_appends_tenex_dir() {
        let home = std::env::temp_dir().join("tenex-home");
        assert_eq!(
            Config::default_instance_root_from(Some(home.clone())),
            home.join(".tenex")
        );
    }

    #[test]
    fn test_instance_root_from_state_path_falls_back_when_parent_missing() {
        #[cfg(windows)]
        let root = Path::new("C:\\");
        #[cfg(not(windows))]
        let root = Path::new("/");

        assert_eq!(
            Config::instance_root_from_state_path(root),
            PathBuf::from(".")
        );
    }

    #[test]
    fn test_instance_root_from_state_path_returns_parent_directory() {
        let state_path = std::env::temp_dir().join("state.json");
        assert_eq!(
            Config::instance_root_from_state_path(&state_path),
            std::env::temp_dir()
        );
    }

    #[test]
    fn test_project_dir_name_defaults_when_repo_root_missing_file_name() {
        assert_eq!(Config::project_dir_name(Path::new("")), "project");
    }

    #[test]
    fn test_worktree_leaf_dir_name_strips_prefix_and_replaces_slashes() {
        assert_eq!(
            Config::worktree_leaf_dir_name("agent/feature/foo", "agent/"),
            "feature-foo"
        );
    }

    #[test]
    fn test_worktree_leaf_dir_name_falls_back_to_tenex_prefix() {
        assert_eq!(
            Config::worktree_leaf_dir_name("tenex/feature/foo", "agent/"),
            "feature-foo"
        );
    }

    #[test]
    fn test_worktree_leaf_dir_name_preserves_branch_when_no_prefix_matches() {
        assert_eq!(
            Config::worktree_leaf_dir_name("feature/foo", "agent/"),
            "feature-foo"
        );
    }

    #[test]
    fn test_worktree_path_for_repo_root_uses_project_name_and_leaf_name() {
        let mut config = Config::default();
        let worktree_dir = std::env::temp_dir().join("tenex-worktrees");
        config.worktree_dir = worktree_dir.clone();

        assert_eq!(
            config.worktree_path_for_repo_root(Path::new("repo"), "agent/feature/foo"),
            worktree_dir.join("repo").join("feature-foo")
        );
    }

    #[test]
    fn test_generate_branch_name_special_chars() {
        let config = Config::default();

        // Test various special characters
        assert_eq!(
            config.generate_branch_name("fix@#$%bug"),
            "agent/fix----bug"
        );
        assert_eq!(
            config.generate_branch_name("hello/world"),
            "agent/hello-world"
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
