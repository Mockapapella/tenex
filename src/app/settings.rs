//! User settings persistence
//!
//! Stores user preferences that persist across sessions, such as
//! keyboard remapping choices.

use crate::config::Config;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, warn};

/// Which model/program Tenex should run when spawning new agents.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentProgram {
    /// Use the `codex` CLI when spawning new agents.
    Codex,
    /// Use the `claude` CLI (Tenex default) when spawning new agents.
    #[default]
    Claude,
    /// Use a user-provided command when spawning new agents.
    Custom,
}

impl AgentProgram {
    /// All supported programs, in display order.
    pub const ALL: &'static [Self] = &[Self::Codex, Self::Claude, Self::Custom];

    /// Lowercase label shown in the UI.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Custom => "custom",
        }
    }
}

/// Which kind of agent should be configured in settings.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    /// Default agent program used for new agents.
    #[default]
    Default,
    /// Agent program used for planning swarms.
    Planner,
    /// Agent program used for review swarms.
    Review,
}

impl AgentRole {
    /// All supported roles, in display order.
    pub const ALL: &'static [Self] = &[Self::Default, Self::Planner, Self::Review];

    /// Lowercase label shown in the UI.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Planner => "planner",
            Self::Review => "review",
        }
    }

    /// Title-cased label for the settings menu.
    #[must_use]
    pub const fn menu_label(self) -> &'static str {
        match self {
            Self::Default => "Default agent",
            Self::Planner => "Planner agent",
            Self::Review => "Review agent",
        }
    }
}

/// Persistent user settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Whether to use Ctrl+N instead of Ctrl+M for merge (for incompatible terminals)
    #[serde(default)]
    pub merge_key_remapped: bool,

    /// Whether the user has been asked about keyboard remapping
    #[serde(default)]
    pub keyboard_remap_asked: bool,

    /// Which model/program to use for new agents
    #[serde(default)]
    pub agent_program: AgentProgram,

    /// Custom agent command (used when `agent_program == Custom`)
    #[serde(default)]
    pub custom_agent_command: String,

    /// Which model/program to use for planner agents (planning swarms)
    #[serde(default)]
    pub planner_agent_program: AgentProgram,

    /// Custom planner command (used when `planner_agent_program == Custom`)
    #[serde(default)]
    pub planner_custom_agent_command: String,

    /// Which model/program to use for review agents (review swarms)
    #[serde(default)]
    pub review_agent_program: AgentProgram,

    /// Custom review command (used when `review_agent_program == Custom`)
    #[serde(default)]
    pub review_custom_agent_command: String,

    /// The most recent Tenex version for which the user has seen "What's New".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_version: Option<String>,
}

impl Settings {
    fn deserialize_with_upgrade_defaults(content: &str) -> Result<Self, serde_json::Error> {
        let mut value: serde_json::Value = serde_json::from_str(content)?;

        // Preserve behavior across upgrades:
        // Before planner/review-specific settings existed, those swarms used the default agent
        // program. When upgrading from an older settings.json, default planner/review settings
        // to whatever `agent_program` was set to (and copy the custom command as well).
        if let Some(obj) = value.as_object_mut() {
            let agent_program = obj.get("agent_program").cloned();
            let custom_command = obj.get("custom_agent_command").cloned();

            if let Some(agent_program) = agent_program {
                if !obj.contains_key("planner_agent_program") {
                    obj.insert("planner_agent_program".to_string(), agent_program.clone());
                }
                if !obj.contains_key("review_agent_program") {
                    obj.insert("review_agent_program".to_string(), agent_program);
                }
            }

            if let Some(custom_command) = custom_command {
                if !obj.contains_key("planner_custom_agent_command") {
                    obj.insert(
                        "planner_custom_agent_command".to_string(),
                        custom_command.clone(),
                    );
                }
                if !obj.contains_key("review_custom_agent_command") {
                    obj.insert("review_custom_agent_command".to_string(), custom_command);
                }
            }
        }

        serde_json::from_value(value)
    }

