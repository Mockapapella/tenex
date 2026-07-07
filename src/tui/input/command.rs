//! Slash command palette and related pickers

use crate::app::App;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `CommandPalette` mode
pub fn handle_command_palette_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_command_palette_mode(app, code)
}

/// Handle key events in `ModelSelector` mode
pub fn handle_model_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_model_selector_mode(app, code)
}

/// Handle key events in `SettingsMenu` mode
pub fn handle_settings_menu_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_settings_menu_mode(app, code)
}

#[cfg(test)]
mod tests;
