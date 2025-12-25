//! Normal and Scrolling mode key handling
//!
//! Handles key events in the default application modes where
//! keybindings are mapped to actions via the config system.

use crate::app::{Actions, App};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Handle key events in Normal or Scrolling mode
pub fn handle_normal_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    if let Some(action) = crate::config::get_action(code, modifiers) {
        action_handler.handle_action(app, action)?;
    }
    Ok(())
}
