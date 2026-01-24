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
    // Ctrl+q should always exit content focus (scrolling) before it can quit the app.
    if matches!(app.mode, AppMode::Scrolling(_))
        && matches!(code, KeyCode::Char('q' | 'Q'))
        && modifiers.contains(KeyModifiers::CONTROL)
    {
        app.apply_mode(AppMode::normal());
        return Ok(());
    }

    // Tab switching should be bound to Normal mode only (agents list focus).
    if matches!(app.mode, AppMode::Scrolling(_)) && matches!(code, KeyCode::Tab | KeyCode::BackTab)
    {
        return Ok(());
    }

    // When the content pane is focused, treat ↑/↓ as scrolling rather than switching agents.
    if matches!(app.mode, AppMode::Scrolling(_)) && modifiers == KeyModifiers::NONE {
        match code {
            KeyCode::Up => {
                app.data.scroll_up(1);
                return Ok(());
            }
            KeyCode::Down => {
                app.data.scroll_down(1);
                return Ok(());
            }
            _ => {}
        }
    }

    if let Some(action) = crate::config::get_action(code, modifiers) {
        match app.mode {
            AppMode::Normal(_) => crate::action::dispatch_normal_mode(app, action)?,
            AppMode::Scrolling(_) => crate::action::dispatch_scrolling_mode(app, action)?,
            _ => {}
        }
    }
    Ok(())
}
