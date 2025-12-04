//! Keybinding configuration

use crossterm::event::{KeyCode, KeyModifiers};
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
}

/// A keybinding definition
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    /// The key code
    pub code: KeyCode,
    /// Key modifiers (Ctrl, Alt, Shift)
    pub modifiers: KeyModifiers,
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

        bindings.insert("n".to_string(), Action::NewAgent);
        bindings.insert("N".to_string(), Action::NewAgentWithPrompt);
        bindings.insert("Enter".to_string(), Action::Attach);
        bindings.insert("o".to_string(), Action::Attach);
        bindings.insert("d".to_string(), Action::Kill);
        bindings.insert("p".to_string(), Action::Push);
        bindings.insert("c".to_string(), Action::Pause);
        bindings.insert("r".to_string(), Action::Resume);
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

        Self { bindings }
    }
}

impl KeyBindings {
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
            keys.get_action(KeyCode::Char('n'), KeyModifiers::NONE),
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
    fn test_key_to_string() {
        assert_eq!(key_to_string(KeyCode::Char('a'), KeyModifiers::NONE), "a");
        assert_eq!(key_to_string(KeyCode::Char('a'), KeyModifiers::CONTROL), "Ctrl+a");
        assert_eq!(key_to_string(KeyCode::Enter, KeyModifiers::NONE), "Enter");
        assert_eq!(key_to_string(KeyCode::F(1), KeyModifiers::NONE), "F1");
    }

    #[test]
    fn test_keybinding_struct() {
        let kb = KeyBinding {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(kb.code, KeyCode::Char('a'));
        assert_eq!(kb.modifiers, KeyModifiers::NONE);

        let kb_ctrl = KeyBinding {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
        };
        assert_eq!(kb_ctrl.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_serde_roundtrip() {
        let keys = KeyBindings::default();
        let json = serde_json::to_string(&keys).unwrap();
        let parsed: KeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(keys, parsed);
    }
}
