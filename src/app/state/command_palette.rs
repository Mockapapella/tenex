//! Slash command palette state

/// State for the `/` command palette
#[derive(Debug, Default, Clone, Copy)]
pub struct CommandPaletteState {
    /// Currently selected index in filtered list
    pub selected: usize,
}

impl CommandPaletteState {
    /// Create a new command palette state
    #[must_use]
    pub const fn new() -> Self {
        Self { selected: 0 }
    }

    /// Reset the palette selection
    pub const fn reset(&mut self) {
        self.selected = 0;
    }
}

use super::{App, SlashCommand};
use crate::app::AgentRole;
use crate::state::{AppMode, CommandPaletteMode, HelpMode, SettingsMenuMode};

impl App {
    /// Enter slash command palette mode and pre-fill the leading `/`
    pub fn start_command_palette(&mut self) {
        self.apply_mode(CommandPaletteMode.into());
    }

    /// Return the list of slash commands filtered by the current palette input.
    #[must_use]
    pub fn filtered_slash_commands(&self) -> Vec<SlashCommand> {
        self.data.filtered_slash_commands()
    }

    /// Execute the currently-typed slash command (called when user presses Enter).
    pub fn submit_slash_command_palette(&mut self) {
        let next = self.data.submit_slash_command_palette();
        self.apply_mode(next);
    }

    /// Execute a resolved slash command.
    pub fn run_slash_command(&mut self, cmd: SlashCommand) {
        let next = match cmd.name {
            "/agents" => {
                self.data.input.clear();
                self.data.model_selector.role = AgentRole::Default;
                SettingsMenuMode.into()
            }
            "/help" => {
                self.data.ui.help_scroll = 0;
                HelpMode.into()
            }
            other => {
                self.set_status(format!("Unknown command: {other}"));
                AppMode::normal()
            }
        };
        self.apply_mode(next);
    }

    /// Select the next slash command in the filtered list.
    pub fn select_next_slash_command(&mut self) {
        self.data.select_next_slash_command();
    }

    /// Select the previous slash command in the filtered list.
    pub fn select_prev_slash_command(&mut self) {
        self.data.select_prev_slash_command();
    }

    /// Reset the slash command selection back to the first entry.
    pub const fn reset_slash_command_selection(&mut self) {
        self.data.command_palette.selected = 0;
    }

    /// Get the currently selected slash command (based on filter + selection index).
    #[must_use]
    pub fn selected_slash_command(&self) -> Option<SlashCommand> {
        self.filtered_slash_commands()
            .get(self.data.command_palette.selected)
            .copied()
    }

    /// Run the currently highlighted command in the palette (fallbacks to parsing the input).
    pub fn confirm_slash_command_selection(&mut self) {
        let next = self.data.confirm_slash_command_selection();
        self.apply_mode(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let state = CommandPaletteState::new();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_default() {
        let state = CommandPaletteState::default();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_reset() {
        let mut state = CommandPaletteState::new();
        state.selected = 5;
        state.reset();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_clone() {
        let mut state = CommandPaletteState::new();
        state.selected = 3;
        let cloned = state;
        assert_eq!(cloned.selected, 3);
    }

    #[test]
    fn test_debug() {
        let state = CommandPaletteState::new();
        let debug_str = format!("{state:?}");
        assert!(debug_str.contains("CommandPaletteState"));
        assert!(debug_str.contains("selected"));
    }
}
