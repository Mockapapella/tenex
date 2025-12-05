//! Keybinding configuration

use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Actions that can be triggered by keybindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    /// Pause agent (checkout)
    Pause,
    /// Resume paused agent
    Resume,
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
    /// Hierarchical agent workflow actions
    Hierarchy,
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
            Self::Hierarchy => "Hierarchy",
            Self::Navigation => "Navigation",
            Self::Other => "Other",
            Self::Hidden => "",
        }
    }
}

impl Action {
    /// Get the display description for this action
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::NewAgent => "New agent",
            Self::NewAgentWithPrompt => "New agent with prompt",
            Self::Attach => "Attach to agent",
            Self::Kill => "Kill agent (and descendants)",
            Self::Push => "Push branch to remote",
            Self::Pause => "Pause agent",
            Self::Resume => "Resume agent",
            Self::SwitchTab => "Switch preview/diff",
            Self::NextAgent => "Select next",
            Self::PrevAgent => "Select previous",
            Self::Help => "Show this help",
            Self::Quit => "Quit",
            Self::ScrollUp => "Scroll up",
            Self::ScrollDown => "Scroll down",
            Self::ScrollTop => "Scroll to top",
            Self::ScrollBottom => "Scroll to bottom",
            Self::Cancel => "Cancel",
            Self::Confirm => "Confirm",
            Self::SpawnChildren => "Spawn children (new root)",
            Self::AddChildren => "Add children to selected",
            Self::Synthesize => "Synthesize children",
            Self::ToggleCollapse => "Toggle collapse/expand",
            Self::Broadcast => "Broadcast to descendants",
        }
    }

    /// Get the group this action belongs to
    #[must_use]
    pub const fn group(self) -> ActionGroup {
        match self {
            Self::NewAgent | Self::NewAgentWithPrompt | Self::Attach | Self::Kill => {
                ActionGroup::Agents
            }
            Self::SpawnChildren
            | Self::AddChildren
            | Self::Synthesize
            | Self::ToggleCollapse
            | Self::Broadcast => ActionGroup::Hierarchy,
            Self::NextAgent
            | Self::PrevAgent
            | Self::SwitchTab
            | Self::ScrollUp
            | Self::ScrollDown
            | Self::ScrollTop
            | Self::ScrollBottom => ActionGroup::Navigation,
            Self::Help | Self::Quit => ActionGroup::Other,
            Self::Push | Self::Pause | Self::Resume | Self::Cancel | Self::Confirm => {
                ActionGroup::Hidden
            }
        }
    }

    /// All actions in display order for help
    pub const ALL_FOR_HELP: &'static [Self] = &[
        // Agents
        Self::NewAgent,
        Self::NewAgentWithPrompt,
        Self::Attach,
        Self::Kill,
        // Hierarchy
        Self::SpawnChildren,
        Self::AddChildren,
        Self::Synthesize,
        Self::ToggleCollapse,
        Self::Broadcast,
        // Navigation
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

/// Keybinding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    /// Map of key strings to actions (for serialization)
    bindings: HashMap<String, Action>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        let mut bindings = HashMap::new();

        bindings.insert("a".to_string(), Action::NewAgent);
        bindings.insert("A".to_string(), Action::NewAgentWithPrompt);
        bindings.insert("Enter".to_string(), Action::Attach);
        bindings.insert("o".to_string(), Action::Attach);
        bindings.insert("d".to_string(), Action::Kill);
        bindings.insert("Tab".to_string(), Action::SwitchTab);
        bindings.insert("j".to_string(), Action::NextAgent);
        bindings.insert("Down".to_string(), Action::NextAgent);
        bindings.insert("k".to_string(), Action::PrevAgent);
        bindings.insert("Up".to_string(), Action::PrevAgent);
        bindings.insert("?".to_string(), Action::Help);
        bindings.insert("q".to_string(), Action::Quit);
        bindings.insert("Ctrl+u".to_string(), Action::ScrollUp);
        bindings.insert("Ctrl+d".to_string(), Action::ScrollDown);
        bindings.insert("g".to_string(), Action::ScrollTop);
        bindings.insert("G".to_string(), Action::ScrollBottom);
        bindings.insert("Esc".to_string(), Action::Cancel);
        bindings.insert("y".to_string(), Action::Confirm);
        // Hierarchy keybindings
        bindings.insert("S".to_string(), Action::SpawnChildren);
        bindings.insert("+".to_string(), Action::AddChildren);
        bindings.insert("s".to_string(), Action::Synthesize);
        bindings.insert(" ".to_string(), Action::ToggleCollapse);
        bindings.insert("B".to_string(), Action::Broadcast);

        Self { bindings }
    }
}

