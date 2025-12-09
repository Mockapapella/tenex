//! Keybinding configuration - single source of truth for all hotkeys

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Actions that can be triggered by keybindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Create a new agent
    NewAgent,
    /// Create a new agent with a prompt
    NewAgentWithPrompt,
    /// Focus the preview pane (keystrokes forwarded to agent)
    FocusPreview,
    /// Unfocus the preview pane (return to agent list)
    UnfocusPreview,
    /// Kill selected agent
    Kill,
    /// Push branch to remote
    Push,
    /// Rename branch (local and remote)
    RenameBranch,
    /// Open pull request (push first if needed)
    OpenPR,
    /// Switch between preview/diff tabs
    SwitchTab,
    /// Select next agent
    NextAgent,
    /// Select previous agent
    PrevAgent,
    /// Show help
    Help,
    /// Quit application
    Quit,
    /// Scroll up in preview
    ScrollUp,
    /// Scroll down in preview
    ScrollDown,
    /// Scroll to top
    ScrollTop,
    /// Scroll to bottom
    ScrollBottom,
    /// Cancel current operation
    Cancel,
    /// Confirm current operation
    Confirm,
    /// Spawn new root with children (no pre-prompt)
    SpawnChildren,
    /// Plan: spawn new root with children (with planning pre-prompt)
    PlanSwarm,
    /// Add children to selected agent
    AddChildren,
    /// Synthesize children into parent
    Synthesize,
    /// Toggle expand/collapse of selected agent
    ToggleCollapse,
    /// Broadcast message to agent and all descendants
    Broadcast,
    /// Review changes against a base branch
    ReviewSwarm,
    /// Spawn a new terminal (not a Claude agent)
    SpawnTerminal,
    /// Spawn a new terminal with a startup command
    SpawnTerminalPrompted,
}

/// Categories for grouping actions in help display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    /// Agent creation and management actions
    Agents,
    /// Terminal (non-Claude shell) actions
    Terminals,
    /// Git operations (push, rename, PR)
    GitOps,
    /// Navigation and scrolling actions
    Navigation,
    /// Miscellaneous actions
    Other,
    /// Actions not shown in help (internal or context-specific)
    Hidden,
}

impl ActionGroup {
    /// Get the display title for this group
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Terminals => "Terminals",
            Self::GitOps => "Git Ops",
            Self::Navigation => "Navigation",
            Self::Other => "Other",
            Self::Hidden => "",
        }
    }
}

/// A single keybinding definition
#[derive(Debug, Clone, Copy)]
struct Binding {
    code: KeyCode,
    modifiers: KeyModifiers,
    action: Action,
}

