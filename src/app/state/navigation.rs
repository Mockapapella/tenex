//! Application navigation and selection.

use crate::agent::{Agent, Status};

use super::{App, Tab};

impl App {
    /// Get the currently selected agent (from visible agents list)
    #[must_use]
    pub fn selected_agent(&self) -> Option<&Agent> {
        self.storage.visible_agent_at(self.selected)
    }

    /// Get a mutable reference to the currently selected agent
    pub fn selected_agent_mut(&mut self) -> Option<&mut Agent> {
        // Get the ID first, then get mutable reference
        let agent_id = self.storage.visible_agent_at(self.selected)?.id;
        self.storage.get_mut(agent_id)
    }

    /// Move selection to the next agent (in visible list)
    pub fn select_next(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count > 0 {
            self.selected = (self.selected + 1) % visible_count;
            self.reset_scroll();
            self.ensure_agent_list_scroll();
        }
    }

    /// Move selection to the previous agent (in visible list)
    pub fn select_prev(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(visible_count - 1);
            self.reset_scroll();
            self.ensure_agent_list_scroll();
        }
    }

    /// Switch between preview and diff tabs
    pub const fn switch_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Preview => Tab::Diff,
            Tab::Diff => Tab::Preview,
        };
        self.reset_scroll();
    }

    /// Ensure the selection index is valid for the current visible agents
    pub fn validate_selection(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count == 0 {
            self.selected = 0;
        } else if self.selected >= visible_count {
            self.selected = visible_count - 1;
        }
        self.ensure_agent_list_scroll();
    }

    /// Ensure the agent list scroll offset keeps the selected agent visible.
    pub fn ensure_agent_list_scroll(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count == 0 {
            self.ui.agent_list_scroll = 0;
            return;
        }

        // `preview_dimensions` stores the preview inner height, which is `frame_height - 4`.
        // The agent list inner height is `frame_height - 3` (one line taller, because it has no tab bar).
        let preview_inner_height = usize::from(self.ui.preview_dimensions.map_or(20, |(_, h)| h));
        let viewport_height = preview_inner_height.saturating_add(1);
        let max_scroll = visible_count.saturating_sub(viewport_height);

        let mut scroll = self.ui.agent_list_scroll.min(max_scroll);

        if self.selected < scroll {
            scroll = self.selected;
        } else {
            let bottom = scroll.saturating_add(viewport_height).saturating_sub(1);
            if self.selected > bottom {
                scroll = self
                    .selected
                    .saturating_sub(viewport_height.saturating_sub(1));
            }
        }

        self.ui.agent_list_scroll = scroll.min(max_scroll);
    }

    /// Toggle collapse state of the selected agent
    pub fn toggle_selected_collapse(&mut self) {
        if let Some(agent) = self.selected_agent_mut() {
            agent.collapsed = !agent.collapsed;
            self.ensure_agent_list_scroll();
        }
    }

    /// Check if selected agent has children (for UI)
    #[must_use]
    pub fn selected_has_children(&self) -> bool {
        self.selected_agent()
            .is_some_and(|a| self.storage.has_children(a.id))
    }

    /// Get depth of the selected agent (for UI)
    #[must_use]
    pub fn selected_depth(&self) -> usize {
        self.selected_agent()
            .map_or(0, |a| self.storage.depth(a.id))
    }

    /// Check if there are any running agents
    #[must_use]
    pub fn has_running_agents(&self) -> bool {
        self.storage.iter().any(|a| a.status == Status::Running)
    }

    /// Get the count of currently running agents
    #[must_use]
    pub fn running_agent_count(&self) -> usize {
        self.storage
            .iter()
            .filter(|a| a.status == Status::Running)
            .count()
    }
}

