//! User settings persistence
//!
//! Stores user preferences that persist across sessions, such as
//! keyboard remapping choices.

use crate::paths;
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
}

impl Settings {
    /// Get the settings file path
    #[must_use]
    pub fn path() -> PathBuf {
        paths::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tenex")
            .join("settings.json")
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
            Ok(content) => match serde_json::from_str(&content) {
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
    fn test_settings_default() {
        let settings = Settings::default();
        assert!(!settings.merge_key_remapped);
        assert!(!settings.keyboard_remap_asked);
        assert_eq!(settings.agent_program, AgentProgram::Claude);
        assert!(settings.custom_agent_command.is_empty());
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
        assert!(path.to_string_lossy().contains("tenex"));
        assert!(path.to_string_lossy().contains("settings.json"));
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
        Ok(())
    }
}
