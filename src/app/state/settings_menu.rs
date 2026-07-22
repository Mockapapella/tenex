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
        self.selected = (self.selected + 1) % count;
    }

    /// Select the previous menu item.
    pub const fn select_prev(&mut self) {
        let count = AgentRole::ALL.len();
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
