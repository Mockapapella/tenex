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
mod spawn;
mod text_input;
mod ui;

pub use command_palette::CommandPaletteState;
pub use git_op::GitOpState;
pub use input::InputState;
pub use models::ModelSelectorState;
pub use review::ReviewState;
pub use spawn::SpawnState;
pub use ui::UiState;

use crate::agent::Storage;
use crate::config::Config;
use crate::update::UpdateInfo;
use serde::{Deserialize, Serialize};

use super::Settings;

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
        description: "Select default agent model/program",
    },
    SlashCommand {
        name: "/help",
        description: "Show help",
    },
];

/// Main application state
#[derive(Debug)]
pub struct App {
    /// Application configuration
    pub config: Config,

    /// Agent storage
    pub storage: Storage,

    /// Currently selected agent index (in visible agents list)
    pub selected: usize,

    /// Current application mode
    pub mode: Mode,

    /// Currently active tab in the detail pane
    pub active_tab: Tab,

    /// Whether the application should quit
    pub should_quit: bool,

    /// Input state (buffer, cursor, scroll)
    pub input: InputState,

    /// UI state (scroll positions, preview content, dimensions)
    pub ui: UiState,

    /// Git operation state (push, rename, PR)
    pub git_op: GitOpState,

    /// Review state (branch selection)
    pub review: ReviewState,

    /// Slash command palette state (`/`)
    pub command_palette: CommandPaletteState,

    /// Model selector state (`/agents`)
    pub model_selector: ModelSelectorState,

    /// Spawn state (child agent spawning)
    pub spawn: SpawnState,

    /// User settings (persistent preferences)
    pub settings: Settings,

    /// Whether the terminal supports the keyboard enhancement protocol
    pub keyboard_enhancement_supported: bool,
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
            config,
            storage,
            selected: 0,
            mode: Mode::Normal,
            active_tab: Tab::Preview,
            should_quit: false,
            input: InputState::new(),
            ui: UiState::new(),
            git_op: GitOpState::new(),
            review: ReviewState::new(),
            command_palette: CommandPaletteState::new(),
            model_selector: ModelSelectorState::new(),
            spawn: SpawnState::new(),
            settings,
            keyboard_enhancement_supported,
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

/// Application mode/state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal operation mode
    #[default]
    Normal,
    /// Creating a new agent (typing name)
    Creating,
    /// Typing a prompt to send to agent
    Prompting,
    /// Confirming an action
    Confirming(ConfirmAction),
    /// Showing help overlay
    Help,
    /// Slash command palette (type `/...`)
    CommandPalette,
    /// Selecting which model/program to run for new agents
    ModelSelector,
    /// Scrolling through preview/diff
    Scrolling,
    /// Preview pane is focused - keystrokes are forwarded to the mux backend
    PreviewFocused,
    /// Selecting number of child agents to spawn
    ChildCount,
    /// Typing the task/prompt for child agents
    ChildPrompt,
    /// Typing a message to broadcast to agent and leaf descendants
    Broadcasting,
    /// Showing an error modal
    ErrorModal(String),
    /// Editing prompt after choosing to reconnect to existing worktree
    ReconnectPrompt,
    /// Showing info that an agent must be selected before review
    ReviewInfo,
    /// Selecting number of review agents
    ReviewChildCount,
    /// Selecting base branch for review
    BranchSelector,
    /// Confirming push to remote (Y/N)
    ConfirmPush,
    /// Renaming branch (input mode) - triggered by 'r' key
    RenameBranch,
    /// Confirming push before opening PR (Y/N) - triggered by Ctrl+o
    ConfirmPushForPR,
    /// Typing a startup command for a new terminal - triggered by 'T' key
    TerminalPrompt,
    /// Typing a custom command to run for new agents
    CustomAgentCommand,
    /// Selecting branch to rebase onto - triggered by Alt+r
    RebaseBranchSelector,
    /// Selecting branch to merge from - triggered by Alt+m
    MergeBranchSelector,
    /// Showing success modal after git operation
    SuccessModal(String),
    /// Prompting user to remap Ctrl+M due to terminal incompatibility
    KeyboardRemapPrompt,
    /// Prompting user to update Tenex to a newer version
    UpdatePrompt(UpdateInfo),
    /// User accepted update; exit TUI to install and restart
    UpdateRequested(UpdateInfo),
}

/// Actions that require confirmation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Kill an agent
    Kill,
    /// Reset all state
    Reset,
    /// Quit the application
    Quit,
    /// Synthesize children into parent
    Synthesize,
    /// Worktree already exists - ask to reconnect or recreate
    WorktreeConflict,
}

/// Information about an existing worktree that conflicts with a new agent
#[derive(Debug, Clone)]
pub struct WorktreeConflictInfo {
    /// The title the user entered for the new agent
    pub title: String,
    /// Optional prompt for the new agent
    pub prompt: Option<String>,
    /// The generated branch name
    pub branch: String,
    /// The path to the existing worktree
    pub worktree_path: std::path::PathBuf,
    /// The branch the existing worktree is based on (if available)
    pub existing_branch: Option<String>,
    /// The commit hash of the existing worktree's HEAD (short form)
    pub existing_commit: Option<String>,
    /// The current HEAD branch that would be used for a new worktree
    pub current_branch: String,
    /// The current HEAD commit hash (short form)
    pub current_commit: String,
    /// If this is a swarm creation, the number of children to spawn
    pub swarm_child_count: Option<usize>,
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
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview => write!(f, "Preview"),
            Self::Diff => write!(f, "Diff"),
        }
    }
}

#[cfg(test)]
mod tests;
