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
    /// Scrolling through preview/diff
    Scrolling,
    /// Preview pane is focused - keystrokes are forwarded to the mux backend
    PreviewFocused,
    /// A modal/overlay is open.
    Overlay(OverlayMode),
    /// User accepted update; exit TUI to install and restart
    UpdateRequested(UpdateInfo),
}

/// Modal/overlay modes that temporarily take over input and rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayMode {
    /// Showing help overlay.
    Help,
    /// Slash command palette (type `/...`).
    CommandPalette,
    /// Selecting which model/program to run for new agents.
    ModelSelector,
    /// Text input overlays backed by the shared input buffer.
    TextInput(TextInputKind),
    /// Count picker overlays (child/review counts).
    CountPicker(CountPickerKind),
    /// Branch picker overlays (review base branch, rebase target, merge source).
    BranchPicker(BranchPickerKind),
    /// Confirmation overlays (yes/no, worktree conflict, update prompt).
    Confirm(ConfirmKind),
    /// Showing info that an agent must be selected before review.
    ReviewInfo,
    /// Showing an error modal.
    Error(String),
    /// Showing success modal after git operation.
    Success(String),
}

/// A specific "text input" overlay backed by the shared input buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputKind {
    /// Creating a new agent (typing name).
    Creating,
    /// Typing a prompt to send to agent.
    Prompting,
    /// Typing the task/prompt for child agents.
    ChildPrompt,
    /// Typing a message to broadcast to agent and leaf descendants.
    Broadcasting,
    /// Editing prompt after choosing to reconnect to existing worktree.
    ReconnectPrompt,
    /// Typing a startup command for a new terminal.
    TerminalPrompt,
    /// Typing a custom command to run for new agents.
    CustomAgentCommand,
    /// Renaming branch (input mode) - triggered by 'r' key.
    RenameBranch,
}

/// A specific count picker overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountPickerKind {
    /// Selecting number of child agents to spawn.
    ChildCount,
    /// Selecting number of review agents.
    ReviewChildCount,
}

/// A specific branch picker overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPickerKind {
    /// Selecting base branch for review.
    ReviewBaseBranch,
    /// Selecting branch to rebase onto - triggered by Alt+r.
    RebaseTargetBranch,
    /// Selecting branch to merge from - triggered by Alt+m.
    MergeFromBranch,
}

/// A specific confirmation overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Confirming an action (Kill/Reset/Quit/Synthesize/WorktreeConflict).
    Action(ConfirmAction),
    /// Confirming push to remote (Y/N).
    Push,
    /// Confirming push before opening PR (Y/N) - triggered by Ctrl+o.
    PushForPR,
    /// Prompting user to remap Ctrl+M due to terminal incompatibility.
    KeyboardRemap,
    /// Prompting user to update Tenex to a newer version.
    UpdatePrompt(UpdateInfo),
}

impl Mode {
    /// Returns true if entering this mode should clear the shared input buffer.
    ///
    /// This encodes the behavior previously duplicated in `App::enter_mode`.
    #[must_use]
    pub const fn clears_input_on_enter(&self) -> bool {
        match self {
            Self::Overlay(overlay) => overlay.clears_input_on_enter(),
            Self::Normal | Self::Scrolling | Self::PreviewFocused | Self::UpdateRequested(_) => {
                false
            }
        }
    }

    /// Returns true if this mode uses the shared text input buffer.
    ///
    /// This consolidates checks used by text-editing helpers like `handle_char`.
    #[must_use]
    pub const fn uses_text_input_buffer(&self) -> bool {
        match self {
            Self::Overlay(overlay) => overlay.uses_text_input_buffer(),
            Self::Normal | Self::Scrolling | Self::PreviewFocused | Self::UpdateRequested(_) => {
                false
            }
        }
    }

    /// Returns true if this mode is handled by the "simple" text input handler and
    /// rendered via the generic input overlay.
    #[must_use]
    pub const fn is_text_input_overlay(&self) -> bool {
        matches!(
            self,
            Self::Overlay(OverlayMode::TextInput(kind)) if kind.uses_generic_input_overlay()
        )
    }

