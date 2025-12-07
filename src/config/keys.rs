//! Keybinding configuration - single source of truth for all hotkeys

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Actions that can be triggered by keybindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Create a new agent
    NewAgent,
    /// Create a new agent with a prompt
    NewAgentWithPrompt,
    /// Attach to selected agent
    Attach,
    /// Kill selected agent
    Kill,
    /// Push branch to remote
    Push,
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
    /// Spawn new root with children
    SpawnChildren,
    /// Add children to selected agent
    AddChildren,
    /// Synthesize children into parent
    Synthesize,
    /// Toggle expand/collapse of selected agent
    ToggleCollapse,
    /// Broadcast message to agent and all descendants
    Broadcast,
}

/// Categories for grouping actions in help display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionGroup {
    /// Agent creation and management actions
    Agents,
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
        action: Action::Attach,
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
    Binding {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::NONE,
        action: Action::Quit,
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
            Self::Attach => "[Enter] into agent",
            Self::Kill => "[d]elete agent (and descendants)",
            Self::Push => "Push branch to remote",
            Self::SwitchTab => "[Tab] switch preview/diff",
            Self::NextAgent => "[j] / [↓] next agent",
            Self::PrevAgent => "[k] / [↑] prev agent",
            Self::Help => "[?] help",
            Self::Quit => "[q]uit",
            Self::ScrollUp => "[Ctrl+u] scroll up",
            Self::ScrollDown => "[Ctrl+d] scroll down",
            Self::ScrollTop => "[g]o to top",
            Self::ScrollBottom => "[G]o to bottom",
            Self::Cancel => "Cancel",
            Self::Confirm => "Confirm",
            Self::SpawnChildren => "[S]warm (new root)",
            Self::AddChildren => "[+] add children",
            Self::Synthesize => "[s]ynthesize children",
            Self::ToggleCollapse => "[Space] collapse/expand",
            Self::Broadcast => "[B]roadcast to descendants",
        }
    }

    /// Get the display keys for this action (for help display)
    #[must_use]
    pub const fn keys(self) -> &'static str {
        match self {
            Self::NewAgent => "a",
            Self::NewAgentWithPrompt => "A",
            Self::Attach => "Enter",
            Self::Kill => "d",
            Self::SwitchTab => "Tab",
            Self::NextAgent => "j/↓",
            Self::PrevAgent => "k/↑",
            Self::Help => "?",
            Self::Quit => "q",
            Self::ScrollUp => "Ctrl+u",
            Self::ScrollDown => "Ctrl+d",
            Self::ScrollTop => "g",
            Self::ScrollBottom => "G",
            Self::Cancel => "Esc",
            Self::Confirm => "y",
            Self::SpawnChildren => "S",
            Self::AddChildren => "+",
            Self::Synthesize => "s",
            Self::ToggleCollapse => "Space",
            Self::Broadcast => "B",
            Self::Push => "",
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
            | Self::AddChildren
            | Self::Synthesize
            | Self::Broadcast => ActionGroup::Agents,
            Self::Attach
            | Self::ToggleCollapse
            | Self::NextAgent
            | Self::PrevAgent
            | Self::SwitchTab
            | Self::ScrollUp
            | Self::ScrollDown
            | Self::ScrollTop
            | Self::ScrollBottom => ActionGroup::Navigation,
            Self::Help | Self::Quit => ActionGroup::Other,
            Self::Push | Self::Cancel | Self::Confirm => ActionGroup::Hidden,
        }
    }

    /// All actions in display order for help
    pub const ALL_FOR_HELP: &'static [Self] = &[
        // Agents
        Self::NewAgent,
        Self::NewAgentWithPrompt,
        Self::Kill,
        Self::SpawnChildren,
        Self::AddChildren,
        Self::Synthesize,
        Self::Broadcast,
        // Navigation
        Self::Attach,
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
        Self::Quit,
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
    let hints = [
        (Action::NewAgent, "add"),
        (Action::Kill, "del"),
        (Action::SwitchTab, "switch"),
        (Action::Help, "help"),
        (Action::Quit, "quit"),
    ];

    hints
        .iter()
        .map(|(action, label)| {
            let key = action.keys().split('/').next().unwrap_or("");
            format!("[{key}]{label}")
        })
        .collect::<Vec<_>>()
        .join(" ")
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
        assert_eq!(
            get_action(KeyCode::Char('q'), KeyModifiers::NONE),
            Some(Action::Quit)
        );
        assert_eq!(
            get_action(KeyCode::Enter, KeyModifiers::NONE),
            Some(Action::Attach)
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
        assert!(hints.contains("[a]add"));
        assert!(hints.contains("[d]del"));
        assert!(hints.contains("[?]help"));
        assert!(hints.contains("[q]quit"));
    }

    #[test]
    fn test_action_description() {
        assert_eq!(Action::NewAgent.description(), "[a]dd agent");
        assert_eq!(Action::SpawnChildren.description(), "[S]warm (new root)");
    }
}
