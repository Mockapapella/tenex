//! Keybinding configuration - single source of truth for all hotkeys

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Actions that can be triggered by keybindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Create a new agent
    NewAgent,
    /// Create a new agent with a prompt
    NewAgentWithPrompt,
    /// Focus the active detail pane (Preview attaches terminal, Diff enters diff focus)
    FocusPreview,
    /// Detach from the agent terminal (return to Tenex controls)
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
    /// Move the diff cursor up (Diff tab)
    DiffCursorUp,
    /// Move the diff cursor down (Diff tab)
    DiffCursorDown,
    /// Delete the selected diff line (Diff tab)
    DiffDeleteLine,
    /// Delete the selected diff hunk (Diff tab)
    DiffDeleteHunk,
    /// Undo the last diff edit (Diff tab)
    DiffUndo,
    /// Redo the last undone diff edit (Diff tab)
    DiffRedo,
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
    /// Plan: spawn planners under selected agent (with planning pre-prompt)
    PlanSwarm,
    /// Add children to selected agent
    AddChildren,
    /// Synthesize children into parent
    Synthesize,
    /// Toggle expand/collapse of selected agent
    ToggleCollapse,
    /// Broadcast message to agent and all descendants
    Broadcast,
    /// Review: spawn reviewers under selected agent against a base branch
    ReviewSwarm,
    /// Spawn a new terminal (not a Claude agent)
    SpawnTerminal,
    /// Spawn a new terminal with a startup command
    SpawnTerminalPrompted,
    /// Rebase current branch onto selected branch
    Rebase,
    /// Merge selected branch into current branch
    Merge,
    /// Open slash command palette
    CommandPalette,
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
        code: KeyCode::Down,
        modifiers: KeyModifiers::NONE,
        action: Action::NextAgent,
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
    // Diff (interactive)
    Binding {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::NONE,
        action: Action::DiffDeleteLine,
    },
    Binding {
        code: KeyCode::Char('X'),
        modifiers: KeyModifiers::NONE,
        action: Action::DiffDeleteHunk,
    },
    Binding {
        code: KeyCode::Char('z'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::DiffUndo,
    },
    Binding {
        code: KeyCode::Char('y'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::DiffRedo,
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
        code: KeyCode::Char('/'),
        modifiers: KeyModifiers::NONE,
        action: Action::CommandPalette,
    },
    Binding {
        code: KeyCode::Char('/'),
        modifiers: KeyModifiers::SHIFT,
        action: Action::CommandPalette,
    },
    // Git operations (all use Ctrl modifier, requires Kitty keyboard protocol)
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
    Binding {
        code: KeyCode::Char('r'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::Rebase,
    },
    Binding {
        code: KeyCode::Char('m'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::Merge,
    },
    // Alternative binding for Merge when terminal doesn't support Ctrl+M
    Binding {
        code: KeyCode::Char('n'),
        modifiers: KeyModifiers::CONTROL,
        action: Action::Merge,
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
            Self::FocusPreview => "[Enter] focus preview (Preview tab) / diff (Diff tab)",
            Self::UnfocusPreview => "[Ctrl+q] detach terminal / quit app",
            Self::Kill => "[d]elete agent and sub-agents",
            Self::Push => "[Ctrl+p]ush branch to remote",
            Self::RenameBranch => "[r]ename branch",
            Self::OpenPR => "[Ctrl+o]pen pull request",
            Self::SwitchTab => "[Tab] switch preview/diff",
            Self::DiffCursorUp => "[↑] diff cursor up (diff focus)",
            Self::DiffCursorDown => "[↓] diff cursor down (diff focus)",
            Self::DiffDeleteLine => "[x] delete diff line",
            Self::DiffDeleteHunk => "[X] delete diff hunk",
            Self::DiffUndo => "[Ctrl+z] undo diff edit",
            Self::DiffRedo => "[Ctrl+Shift+z] redo diff edit (Ctrl+y fallback)",
            Self::NextAgent => "[↓] next agent",
            Self::PrevAgent => "[↑] prev agent",
            Self::Help => "[?] help",
            Self::Quit => "[Ctrl+q]uit",
            Self::ScrollUp => "[Ctrl+u] scroll preview/diff up",
            Self::ScrollDown => "[Ctrl+d] scroll preview/diff down",
            Self::ScrollTop => "[g]o to top",
            Self::ScrollBottom => "[G]o to bottom",
            Self::Cancel => "Cancel",
            Self::Confirm => "Confirm",
            Self::SpawnChildren => "[S]pawn swarm",
            Self::PlanSwarm => "[P] spawn planners for selected agent",
            Self::AddChildren => "[+] spawn sub-agents for selected agent",
            Self::Synthesize => "[s]ynthesize sub-agent outputs",
            Self::ToggleCollapse => "[Space] collapse/expand",
            Self::Broadcast => "[B]roadcast to leaf sub-agents",
            Self::ReviewSwarm => "[R] spawn reviewers for selected agent",
            Self::SpawnTerminal => "[t]erminal",
            Self::SpawnTerminalPrompted => "[T]erminal with command",
            Self::Rebase => "[Ctrl+r]ebase onto branch",
            Self::Merge => "[Ctrl+m]erge branch",
            Self::CommandPalette => "[/] commands",
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
            Self::DiffCursorUp => "↑ (diff focus)",
            Self::DiffCursorDown => "↓ (diff focus)",
            Self::DiffDeleteLine => "x",
            Self::DiffDeleteHunk => "X",
            Self::DiffUndo => "Ctrl+z",
            Self::DiffRedo => "Ctrl+Shift+z",
            Self::NextAgent => "↓",
            Self::PrevAgent => "↑",
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
            Self::Rebase => "Ctrl+r",
            Self::Merge => "Ctrl+m",
            Self::CommandPalette => "/",
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
            Self::Push | Self::RenameBranch | Self::OpenPR | Self::Rebase | Self::Merge => {
                ActionGroup::GitOps
            }
            Self::FocusPreview
            | Self::UnfocusPreview
            | Self::ToggleCollapse
            | Self::NextAgent
            | Self::PrevAgent
            | Self::SwitchTab
            | Self::DiffCursorUp
            | Self::DiffCursorDown
            | Self::DiffDeleteLine
            | Self::DiffDeleteHunk
            | Self::DiffUndo
            | Self::DiffRedo
            | Self::ScrollUp
            | Self::ScrollDown
            | Self::ScrollTop
            | Self::ScrollBottom => ActionGroup::Navigation,
            Self::Help | Self::Quit | Self::CommandPalette => ActionGroup::Other,
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
        Self::Rebase,
        Self::Merge,
        // Navigation
        Self::FocusPreview,
        Self::UnfocusPreview,
        Self::ToggleCollapse,
        Self::NextAgent,
        Self::PrevAgent,
        Self::SwitchTab,
        Self::DiffCursorUp,
        Self::DiffCursorDown,
        Self::DiffDeleteLine,
        Self::DiffDeleteHunk,
        Self::DiffUndo,
        Self::DiffRedo,
        Self::ScrollUp,
        Self::ScrollDown,
        Self::ScrollTop,
        Self::ScrollBottom,
        // Other
        Self::Help,
        Self::CommandPalette,
    ];
}

/// Get the action for a key event
#[must_use]
pub fn get_action(code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    // Special-case: allow Ctrl+Shift+z for redo on terminals that distinguish it.
    if matches!(code, KeyCode::Char('z' | 'Z'))
        && modifiers.contains(KeyModifiers::CONTROL)
        && modifiers.contains(KeyModifiers::SHIFT)
    {
        return Some(Action::DiffRedo);
    }

    let (code, modifiers) = normalize_key_event(code, modifiers);

    for binding in BINDINGS {
        let (binding_code, binding_modifiers) =
            normalize_key_event(binding.code, binding.modifiers);
        if binding_code == code && binding_modifiers == modifiers {
            return Some(binding.action);
        }
    }
    None
}

fn normalize_key_event(code: KeyCode, modifiers: KeyModifiers) -> (KeyCode, KeyModifiers) {
    let KeyCode::Char(c) = code else {
        return (code, modifiers);
    };

    // For `KeyCode::Char`, terminals differ:
    // - Some report the shifted character directly (e.g. 'G') and may redundantly set SHIFT.
    // - With Kitty keyboard protocol, some report the unshifted character (e.g. 'g') with SHIFT.
    // Normalize so bindings work across both representations.
    let mut normalized_char = c;
    if modifiers.contains(KeyModifiers::SHIFT) && normalized_char.is_ascii_lowercase() {
        normalized_char = normalized_char.to_ascii_uppercase();
    }

    // The SHIFT modifier is redundant for char keys after normalization.
    let mut normalized_modifiers = modifiers;
    normalized_modifiers.remove(KeyModifiers::SHIFT);

    // Ctrl+<letter> is case-insensitive. Normalize so bindings can be defined in one place.
    if normalized_modifiers.contains(KeyModifiers::CONTROL) {
        (
            KeyCode::Char(normalized_char.to_ascii_lowercase()),
            normalized_modifiers,
        )
    } else {
        (KeyCode::Char(normalized_char), normalized_modifiers)
    }
}

/// Get the display keys for an action, considering keyboard remap settings
/// Returns Ctrl+n instead of Ctrl+m for Merge when remapped
#[must_use]
pub fn get_display_keys(action: Action, merge_key_remapped: bool) -> &'static str {
    if action == Action::Merge && merge_key_remapped {
        "Ctrl+n"
    } else {
        action.keys()
    }
}

/// Get the description for an action, considering keyboard remap settings
/// Returns updated description for Merge when remapped
#[must_use]
pub fn get_display_description(action: Action, merge_key_remapped: bool) -> &'static str {
    if action == Action::Merge && merge_key_remapped {
        "[Ctrl+n] merge branch"
    } else {
        action.description()
    }
}

/// Generate status bar hint text
#[must_use]
pub fn status_hints() -> String {
    "[?]help  [/]commands".to_string()
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
        // Some terminals report Ctrl+<char> as uppercase or with redundant SHIFT.
        assert_eq!(
            get_action(KeyCode::Char('U'), KeyModifiers::CONTROL),
            Some(Action::ScrollUp)
        );
        assert_eq!(
            get_action(
                KeyCode::Char('u'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            Some(Action::ScrollUp)
        );
        assert_eq!(
            get_action(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Action::ScrollDown)
        );
        assert_eq!(
            get_action(KeyCode::Char('D'), KeyModifiers::CONTROL),
            Some(Action::ScrollDown)
        );
        assert_eq!(
            get_action(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
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
        assert_eq!(
            get_action(KeyCode::Char('r'), KeyModifiers::CONTROL),
            Some(Action::Rebase)
        );
        assert_eq!(
            get_action(KeyCode::Char('m'), KeyModifiers::CONTROL),
            Some(Action::Merge)
        );
        assert_eq!(
            get_action(KeyCode::Char('M'), KeyModifiers::CONTROL),
            Some(Action::Merge)
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

        // Some terminals (notably with Kitty keyboard protocol) report lowercase + SHIFT.
        assert_eq!(
            get_action(KeyCode::Char('g'), KeyModifiers::SHIFT),
            Some(Action::ScrollBottom)
        );
    }

    #[test]
    fn test_unknown_key() {
        assert_eq!(get_action(KeyCode::Char('v'), KeyModifiers::NONE), None);
    }

    #[test]
    fn test_action_keys() {
        assert_eq!(Action::NewAgent.keys(), "a");
        assert_eq!(Action::SpawnChildren.keys(), "S");
        assert_eq!(Action::NextAgent.keys(), "↓");
    }

    #[test]
    fn test_status_hints() {
        let hints = status_hints();
        assert_eq!(hints, "[?]help  [/]commands");
    }

    #[test]
    fn test_action_description() {
        assert_eq!(Action::NewAgent.description(), "[a]dd agent");
        assert_eq!(Action::SpawnChildren.description(), "[S]pawn swarm");
        assert_eq!(
            Action::PlanSwarm.description(),
            "[P] spawn planners for selected agent"
        );
    }
}