/// All keybindings - single source of truth
const BINDINGS: &[Binding] = &[
    // Agents
    Binding {
        code: KeyCode::Char('a'),
        modifiers: KeyModifiers::NONE,
        action: Action::NewAgent,
    },
    Binding {
        code: KeyCode::Char('A'),
        modifiers: KeyModifiers::NONE,
        action: Action::NewAgentWithPrompt,
    },
    Binding {
        code: KeyCode::Char('A'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::NewAgentWithPrompt,
    },
    Binding {
        code: KeyCode::Enter,
        modifiers: KeyModifiers::NONE,
        action: Action::FocusPreview,
    },
    Binding {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::Quit,
    },
    Binding {
        code: KeyCode::Char('d'),
        modifiers: KeyModifiers::NONE,
        action: Action::Kill,
    },
    // Hierarchy
    Binding {
        code: KeyCode::Char('S'),
        modifiers: KeyModifiers::NONE,
        action: Action::SpawnChildren,
    },
    Binding {
        code: KeyCode::Char('S'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::SpawnChildren,
    },
    Binding {
        code: KeyCode::Char('P'),
        modifiers: KeyModifiers::NONE,
        action: Action::PlanSwarm,
    },
    Binding {
        code: KeyCode::Char('P'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::PlanSwarm,
    },
    Binding {
        code: KeyCode::Char('+'),
        modifiers: KeyModifiers::NONE,
        action: Action::AddChildren,
    },
    Binding {
        code: KeyCode::Char('+'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::AddChildren,
    },
    Binding {
        code: KeyCode::Char('s'),
        modifiers: KeyModifiers::NONE,
        action: Action::Synthesize,
    },
    Binding {
        code: KeyCode::Char(' '),
        modifiers: KeyModifiers::NONE,
        action: Action::ToggleCollapse,
    },
    Binding {
        code: KeyCode::Char('B'),
        modifiers: KeyModifiers::NONE,
        action: Action::Broadcast,
    },
    Binding {
        code: KeyCode::Char('B'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::Broadcast,
    },
    Binding {
        code: KeyCode::Char('R'),
        modifiers: KeyModifiers::NONE,
        action: Action::ReviewSwarm,
    },
    Binding {
        code: KeyCode::Char('R'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::ReviewSwarm,
    },
    // Terminals
    Binding {
        code: KeyCode::Char('t'),
        modifiers: KeyModifiers::NONE,
        action: Action::SpawnTerminal,
    },
    Binding {
        code: KeyCode::Char('T'),
        modifiers: KeyModifiers::NONE,
        action: Action::SpawnTerminalPrompted,
    },
    Binding {
        code: KeyCode::Char('T'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::SpawnTerminalPrompted,
    },
    // Navigation
    Binding {
        code: KeyCode::Char('j'),
        modifiers: KeyModifiers::NONE,
        action: Action::NextAgent,
    },
    Binding {
        code: KeyCode::Down,
        modifiers: KeyModifiers::NONE,
        action: Action::NextAgent,
    },
    Binding {
        code: KeyCode::Char('k'),
        modifiers: KeyModifiers::NONE,
        action: Action::PrevAgent,
    },
    Binding {
        code: KeyCode::Up,
        modifiers: KeyModifiers::NONE,
        action: Action::PrevAgent,
    },
    Binding {
        code: KeyCode::Tab,
        modifiers: KeyModifiers::NONE,
        action: Action::SwitchTab,
    },
    Binding {
        code: KeyCode::Char('u'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::ScrollUp,
    },
    Binding {
        code: KeyCode::Char('d'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::ScrollDown,
    },
    Binding {
        code: KeyCode::Char('g'),
        modifiers: KeyModifiers::NONE,
        action: Action::ScrollTop,
    },
    Binding {
        code: KeyCode::Char('G'),
        modifiers: KeyModifiers::NONE,
        action: Action::ScrollBottom,
    },
    Binding {
        code: KeyCode::Char('G'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::ScrollBottom,
    },
    // Other
    Binding {
        code: KeyCode::Char('?'),
        modifiers: KeyModifiers::NONE,
        action: Action::Help,
    },
    Binding {
        code: KeyCode::Char('?'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::Help,
    },
    // Git operations
    Binding {
        code: KeyCode::Char('p'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::Push,
    },
    Binding {
        code: KeyCode::Char('r'),
        modifiers: KeyModifiers::NONE,
        action: Action::RenameBranch,
    },
    Binding {
        code: KeyCode::Char('o'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::OpenPR,
    },
    // Hidden (not shown in help but still functional)
    Binding {
        code: KeyCode::Esc,
        modifiers: KeyModifiers::NONE,
        action: Action::Cancel,
    },
    Binding {
        code: KeyCode::Char('y'),
        modifiers: KeyModifiers::NONE,
        action: Action::Confirm,
    },
];

impl Action {
    /// Get the display description for this action (with mnemonic hints)
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::NewAgent => "[a]dd agent",
            Self::NewAgentWithPrompt => "[A]dd agent with prompt",
            Self::FocusPreview => "[Enter] focus preview",
            Self::UnfocusPreview => "[Ctrl+q]uit preview / app",
            Self::Kill => "[d]elete agent and sub-agents",
            Self::Push => "[Ctrl+p]ush branch to remote",
            Self::RenameBranch => "[r]ename branch",
            Self::OpenPR => "[Ctrl+o]pen pull request",
            Self::SwitchTab => "[Tab] switch preview/diff",
            Self::NextAgent => "[j] / [↓] next agent",
            Self::PrevAgent => "[k] / [↑] prev agent",
            Self::Help => "[?] help",
            Self::Quit => "[Ctrl+q]uit",
            Self::ScrollUp => "[Ctrl+u] scroll up",
            Self::ScrollDown => "[Ctrl+d] scroll down",
            Self::ScrollTop => "[g]o to top",
            Self::ScrollBottom => "[G]o to bottom",
            Self::Cancel => "Cancel",
            Self::Confirm => "Confirm",
            Self::SpawnChildren => "[S]pawn swarm",
            Self::PlanSwarm => "[P]lanning swarm",
            Self::AddChildren => "[+] add agents",
            Self::Synthesize => "[s]ynthesize sub-agent outputs",
            Self::ToggleCollapse => "[Space] collapse/expand",
            Self::Broadcast => "[B]roadcast to leaf sub-agents",
            Self::ReviewSwarm => "[R]eview swarm",
            Self::SpawnTerminal => "[t]erminal",
            Self::SpawnTerminalPrompted => "[T]erminal with command",
        }
    }

    /// Get the display keys for this action (for help display)
    #[must_use]
    pub const fn keys(self) -> &'static str {
        match self {
            Self::NewAgent => "a",
            Self::NewAgentWithPrompt => "A",
            Self::FocusPreview => "Enter",
            Self::Kill => "d",
            Self::SwitchTab => "Tab",
            Self::NextAgent => "j/↓",
            Self::PrevAgent => "k/↑",
            Self::Help => "?",
            // Both use Ctrl+q: UnfocusPreview when in preview, Quit otherwise
            Self::UnfocusPreview | Self::Quit => "Ctrl+q",
            Self::ScrollUp => "Ctrl+u",
            Self::ScrollDown => "Ctrl+d",
            Self::ScrollTop => "g",
            Self::ScrollBottom => "G",
            Self::Cancel => "Esc",
            Self::Confirm => "y",
            Self::SpawnChildren => "S",
            Self::PlanSwarm => "P",
            Self::AddChildren => "+",
            Self::Synthesize => "s",
            Self::ToggleCollapse => "Space",
            Self::Broadcast => "B",
            Self::ReviewSwarm => "R",
            Self::Push => "Ctrl+p",
            Self::RenameBranch => "r",
            Self::OpenPR => "Ctrl+o",
            Self::SpawnTerminal => "t",
            Self::SpawnTerminalPrompted => "T",
        }
    }

    /// Get the group this action belongs to
    #[must_use]
    pub const fn group(self) -> ActionGroup {
        match self {
            Self::NewAgent
            | Self::NewAgentWithPrompt
            | Self::Kill
            | Self::SpawnChildren
            | Self::PlanSwarm
            | Self::AddChildren
            | Self::Synthesize
            | Self::Broadcast
            | Self::ReviewSwarm => ActionGroup::Agents,
            Self::SpawnTerminal | Self::SpawnTerminalPrompted => ActionGroup::Terminals,
            Self::Push | Self::RenameBranch | Self::OpenPR => ActionGroup::GitOps,
            Self::FocusPreview
            | Self::UnfocusPreview
            | Self::ToggleCollapse
            | Self::NextAgent
            | Self::PrevAgent
            | Self::SwitchTab
            | Self::ScrollUp
            | Self::ScrollDown
            | Self::ScrollTop
            | Self::ScrollBottom => ActionGroup::Navigation,
            Self::Help | Self::Quit => ActionGroup::Other,
            Self::Cancel | Self::Confirm => ActionGroup::Hidden,
        }
    }

    /// All actions in display order for help
    pub const ALL_FOR_HELP: &'static [Self] = &[
        // Agents
        Self::NewAgent,
        Self::NewAgentWithPrompt,
        Self::Kill,
        Self::SpawnChildren,
        Self::PlanSwarm,
        Self::ReviewSwarm,
        Self::AddChildren,
        Self::Synthesize,
        Self::Broadcast,
        // Terminals
        Self::SpawnTerminal,
        Self::SpawnTerminalPrompted,
        // Git Ops
        Self::Push,
        Self::RenameBranch,
        Self::OpenPR,
        // Navigation
        Self::FocusPreview,
        Self::UnfocusPreview,
        Self::ToggleCollapse,
        Self::NextAgent,
        Self::PrevAgent,
        Self::SwitchTab,
        Self::ScrollUp,
        Self::ScrollDown,
        Self::ScrollTop,
        Self::ScrollBottom,
        // Other
        Self::Help,
    ];
}

/// Get the action for a key event
#[must_use]
pub fn get_action(code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    for binding in BINDINGS {
        if binding.code == code && binding.modifiers == modifiers {
            return Some(binding.action);
        }
    }
    None
}

/// Generate status bar hint text
#[must_use]
pub fn status_hints() -> String {
    "[?]help".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keybindings() {
        assert_eq!(
            get_action(KeyCode::Char('a'), KeyModifiers::NONE),
            Some(Action::NewAgent)
        );
        // Plain 'q' no longer quits - only Ctrl+q does
        assert_eq!(get_action(KeyCode::Char('q'), KeyModifiers::NONE), None);
        assert_eq!(
            get_action(KeyCode::Enter, KeyModifiers::NONE),
            Some(Action::FocusPreview)
        );
        // Ctrl+q maps to Quit (but exits preview focus when in PreviewFocused mode)
        assert_eq!(
            get_action(KeyCode::Char('q'), KeyModifiers::CONTROL),
            Some(Action::Quit)
        );
    }

    #[test]
    fn test_modifier_keys() {
        assert_eq!(
            get_action(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Some(Action::ScrollUp)
        );
        assert_eq!(
            get_action(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Action::ScrollDown)
        );
        assert_eq!(
            get_action(KeyCode::Char('p'), KeyModifiers::CONTROL),
            Some(Action::Push)
        );
        assert_eq!(
            get_action(KeyCode::Char('r'), KeyModifiers::NONE),
            Some(Action::RenameBranch)
        );
        assert_eq!(
            get_action(KeyCode::Char('o'), KeyModifiers::CONTROL),
            Some(Action::OpenPR)
        );
    }

    #[test]
    fn test_uppercase_keybindings() {
        // Uppercase 'S' should trigger SpawnChildren
        assert_eq!(
            get_action(KeyCode::Char('S'), KeyModifiers::SHIFT),
            Some(Action::SpawnChildren)
        );
        // Also works without SHIFT modifier (some terminals don't send it)
        assert_eq!(
            get_action(KeyCode::Char('S'), KeyModifiers::NONE),
            Some(Action::SpawnChildren)
        );

        // Lowercase 's' should trigger Synthesize
        assert_eq!(
            get_action(KeyCode::Char('s'), KeyModifiers::NONE),
            Some(Action::Synthesize)
        );

        // Uppercase 'A' should trigger NewAgentWithPrompt
        assert_eq!(
            get_action(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Some(Action::NewAgentWithPrompt)
        );

        // Uppercase 'G' should trigger ScrollBottom
        assert_eq!(
            get_action(KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(Action::ScrollBottom)
        );
    }

    #[test]
    fn test_unknown_key() {
        assert_eq!(get_action(KeyCode::Char('x'), KeyModifiers::NONE), None);
    }

    #[test]
    fn test_action_keys() {
        assert_eq!(Action::NewAgent.keys(), "a");
        assert_eq!(Action::SpawnChildren.keys(), "S");
        assert_eq!(Action::NextAgent.keys(), "j/↓");
    }

    #[test]
    fn test_status_hints() {
        let hints = status_hints();
        assert_eq!(hints, "[?]help");
    }

    #[test]
    fn test_action_description() {
        assert_eq!(Action::NewAgent.description(), "[a]dd agent");
        assert_eq!(Action::SpawnChildren.description(), "[S]pawn swarm");
        assert_eq!(Action::PlanSwarm.description(), "[P]lanning swarm");
    }
}