impl KeyBindings {
    /// Merge in any missing default keybindings
    ///
    /// This ensures that new keybindings added in updates are available
    /// even if the user has an older saved config.
    pub fn merge_defaults(&mut self) {
        let defaults = Self::default();
        for (key, action) in defaults.bindings {
            self.bindings.entry(key).or_insert(action);
        }
    }

    /// Get the action for a key event
    #[must_use]
    pub fn get_action(&self, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        let key_str = key_to_string(code, modifiers);
        self.bindings.get(&key_str).copied()
    }

    /// Set a keybinding
    pub fn set(&mut self, key: &str, action: Action) {
        self.bindings.insert(key.to_string(), action);
    }

    /// Get all bindings for an action
    #[must_use]
    pub fn keys_for_action(&self, action: Action) -> Vec<String> {
        self.bindings
            .iter()
            .filter_map(|(k, &v)| if v == action { Some(k.clone()) } else { None })
            .collect()
    }

    /// Format key(s) for an action for display (e.g., "Enter/o" or "j/Down")
    #[must_use]
    pub fn format_keys(&self, action: Action) -> String {
        let mut keys = self.keys_for_action(action);
        // Sort to ensure consistent display order (prefer shorter/simpler keys first)
        keys.sort_by(|a, b| {
            // Prefer single chars over multi-char keys
            let a_simple = a.len() == 1 || a == "Space";
            let b_simple = b.len() == 1 || b == "Space";
            match (a_simple, b_simple) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });
        // Replace space with readable name
        keys.iter()
            .map(|k| {
                if k == " " {
                    "Space".to_string()
                } else {
                    k.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Generate a formatted help line for an action: "  keys    description"
    #[must_use]
    pub fn help_line(&self, action: Action) -> String {
        let keys = self.format_keys(action);
        format!("  {keys:<10} {}", action.description())
    }

    /// Generate status bar hint text
    #[must_use]
    pub fn status_hints(&self) -> String {
        // Show key hints for common actions
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
                let key = self
                    .keys_for_action(*action)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                format!("[{key}]{label}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Convert a key code and modifiers to a string representation
#[must_use]
pub fn key_to_string(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mut parts = Vec::new();

    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) && !matches!(code, KeyCode::Char(_)) {
        parts.push("Shift".to_string());
    }

    let key_part = match code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return String::new(),
    };

    parts.push(key_part);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_keybindings() {
        let keys = KeyBindings::default();

        assert_eq!(
            keys.get_action(KeyCode::Char('a'), KeyModifiers::NONE),
            Some(Action::NewAgent)
        );
        assert_eq!(
            keys.get_action(KeyCode::Char('q'), KeyModifiers::NONE),
            Some(Action::Quit)
        );
        assert_eq!(
            keys.get_action(KeyCode::Enter, KeyModifiers::NONE),
            Some(Action::Attach)
        );
    }

    #[test]
    fn test_modifier_keys() {
        let keys = KeyBindings::default();

        assert_eq!(
            keys.get_action(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Some(Action::ScrollUp)
        );
        assert_eq!(
            keys.get_action(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(Action::ScrollDown)
        );
    }

    #[test]
    fn test_uppercase_keybindings() {
        let keys = KeyBindings::default();

        // Uppercase 'S' should trigger SpawnChildren
        assert_eq!(
            keys.get_action(KeyCode::Char('S'), KeyModifiers::SHIFT),
            Some(Action::SpawnChildren)
        );
        // Also works without SHIFT modifier (some terminals don't send it)
        assert_eq!(
            keys.get_action(KeyCode::Char('S'), KeyModifiers::NONE),
            Some(Action::SpawnChildren)
        );

        // Lowercase 's' should trigger Synthesize
        assert_eq!(
            keys.get_action(KeyCode::Char('s'), KeyModifiers::NONE),
            Some(Action::Synthesize)
        );

        // Uppercase 'A' should trigger NewAgentWithPrompt
        assert_eq!(
            keys.get_action(KeyCode::Char('A'), KeyModifiers::SHIFT),
            Some(Action::NewAgentWithPrompt)
        );

        // Uppercase 'G' should trigger ScrollBottom
        assert_eq!(
            keys.get_action(KeyCode::Char('G'), KeyModifiers::SHIFT),
            Some(Action::ScrollBottom)
        );
    }

    #[test]
    fn test_unknown_key() {
        let keys = KeyBindings::default();

        assert_eq!(
            keys.get_action(KeyCode::Char('x'), KeyModifiers::NONE),
            None
        );
    }

    #[test]
    fn test_set_keybinding() {
        let mut keys = KeyBindings::default();
        keys.set("x", Action::Quit);

        assert_eq!(
            keys.get_action(KeyCode::Char('x'), KeyModifiers::NONE),
            Some(Action::Quit)
        );
    }

    #[test]
    fn test_keys_for_action() {
        let keys = KeyBindings::default();
        let attach_keys = keys.keys_for_action(Action::Attach);

        assert!(attach_keys.contains(&"Enter".to_string()));
        assert!(attach_keys.contains(&"o".to_string()));
    }

    #[test]
    fn test_format_keys_and_help_line() {
        let keys = KeyBindings::default();

        // Test that SpawnChildren shows "S"
        let spawn_keys = keys.format_keys(Action::SpawnChildren);
        assert_eq!(spawn_keys, "S");

        // Test help line format
        let spawn_help = keys.help_line(Action::SpawnChildren);
        assert!(spawn_help.contains('S'));
        assert!(spawn_help.contains("Spawn children"));
    }

    #[test]
    fn test_merge_defaults() {
        // Simulate an old config missing hierarchy keybindings
        let mut keys = KeyBindings {
            bindings: [
                ("a".to_string(), Action::NewAgent),
                ("q".to_string(), Action::Quit),
            ]
            .into_iter()
            .collect(),
        };

        // Should be missing SpawnChildren
        assert_eq!(
            keys.get_action(KeyCode::Char('S'), KeyModifiers::NONE),
            None
        );

        // After merging defaults, it should work
        keys.merge_defaults();
        assert_eq!(
            keys.get_action(KeyCode::Char('S'), KeyModifiers::NONE),
            Some(Action::SpawnChildren)
        );

        // Existing bindings should be preserved (not overwritten)
        assert_eq!(
            keys.get_action(KeyCode::Char('a'), KeyModifiers::NONE),
            Some(Action::NewAgent)
        );
    }

    #[test]
    fn test_key_to_string() {
        assert_eq!(key_to_string(KeyCode::Char('a'), KeyModifiers::NONE), "a");
        assert_eq!(
            key_to_string(KeyCode::Char('a'), KeyModifiers::CONTROL),
            "Ctrl+a"
        );
        assert_eq!(key_to_string(KeyCode::Enter, KeyModifiers::NONE), "Enter");
        assert_eq!(key_to_string(KeyCode::F(1), KeyModifiers::NONE), "F1");
    }

    #[test]
    fn test_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let keys = KeyBindings::default();
        let json = serde_json::to_string(&keys)?;
        let parsed: KeyBindings = serde_json::from_str(&json)?;
        assert_eq!(keys, parsed);
        Ok(())
    }
}
