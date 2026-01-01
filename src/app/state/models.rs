//! Model selector state: selecting which agent command to run by default

use crate::app::AgentProgram;

/// State for the `/agents` selector modal
#[derive(Debug, Default)]
pub struct ModelSelectorState {
    /// Current filter text for model search
    pub filter: String,

    /// Currently selected index in filtered list
    pub selected: usize,
}

impl ModelSelectorState {
    /// Create a new model selector state
    #[must_use]
    pub const fn new() -> Self {
        Self {
            filter: String::new(),
            selected: 0,
        }
    }

    /// Start the selector with the current selection highlighted
    pub fn start(&mut self, current: AgentProgram) {
        self.filter.clear();
        self.selected = AgentProgram::ALL
            .iter()
            .position(|&p| p == current)
            .unwrap_or(0);
    }

    /// Get filtered programs based on current filter
    #[must_use]
    pub fn filtered_programs(&self) -> Vec<AgentProgram> {
        let filter_lower = self.filter.to_ascii_lowercase();
        AgentProgram::ALL
            .iter()
            .copied()
            .filter(|p| {
                filter_lower.is_empty() || p.label().to_ascii_lowercase().contains(&filter_lower)
            })
            .collect()
    }

    /// Select next item in filtered list
    pub fn select_next(&mut self) {
        let count = self.filtered_programs().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// Select previous item in filtered list
    pub fn select_prev(&mut self) {
        let count = self.filtered_programs().len();
        if count > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
        }
    }

    /// Get the currently selected program
    #[must_use]
    pub fn selected_program(&self) -> Option<AgentProgram> {
        self.filtered_programs().get(self.selected).copied()
    }

    /// Handle character input in filter
    pub fn handle_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
    }

    /// Handle backspace in filter
    pub fn handle_filter_backspace(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }

    /// Clear all model selector state
    pub fn clear(&mut self) {
        self.filter.clear();
        self.selected = 0;
    }
}

use super::App;
use crate::state::{CustomAgentCommandMode, ModelSelectorMode};

impl App {
    /// Enter the `/agents` selector modal.
    pub fn start_model_selector(&mut self) {
        self.apply_mode(ModelSelectorMode.into());
    }

    /// Return the filtered model/program list for the selector UI.
    #[must_use]
    pub fn filtered_model_programs(&self) -> Vec<AgentProgram> {
        self.data.model_selector.filtered_programs()
    }

    /// Select next model/program in filtered list.
    pub fn select_next_model_program(&mut self) {
        self.data.model_selector.select_next();
    }

    /// Select previous model/program in filtered list.
    pub fn select_prev_model_program(&mut self) {
        self.data.model_selector.select_prev();
    }

    /// Handle typing in the `/agents` filter.
    pub fn handle_model_filter_char(&mut self, c: char) {
        self.data.model_selector.handle_filter_char(c);
    }

    /// Handle backspace in the `/agents` filter.
    pub fn handle_model_filter_backspace(&mut self) {
        self.data.model_selector.handle_filter_backspace();
    }

    /// Get the currently highlighted model/program (in `/agents`).
    #[must_use]
    pub fn selected_model_program(&self) -> Option<AgentProgram> {
        self.data.model_selector.selected_program()
    }

    /// Confirm the current `/agents` selection.
    pub fn confirm_model_program_selection(&mut self) {
        let next = self.data.confirm_model_program_selection();
        self.apply_mode(next);
    }

    /// Open the custom agent command prompt (used when selecting `custom`).
    pub fn start_custom_agent_command_prompt(&mut self) {
        self.apply_mode(CustomAgentCommandMode.into());
        self.data
            .input
            .set(self.data.settings.custom_agent_command.clone());
    }

    /// Set the agent program and persist settings to disk.
    pub fn set_agent_program_and_save(&mut self, program: AgentProgram) {
        self.data.settings.agent_program = program;
        if let Err(e) = self.data.settings.save() {
            self.set_error(format!("Failed to save settings: {e}"));
            return;
        }

        self.set_status(format!("Model set to {}", program.label()));
    }

    /// Update the custom agent command, select `custom`, and persist settings.
    pub fn set_custom_agent_command_and_save(&mut self, command: String) {
        self.data.settings.custom_agent_command = command;
        self.data.settings.agent_program = AgentProgram::Custom;
        if let Err(e) = self.data.settings.save() {
            self.set_error(format!("Failed to save settings: {e}"));
            return;
        }

        self.set_status("Model set to custom");
    }

    /// The base command used when spawning new agents (based on user settings).
    #[must_use]
    pub fn agent_spawn_command(&self) -> String {
        match self.data.settings.agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.data.config.default_program.clone(),
            AgentProgram::Custom => {
                let custom = self.data.settings.custom_agent_command.trim();
                if custom.is_empty() {
                    self.data.config.default_program.clone()
                } else {
                    custom.to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let state = ModelSelectorState::new();
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_default() {
        let state = ModelSelectorState::default();
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_start_highlights_current() {
        let mut state = ModelSelectorState::new();
        state.start(AgentProgram::Claude);
        assert_eq!(state.selected, 1);
        assert!(state.filter.is_empty());
    }

    #[test]
    fn test_start_clears_filter() {
        let mut state = ModelSelectorState::new();
        state.filter = "something".to_string();
        state.start(AgentProgram::Codex);
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_filtered_programs() {
        let mut state = ModelSelectorState::new();
        state.filter = "cod".to_string();
        let filtered = state.filtered_programs();
        assert_eq!(filtered, vec![AgentProgram::Codex]);
    }

    #[test]
    fn test_filtered_programs_empty_filter() {
        let state = ModelSelectorState::new();
        let filtered = state.filtered_programs();
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_filtered_programs_case_insensitive() {
        let mut state = ModelSelectorState::new();
        state.filter = "CLAU".to_string();
        let filtered = state.filtered_programs();
        assert_eq!(filtered, vec![AgentProgram::Claude]);
    }

    #[test]
    fn test_select_next_wraps() {
        let mut state = ModelSelectorState::new();
        state.selected = 2;
        state.select_next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_select_next_increments() {
        let mut state = ModelSelectorState::new();
        state.selected = 0;
        state.select_next();
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_select_prev_wraps() {
        let mut state = ModelSelectorState::new();
        state.selected = 0;
        state.select_prev();
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn test_select_prev_decrements() {
        let mut state = ModelSelectorState::new();
        state.selected = 2;
        state.select_prev();
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_selected_program_none_when_filtered_empty() {
        let mut state = ModelSelectorState::new();
        state.filter = "nope".to_string();
        assert!(state.selected_program().is_none());
    }

    #[test]
    fn test_selected_program_returns_correct() {
        let mut state = ModelSelectorState::new();
        state.selected = 1;
        assert_eq!(state.selected_program(), Some(AgentProgram::Claude));
    }

    #[test]
    fn test_handle_filter_char() {
        let mut state = ModelSelectorState::new();
        state.selected = 2;
        state.handle_filter_char('a');
        assert_eq!(state.filter, "a");
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_handle_filter_backspace() {
        let mut state = ModelSelectorState::new();
        state.filter = "abc".to_string();
        state.selected = 2;
        state.handle_filter_backspace();
        assert_eq!(state.filter, "ab");
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_handle_filter_backspace_empty() {
        let mut state = ModelSelectorState::new();
        state.handle_filter_backspace();
        assert!(state.filter.is_empty());
    }

    #[test]
    fn test_clear() {
        let mut state = ModelSelectorState::new();
        state.filter = "test".to_string();
        state.selected = 2;
        state.clear();
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
    }
}
