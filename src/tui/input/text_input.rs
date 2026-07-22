//! Text input mode key handling
//!
//! Handles key events for modes that involve text input:
//! - `Creating` (new agent name)
//! - `Prompting` (new agent with prompt)
//! - `ChildPrompt` (task for children)
//! - `Broadcasting` (message to leaves)
//! - `ReconnectPrompt` (reconnect with edited prompt)
//! - `TerminalPrompt` (terminal startup command)
//! - `SynthesisPrompt` (extra synthesis instructions)

use crate::app::App;
use crate::state::AppMode;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// Handle key events in text input modes
pub fn handle_text_input_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match &app.mode {
        AppMode::Creating(_) => crate::action::dispatch_creating_mode(app, code, modifiers)?,
        AppMode::Prompting(_) => crate::action::dispatch_prompting_mode(app, code, modifiers)?,
        AppMode::ChildPrompt(_) => crate::action::dispatch_child_prompt_mode(app, code, modifiers)?,
        AppMode::Broadcasting(_) => {
            crate::action::dispatch_broadcasting_mode(app, code, modifiers)?;
        }
        AppMode::ReconnectPrompt(_) => {
            crate::action::dispatch_reconnect_prompt_mode(app, code, modifiers)?;
        }
        AppMode::TerminalPrompt(_) => {
            crate::action::dispatch_terminal_prompt_mode(app, code, modifiers)?;
        }
        AppMode::CustomAgentCommand(_) => {
            crate::action::dispatch_custom_agent_command_mode(app, code, modifiers)?;
        }
        AppMode::SynthesisPrompt(_) => {
            crate::action::dispatch_synthesis_prompt_mode(app, code, modifiers)?;
        }
        _ => {}
    }
    Ok(())
}
