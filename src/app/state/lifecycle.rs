//! Application mode transitions and modal lifecycle.

use tracing::{debug, warn};

use super::{App, Mode};

impl App {
    /// Enter a new application mode
    pub fn enter_mode(&mut self, mode: Mode) {
        debug!(new_mode = ?mode, old_mode = ?self.mode, "Entering mode");
        // Don't clear for PushRenameBranch - we pre-fill it with the branch name
        let should_clear = matches!(
            mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::Confirming(_)
                | Mode::CommandPalette
                | Mode::ChildPrompt
                | Mode::Broadcasting
                | Mode::TerminalPrompt
                | Mode::CustomAgentCommand
        );
        self.mode = mode;
        if should_clear {
            self.input.clear();
        }
    }

    /// Exit the current mode and return to normal mode
    pub fn exit_mode(&mut self) {
        debug!(old_mode = ?self.mode, "Exiting mode");
        self.mode = Mode::Normal;
        self.input.clear();
    }

    /// Set an error message and show the error modal
    pub fn set_error(&mut self, message: impl Into<String>) {
        let msg = message.into();
        self.ui.set_error(msg.clone());
        self.mode = Mode::ErrorModal(msg);
    }

    /// Clear the current error message
    pub fn clear_error(&mut self) {
        self.ui.clear_error();
    }

    /// Dismiss the error modal (returns to normal mode)
    pub fn dismiss_error(&mut self) {
        if matches!(self.mode, Mode::ErrorModal(_)) {
            self.mode = Mode::Normal;
        }
        self.ui.clear_error();
    }

    /// Set a status message to display
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.ui.set_status(message);
    }

    /// Clear the current status message
    pub fn clear_status(&mut self) {
        self.ui.clear_status();
    }

    /// Show success modal with message
    pub fn show_success(&mut self, message: impl Into<String>) {
        self.mode = Mode::SuccessModal(message.into());
    }

    /// Dismiss success modal
    pub fn dismiss_success(&mut self) {
        if matches!(self.mode, Mode::SuccessModal(_)) {
            self.mode = Mode::Normal;
        }
    }

    /// Check if keyboard remap prompt should be shown at startup
    /// Returns true if terminal doesn't support enhancement AND user hasn't been asked yet
    #[must_use]
    pub const fn should_show_keyboard_remap_prompt(&self) -> bool {
        !self.keyboard_enhancement_supported && !self.settings.keyboard_remap_asked
    }

    /// Show the keyboard remap prompt modal
    pub fn show_keyboard_remap_prompt(&mut self) {
        self.mode = Mode::KeyboardRemapPrompt;
    }

    /// Accept the keyboard remap (Ctrl+M -> Ctrl+N)
    pub fn accept_keyboard_remap(&mut self) {
        if let Err(e) = self.settings.enable_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        self.mode = Mode::Normal;
    }

    /// Decline the keyboard remap
    pub fn decline_keyboard_remap(&mut self) {
        if let Err(e) = self.settings.decline_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        self.mode = Mode::Normal;
    }

    /// Check if merge key should use Ctrl+N instead of Ctrl+M
    #[must_use]
    pub const fn is_merge_key_remapped(&self) -> bool {
        self.settings.merge_key_remapped
    }
}

