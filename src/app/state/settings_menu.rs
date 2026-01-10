//! Settings menu state: selecting which setting to edit.

use crate::app::AgentRole;

/// State for the `/agents` role selection menu.
#[derive(Debug, Default, Clone, Copy)]
pub struct SettingsMenuState {
    /// Currently selected index in the menu list.
    pub selected: usize,
}

impl SettingsMenuState {
    /// Create a new settings menu state.
    #[must_use]
    pub const fn new() -> Self {
        Self { selected: 0 }
    }

    /// Reset selection back to the first entry.
    pub const fn reset(&mut self) {
        self.selected = 0;
    }

    /// Select the next menu item.
    pub const fn select_next(&mut self) {
        let count = AgentRole::ALL.len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        } else {
            self.selected = 0;
        }
    }

    /// Select the previous menu item.
    pub const fn select_prev(&mut self) {
        let count = AgentRole::ALL.len();
        if count == 0 {
            self.selected = 0;
            return;
        }

        if self.selected == 0 {
            self.selected = count - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Return the currently highlighted role.
    #[must_use]
    pub const fn selected_role(self) -> AgentRole {
        match self.selected {
            1 => AgentRole::Planner,
            2 => AgentRole::Review,
            _ => AgentRole::Default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let state = SettingsMenuState::new();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_reset() {
        let mut state = SettingsMenuState::new();
        state.selected = 2;
        state.reset();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_select_next_wraps() {
        let mut state = SettingsMenuState::new();
        state.selected = AgentRole::ALL.len() - 1;
        state.select_next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_select_prev_wraps() {
        let mut state = SettingsMenuState::new();
        state.selected = 0;
        state.select_prev();
        assert_eq!(state.selected, AgentRole::ALL.len() - 1);
    }

    #[test]
    fn test_selected_role_defaults() {
        let state = SettingsMenuState { selected: 99 };
        assert_eq!(state.selected_role(), AgentRole::Default);
    }
}
