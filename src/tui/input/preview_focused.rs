//! Preview focused mode key handling
//!
//! Handles key events when the preview pane is focused, forwarding
//! keystrokes to tmux via send-keys.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use tenex::app::App;

/// Handle key events in `PreviewFocused` mode
pub fn handle_preview_focused_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) {
    // Ctrl+q exits preview focus mode (same key quits app when not focused)
    if code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL) {
        app.exit_mode();
        return;
    }

    // Collect keys for batched sending (done after event drain loop)
    if let Some(keys) = keycode_to_tmux_keys(code, modifiers) {
        batched_keys.push(keys);
    }
}

/// Convert a `KeyCode` and modifiers to tmux send-keys format
pub fn keycode_to_tmux_keys(code: KeyCode, modifiers: KeyModifiers) -> Option<String> {
    let base_key = match code {
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+letter: C-a, C-b, etc.
                return Some(format!("C-{c}"));
            } else if modifiers.contains(KeyModifiers::ALT) {
                // Alt+letter: M-a, M-b, etc.
                return Some(format!("M-{c}"));
            }
            c.to_string()
        }
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Escape".to_string(),
        KeyCode::Backspace => "BSpace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BTab".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Delete => "DC".to_string(),
        KeyCode::Insert => "IC".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return None,
    };

    // Handle Ctrl/Alt modifiers for non-character keys
    if modifiers.contains(KeyModifiers::CONTROL) && !matches!(code, KeyCode::Char(_)) {
        Some(format!("C-{base_key}"))
    } else if modifiers.contains(KeyModifiers::ALT) && !matches!(code, KeyCode::Char(_)) {
        Some(format!("M-{base_key}"))
    } else {
        Some(base_key)
    }
}
