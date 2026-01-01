//! Normal and Scrolling mode key handling
//!
//! Handles key events in the default application modes where
//! keybindings are mapped to actions via the config system.

use crate::app::App;
use crate::state::AppMode;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Handle key events in Normal or Scrolling mode
pub fn handle_normal_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    if let Some(action) = crate::config::get_action(code, modifiers) {
        match app.mode {
            AppMode::Normal(_) => crate::action::dispatch_normal_mode(app, action)?,
            AppMode::Scrolling(_) => crate::action::dispatch_scrolling_mode(app, action)?,
            _ => {}
        }
    }
    Ok(())
}