    /// Get the settings file path
    #[must_use]
    pub fn path() -> PathBuf {
        Config::settings_path()
    }

    /// Load settings from disk, returning defaults if file doesn't exist
    #[must_use]
    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            debug!("Settings file not found, using defaults");
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => match Self::deserialize_with_upgrade_defaults(&content) {
                Ok(settings) => {
                    debug!("Loaded settings from {:?}", path);
                    settings
                }
                Err(e) => {
                    warn!("Failed to parse settings file: {}", e);
                    Self::default()
                }
            },
            Err(e) => {
                warn!("Failed to read settings file: {}", e);
                Self::default()
            }
        }
    }

    /// Save settings to disk
    ///
    /// # Errors
    ///
    /// Returns an error if the settings file cannot be written.
    pub fn save(&self) -> std::io::Result<()> {
        // Unit tests run as part of `cargo test` should never touch the user's real settings file
        // (typically `~/.tenex/settings.json`). Some tests exercise key handlers that call
        // `Settings::save()` as a side effect; without this guard, running the test suite can
        // clobber a developer's local Tenex settings (including the selected default agent).
        if cfg!(test) {
            return Err(std::io::Error::other(
                "Refusing to write Tenex settings during tests",
            ));
        }

        let path = Self::path();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(&path, content)?;
        debug!("Saved settings to {:?}", path);
        Ok(())
    }

    /// Persist that the user has seen release notes for the given version.
    ///
    /// # Errors
    ///
    /// Returns an error if the settings file cannot be written.
    pub fn set_last_seen_version(&mut self, version: &Version) -> std::io::Result<()> {
        self.last_seen_version = Some(version.to_string());
        self.save()
    }

    /// Enable the merge key remap and save
    ///
    /// # Errors
    ///
    /// Returns an error if the settings file cannot be written.
    pub fn enable_merge_remap(&mut self) -> std::io::Result<()> {
        self.merge_key_remapped = true;
        self.keyboard_remap_asked = true;
        self.save()
    }

    /// Mark that user declined the remap and save
    ///
    /// # Errors
    ///
    /// Returns an error if the settings file cannot be written.
    pub fn decline_merge_remap(&mut self) -> std::io::Result<()> {
        self.merge_key_remapped = false;
        self.keyboard_remap_asked = true;
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_settings_save_is_disabled_in_tests() {
        let err_kind = Settings::default().save().err().map(|err| err.kind());
        assert_eq!(err_kind, Some(std::io::ErrorKind::Other));
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert!(!settings.merge_key_remapped);
        assert!(!settings.keyboard_remap_asked);
        assert_eq!(settings.agent_program, AgentProgram::Claude);
        assert!(settings.custom_agent_command.is_empty());
        assert_eq!(settings.planner_agent_program, AgentProgram::Claude);
        assert!(settings.planner_custom_agent_command.is_empty());
        assert_eq!(settings.review_agent_program, AgentProgram::Claude);
        assert!(settings.review_custom_agent_command.is_empty());
        assert!(settings.last_seen_version.is_none());
    }

    #[test]
    fn test_settings_save_load() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let path = temp_dir.path().join("settings.json");

        let settings = Settings {
            merge_key_remapped: true,
            keyboard_remap_asked: true,
            agent_program: AgentProgram::Codex,
            custom_agent_command: "echo hello".to_string(),
            ..Settings::default()
        };

        // Save manually to temp location
        let content = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&path, content)?;

        // Load from temp location
        let loaded: Settings = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        assert!(loaded.merge_key_remapped);
        assert!(loaded.keyboard_remap_asked);

        Ok(())
    }

    #[test]
    fn test_settings_path_returns_path() {
        let path = Settings::path();
        assert_eq!(
            path.file_name().and_then(|p| p.to_str()),
            Some("settings.json")
        );
    }

    #[test]
    fn test_settings_load_nonexistent_returns_default() {
        // Load() handles nonexistent file gracefully
        let settings = Settings::load();
        // Should return defaults without panic - either value is valid
        // (we just want to ensure load() doesn't panic)
        let _ = settings.merge_key_remapped;
    }

    #[test]
    fn test_settings_clone() {
        let settings = Settings {
            merge_key_remapped: true,
            keyboard_remap_asked: false,
            agent_program: AgentProgram::Claude,
            custom_agent_command: String::new(),
            ..Settings::default()
        };
        let cloned = settings.clone();
        // Verify both original and clone have correct values
        assert!(settings.merge_key_remapped);
        assert!(cloned.merge_key_remapped);
        assert!(!cloned.keyboard_remap_asked);
    }

    #[test]
    fn test_settings_debug() {
        let settings = Settings::default();
        let debug_str = format!("{settings:?}");
        assert!(debug_str.contains("Settings"));
        assert!(debug_str.contains("merge_key_remapped"));
    }

    #[test]
    fn test_settings_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let original = Settings {
            merge_key_remapped: true,
            keyboard_remap_asked: true,
            agent_program: AgentProgram::Custom,
            custom_agent_command: "my-agent --flag".to_string(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&original)?;
        let parsed: Settings = serde_json::from_str(&json)?;
        assert_eq!(original.merge_key_remapped, parsed.merge_key_remapped);
        assert_eq!(original.keyboard_remap_asked, parsed.keyboard_remap_asked);
        assert_eq!(original.agent_program, parsed.agent_program);
        assert_eq!(original.custom_agent_command, parsed.custom_agent_command);
        Ok(())
    }

    #[test]
    fn test_settings_serde_defaults() -> Result<(), Box<dyn std::error::Error>> {
        // Test that missing fields get default values
        let json = "{}";
        let settings: Settings = serde_json::from_str(json)?;
        assert!(!settings.merge_key_remapped);
        assert!(!settings.keyboard_remap_asked);
        assert_eq!(settings.agent_program, AgentProgram::Claude);
        assert!(settings.custom_agent_command.is_empty());
        assert_eq!(settings.planner_agent_program, AgentProgram::Claude);
        assert!(settings.planner_custom_agent_command.is_empty());
        assert_eq!(settings.review_agent_program, AgentProgram::Claude);
        assert!(settings.review_custom_agent_command.is_empty());
        assert!(settings.last_seen_version.is_none());
        Ok(())
    }

    #[test]
    fn test_upgrade_defaults_copy_default_program_to_planner_review()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = Settings::deserialize_with_upgrade_defaults(r#"{"agent_program":"codex"}"#)?;
        assert_eq!(settings.agent_program, AgentProgram::Codex);
        assert_eq!(settings.planner_agent_program, AgentProgram::Codex);
        assert_eq!(settings.review_agent_program, AgentProgram::Codex);
        Ok(())
    }

    #[test]
    fn test_upgrade_defaults_copy_custom_program_and_command_to_planner_review()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = Settings::deserialize_with_upgrade_defaults(
            r#"{"agent_program":"custom","custom_agent_command":"my-agent --flag"}"#,
        )?;
        assert_eq!(settings.agent_program, AgentProgram::Custom);
        assert_eq!(settings.custom_agent_command, "my-agent --flag");
        assert_eq!(settings.planner_agent_program, AgentProgram::Custom);
        assert_eq!(settings.planner_custom_agent_command, "my-agent --flag");
        assert_eq!(settings.review_agent_program, AgentProgram::Custom);
        assert_eq!(settings.review_custom_agent_command, "my-agent --flag");
        Ok(())
    }

    #[test]
    fn test_upgrade_defaults_do_not_override_existing_planner_review_settings()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = Settings::deserialize_with_upgrade_defaults(
            r#"{"agent_program":"claude","planner_agent_program":"codex","review_agent_program":"codex"}"#,
        )?;
        assert_eq!(settings.agent_program, AgentProgram::Claude);
        assert_eq!(settings.planner_agent_program, AgentProgram::Codex);
        assert_eq!(settings.review_agent_program, AgentProgram::Codex);
        Ok(())
    }
}
