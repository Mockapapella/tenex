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

use super::{App, Mode, SLASH_COMMANDS, SlashCommand};

impl App {
    /// Enter slash command palette mode and pre-fill the leading `/`
    pub fn start_command_palette(&mut self) {
        self.enter_mode(Mode::CommandPalette);
        self.command_palette.reset();
        self.input.buffer = "/".to_string();
        self.input.cursor = 1;
        self.input.scroll = 0;
    }

    /// Return the list of slash commands filtered by the current palette input.
    #[must_use]
    pub fn filtered_slash_commands(&self) -> Vec<SlashCommand> {
        let raw = self.input.buffer.trim();
        let query = raw
            .strip_prefix('/')
            .unwrap_or(raw)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        SLASH_COMMANDS
            .iter()
            .copied()
            .filter(|cmd| {
                query.is_empty()
                    || cmd
                        .name
                        .trim_start_matches('/')
                        .to_ascii_lowercase()
                        .starts_with(&query)
            })
            .collect()
    }

    /// Execute the currently-typed slash command (called when user presses Enter).
    pub fn submit_slash_command_palette(&mut self) {
        let typed = self
            .input
            .buffer
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();

        if typed.is_empty() || typed == "/" {
            self.exit_mode();
            return;
        }

        let normalized = if typed.starts_with('/') {
            typed.to_ascii_lowercase()
        } else {
            format!("/{typed}").to_ascii_lowercase()
        };

        if let Some(cmd) = SLASH_COMMANDS
            .iter()
            .copied()
            .find(|c| c.name.eq_ignore_ascii_case(&normalized))
        {
            self.run_slash_command(cmd);
            return;
        }

        let query = normalized.trim_start_matches('/').to_string();
        let matches: Vec<SlashCommand> = SLASH_COMMANDS
            .iter()
            .copied()
            .filter(|c| {
                c.name
                    .trim_start_matches('/')
                    .to_ascii_lowercase()
                    .starts_with(&query)
            })
            .collect();

        match matches.as_slice() {
            [] => {
                self.set_status(format!("Unknown command: {typed}"));
                self.exit_mode();
            }
            [single] => self.run_slash_command(*single),
            _ => {
                self.set_status(format!("Ambiguous command: {typed}"));
                self.exit_mode();
            }
        }
    }

    /// Execute a resolved slash command.
    pub fn run_slash_command(&mut self, cmd: SlashCommand) {
        match cmd.name {
            "/agents" => {
                self.input.clear();
                self.start_model_selector();
            }
            "/help" => {
                self.ui.help_scroll = 0;
                self.enter_mode(Mode::Help);
            }
            _ => {
                self.set_status(format!("Unknown command: {}", cmd.name));
                self.exit_mode();
            }
        }
    }

    /// Select the next slash command in the filtered list.
    pub fn select_next_slash_command(&mut self) {
        let count = self.filtered_slash_commands().len();
        if count > 0 {
            self.command_palette.selected = (self.command_palette.selected + 1) % count;
        } else {
            self.command_palette.selected = 0;
        }
    }

    /// Select the previous slash command in the filtered list.
    pub fn select_prev_slash_command(&mut self) {
        let count = self.filtered_slash_commands().len();
        if count > 0 {
            self.command_palette.selected = self
                .command_palette
                .selected
                .checked_sub(1)
                .unwrap_or(count - 1);
        } else {
            self.command_palette.selected = 0;
        }
    }

    /// Reset the slash command selection back to the first entry.
    pub const fn reset_slash_command_selection(&mut self) {
        self.command_palette.selected = 0;
    }

    /// Get the currently selected slash command (based on filter + selection index).
    #[must_use]
    pub fn selected_slash_command(&self) -> Option<SlashCommand> {
        self.filtered_slash_commands()
            .get(self.command_palette.selected)
            .copied()
    }

    /// Run the currently highlighted command in the palette (fallbacks to parsing the input).
    pub fn confirm_slash_command_selection(&mut self) {
        if let Some(cmd) = self.selected_slash_command() {
            self.run_slash_command(cmd);
        } else {
            self.submit_slash_command_palette();
        }
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