    /// Returns true if submitting an empty string is meaningful for this text input mode.
    #[must_use]
    pub const fn text_input_allows_empty_submit(&self) -> bool {
        matches!(
            self,
            Self::Overlay(OverlayMode::TextInput(kind)) if kind.allows_empty_submit()
        )
    }

    /// Returns the title and prompt for modes rendered via the generic input overlay.
    #[must_use]
    pub fn input_overlay_spec(&self, app: &App) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Overlay(OverlayMode::TextInput(kind)) => kind.input_overlay_spec(app),
            _ => None,
        }
    }
}

impl OverlayMode {
    /// Returns true if entering this overlay should clear the shared input buffer.
    #[must_use]
    pub const fn clears_input_on_enter(&self) -> bool {
        match self {
            Self::TextInput(kind) => kind.clears_input_on_enter(),
            Self::CommandPalette | Self::Confirm(ConfirmKind::Action(_)) => true,
            Self::Help
            | Self::ModelSelector
            | Self::CountPicker(_)
            | Self::BranchPicker(_)
            | Self::Confirm(_)
            | Self::ReviewInfo
            | Self::Error(_)
            | Self::Success(_) => false,
        }
    }

    /// Returns true if this overlay uses the shared text input buffer.
    #[must_use]
    pub const fn uses_text_input_buffer(&self) -> bool {
        match self {
            Self::TextInput(_) | Self::CommandPalette | Self::Confirm(ConfirmKind::Action(_)) => {
                true
            }
            Self::Help
            | Self::ModelSelector
            | Self::CountPicker(_)
            | Self::BranchPicker(_)
            | Self::Confirm(_)
            | Self::ReviewInfo
            | Self::Error(_)
            | Self::Success(_) => false,
        }
    }
}

impl TextInputKind {
    /// Returns true if entering this text input overlay should clear the shared input buffer.
    #[must_use]
    pub const fn clears_input_on_enter(self) -> bool {
        matches!(
            self,
            Self::Creating
                | Self::Prompting
                | Self::ChildPrompt
                | Self::Broadcasting
                | Self::TerminalPrompt
                | Self::CustomAgentCommand
        )
    }

    /// Returns true if this kind is rendered via the generic input overlay.
    #[must_use]
    pub const fn uses_generic_input_overlay(self) -> bool {
        !matches!(self, Self::RenameBranch)
    }

    /// Returns true if submitting an empty string is meaningful for this text input kind.
    #[must_use]
    pub const fn allows_empty_submit(self) -> bool {
        matches!(
            self,
            Self::ReconnectPrompt | Self::Prompting | Self::ChildPrompt | Self::TerminalPrompt
        )
    }

    /// Returns the title and prompt for kinds rendered via the generic input overlay.
    #[must_use]
    pub fn input_overlay_spec(self, app: &App) -> Option<(&'static str, &'static str)> {
        if !self.uses_generic_input_overlay() {
            return None;
        }

        let title = match self {
            Self::Creating => "New Agent",
            Self::Prompting => "New Agent with Prompt",
            Self::ChildPrompt => "Spawn Children",
            Self::Broadcasting => "Broadcast Message",
            Self::ReconnectPrompt => {
                app.spawn
                    .worktree_conflict
                    .as_ref()
                    .map_or("Reconnect", |c| {
                        if c.swarm_child_count.is_some() {
                            "Reconnect Swarm"
                        } else {
                            "Reconnect Agent"
                        }
                    })
            }
            Self::TerminalPrompt => "New Terminal",
            Self::CustomAgentCommand => "Custom Agent Command",
            Self::RenameBranch => {
                return None;
            }
        };

        let prompt = match self {
            Self::Creating => "Enter agent name:",
            Self::Prompting => "Enter prompt:",
            Self::ChildPrompt => "Enter task for children:",
            Self::Broadcasting => "Enter message to broadcast to leaf agents:",
            Self::ReconnectPrompt => "Edit prompt (or leave empty):",
            Self::TerminalPrompt => "Enter startup command (or leave empty):",
            Self::CustomAgentCommand => "Enter the command to run for new agents:",
            Self::RenameBranch => {
                return None;
            }
        };

        Some((title, prompt))
    }
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
