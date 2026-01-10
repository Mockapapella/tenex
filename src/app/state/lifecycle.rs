//! Application mode transitions and modal lifecycle.

use tracing::{debug, warn};

use super::App;
use crate::app::AgentRole;
use crate::state::{
    AppMode, CommandPaletteMode, ErrorModalMode, KeyboardRemapPromptMode, ModelSelectorMode,
    SettingsMenuMode, SuccessModalMode,
};

impl App {
    /// Apply a mode transition to the application, running any required
    /// entry/exit hooks.
    pub fn apply_mode(&mut self, next: AppMode) {
        if self.mode == next {
            return;
        }

        debug!(new_mode = ?next, old_mode = ?self.mode, "Applying mode transition");

        match next {
            AppMode::Normal(_) => {
                self.mode = AppMode::normal();
                self.data.input.clear();
            }
            AppMode::CommandPalette(_) => {
                self.data.command_palette.reset();
                self.data.input.buffer = "/".to_string();
                self.data.input.cursor = 1;
                self.data.input.scroll = 0;
                self.mode = CommandPaletteMode.into();
            }
            AppMode::ModelSelector(_) => {
                let current = match self.data.model_selector.role {
                    AgentRole::Default => self.data.settings.agent_program,
                    AgentRole::Planner => self.data.settings.planner_agent_program,
                    AgentRole::Review => self.data.settings.review_agent_program,
                };

                self.data.model_selector.start(current);
                self.mode = ModelSelectorMode.into();
            }
            AppMode::SettingsMenu(_) => {
                self.data.settings_menu.reset();
                self.data.input.clear();
                self.mode = SettingsMenuMode.into();
            }
            AppMode::Creating(state) => {
                self.data.input.clear();
                self.mode = AppMode::Creating(state);
            }
            AppMode::Prompting(state) => {
                self.data.input.clear();
                self.mode = AppMode::Prompting(state);
            }
            AppMode::Confirming(state) => {
                self.data.input.clear();
                self.mode = AppMode::Confirming(state);
            }
            AppMode::ChildPrompt(state) => {
                self.data.input.clear();
                self.mode = AppMode::ChildPrompt(state);
            }
            AppMode::Broadcasting(state) => {
                self.data.input.clear();
                self.mode = AppMode::Broadcasting(state);
            }
            AppMode::TerminalPrompt(state) => {
                self.data.input.clear();
                self.mode = AppMode::TerminalPrompt(state);
            }
            AppMode::CustomAgentCommand(state) => {
                let existing = match self.data.model_selector.role {
                    AgentRole::Default => self.data.settings.custom_agent_command.clone(),
                    AgentRole::Planner => self.data.settings.planner_custom_agent_command.clone(),
                    AgentRole::Review => self.data.settings.review_custom_agent_command.clone(),
                };

                self.data.input.clear();
                self.data.input.set(existing);
                self.mode = AppMode::CustomAgentCommand(state);
            }
            AppMode::ErrorModal(state) => {
                self.data.ui.set_error(state.message.clone());
                self.mode = AppMode::ErrorModal(state);
            }
            AppMode::SuccessModal(state) => {
                self.mode = AppMode::SuccessModal(state);
            }
            other => {
                self.mode = other;
            }
        }
    }

    /// Enter a new application mode.
    pub fn enter_mode(&mut self, mode: AppMode) {
        self.apply_mode(mode);
    }

    /// Exit the current mode and return to normal mode.
    pub fn exit_mode(&mut self) {
        self.apply_mode(AppMode::normal());
    }

    /// Set an error message and show the error modal.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.apply_mode(
            ErrorModalMode {
                message: message.into(),
            }
            .into(),
        );
    }

    /// Clear the current error message.
    pub fn clear_error(&mut self) {
        self.data.ui.clear_error();
    }

    /// Dismiss the error modal and clear the stored error message.
    pub fn dismiss_error(&mut self) {
        self.data.ui.clear_error();
        self.apply_mode(AppMode::normal());
    }

    /// Set a status message to display.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.data.ui.set_status(message);
    }

    /// Clear the current status message.
    pub fn clear_status(&mut self) {
        self.data.ui.clear_status();
    }

    /// Show success modal with message.
    pub fn show_success(&mut self, message: impl Into<String>) {
        self.apply_mode(
            SuccessModalMode {
                message: message.into(),
            }
            .into(),
        );
    }

    /// Dismiss success modal.
    pub fn dismiss_success(&mut self) {
        self.apply_mode(AppMode::normal());
    }

    /// Check if keyboard remap prompt should be shown at startup
    /// Returns true if terminal doesn't support enhancement AND user hasn't been asked yet
    #[must_use]
    pub const fn should_show_keyboard_remap_prompt(&self) -> bool {
        !self.data.keyboard_enhancement_supported && !self.data.settings.keyboard_remap_asked
    }

    /// Show the keyboard remap prompt modal
    pub fn show_keyboard_remap_prompt(&mut self) {
        self.apply_mode(KeyboardRemapPromptMode.into());
    }

    /// Accept the keyboard remap (Ctrl+M -> Ctrl+N)
    pub fn accept_keyboard_remap(&mut self) {
        if let Err(e) = self.data.settings.enable_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        self.apply_mode(AppMode::normal());
    }

    /// Decline the keyboard remap
    pub fn decline_keyboard_remap(&mut self) {
        if let Err(e) = self.data.settings.decline_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        self.apply_mode(AppMode::normal());
    }

    /// Check if merge key should use Ctrl+N instead of Ctrl+M
    #[must_use]
    pub const fn is_merge_key_remapped(&self) -> bool {
        self.data.settings.merge_key_remapped
    }
}
