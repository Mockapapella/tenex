//! Text input helpers for modes that accept editing.

use super::App;
use crate::state::AppMode;

impl App {
    /// Check if the current mode accepts text input
    ///
    /// This is used to consolidate the mode check that was previously
    /// duplicated across `handle_char`, `handle_backspace`, and `handle_delete`.
    #[must_use]
    pub const fn is_text_input_mode(&self) -> bool {
        matches!(
            self.mode,
            AppMode::Creating(_)
                | AppMode::Prompting(_)
                | AppMode::CommandPalette(_)
                | AppMode::ChildPrompt(_)
                | AppMode::Broadcasting(_)
                | AppMode::RenameBranch(_)
                | AppMode::ReconnectPrompt(_)
                | AppMode::TerminalPrompt(_)
                | AppMode::CustomAgentCommand(_)
        ) || matches!(self.mode, AppMode::Confirming(_))
    }

    /// Handle a character input in text input modes
    pub fn handle_char(&mut self, c: char) {
        if self.is_text_input_mode() {
            self.data.input.insert_char(c);
        }
    }

    /// Handle backspace in text input modes
    pub fn handle_backspace(&mut self) {
        if self.is_text_input_mode() {
            self.data.input.backspace();
        }
    }

    /// Handle delete key in text input modes (delete char at cursor)
    pub fn handle_delete(&mut self) {
        if self.is_text_input_mode() {
            self.data.input.delete();
        }
    }

    /// Move cursor left in text input
    pub fn input_cursor_left(&mut self) {
        self.data.input.cursor_left();
    }

    /// Move cursor right in text input
    pub fn input_cursor_right(&mut self) {
        self.data.input.cursor_right();
    }

    /// Move cursor up one line in text input
    pub fn input_cursor_up(&mut self) {
        self.data.input.cursor_up();
    }

    /// Move cursor down one line in text input
    pub fn input_cursor_down(&mut self) {
        self.data.input.cursor_down();
    }

    /// Move cursor to start of line
    pub fn input_cursor_home(&mut self) {
        self.data.input.cursor_home();
    }

    /// Move cursor to end of line
    pub fn input_cursor_end(&mut self) {
        self.data.input.cursor_end();
    }
}
