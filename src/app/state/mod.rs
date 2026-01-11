//! Application state
//!
//! This module contains the main `App` struct and its sub-states,
//! organized into focused modules by domain.

mod command_palette;
mod git_op;
mod input;
mod lifecycle;
mod models;
mod navigation;
mod review;
mod scroll;
mod settings_menu;
mod spawn;
mod text_input;
mod ui;

pub use command_palette::CommandPaletteState;
pub use git_op::GitOpState;
pub use input::InputState;
pub use models::ModelSelectorState;
pub use review::ReviewState;
pub use settings_menu::SettingsMenuState;
pub use spawn::SpawnState;
pub use spawn::WorktreeConflictInfo;
pub use ui::{DiffEdit, DiffLineMeta, UiState};

use crate::agent::Storage;
use crate::config::Config;
use crate::state::AppMode;
use serde::{Deserialize, Serialize};

use super::{Actions, AppData, Settings};

// Re-export BranchInfo so it's available from app module
pub use crate::git::BranchInfo;

/// Slash command definition (for the `/` command palette)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommand {
    /// Command name, including the leading `/` (for example, `/help`).
    pub name: &'static str,
    /// Human-readable description shown in the palette.
    pub description: &'static str,
}

/// All available slash commands (shown in the command palette)
pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/agents",
        description: "Configure agent programs (default/planner/review)",
    },
    SlashCommand {
        name: "/help",
        description: "Show help",
    },
];

/// Main application state
#[derive(Debug)]
pub struct App {
    /// Current application mode (typed).
    pub mode: AppMode,

    /// Persistent application data.
    pub data: AppData,

    /// Action handler context.
    pub actions: Actions,
}

impl App {
    /// Create a new application with the given config, storage, and settings
    #[must_use]
    pub const fn new(
        config: Config,
        storage: Storage,
        settings: Settings,
        keyboard_enhancement_supported: bool,
    ) -> Self {
        Self {
            mode: AppMode::normal(),
            data: AppData::new(config, storage, settings, keyboard_enhancement_supported),
            actions: Actions::new(),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        )
    }
}

/// Input mode for text entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal mode (no text input)
    #[default]
    Normal,
    /// Editing text
    Editing,
}

/// Tab in the detail pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tab {
    /// Terminal preview
    #[default]
    Preview,
    /// Git diff view
    Diff,
    /// Current branch commit list
    Commits,
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview => write!(f, "Preview"),
            Self::Diff => write!(f, "Diff"),
            Self::Commits => write!(f, "Commits"),
        }
    }
}

#[cfg(test)]
mod tests;
