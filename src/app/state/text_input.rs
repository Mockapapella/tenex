//! Text input helpers for modes that accept editing.

use super::{App, Mode};

impl App {
    /// Check if the current mode accepts text input
    ///
    /// This is used to consolidate the mode check that was previously
    /// duplicated across `handle_char`, `handle_backspace`, and `handle_delete`.
    #[must_use]
    pub const fn is_text_input_mode(&self) -> bool {
        matches!(
            self.mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::CommandPalette
                | Mode::ChildPrompt
                | Mode::Broadcasting
                | Mode::RenameBranch
                | Mode::ReconnectPrompt
                | Mode::TerminalPrompt
                | Mode::CustomAgentCommand
        ) || matches!(self.mode, Mode::Confirming(_))
    }

    /// Handle a character input in text input modes
    pub fn handle_char(&mut self, c: char) {
        if self.is_text_input_mode() {
            self.input.insert_char(c);
        }
    }

    /// Handle backspace in text input modes
    pub fn handle_backspace(&mut self) {
        if self.is_text_input_mode() {
            self.input.backspace();
        }
    }

    /// Handle delete key in text input modes (delete char at cursor)
    pub fn handle_delete(&mut self) {
        if self.is_text_input_mode() {
            self.input.delete();
        }
    }

    /// Move cursor left in text input
    pub fn input_cursor_left(&mut self) {
        self.input.cursor_left();
    }

    /// Move cursor right in text input
    pub fn input_cursor_right(&mut self) {
        self.input.cursor_right();
    }

    /// Move cursor up one line in text input
    pub fn input_cursor_up(&mut self) {
        self.input.cursor_up();
    }

    /// Move cursor down one line in text input
    pub fn input_cursor_down(&mut self) {
        self.input.cursor_down();
    }

    /// Move cursor to start of line
    pub fn input_cursor_home(&mut self) {
        self.input.cursor_home();
    }

    /// Move cursor to end of line
    pub fn input_cursor_end(&mut self) {
        self.input.cursor_end();
    }
}

