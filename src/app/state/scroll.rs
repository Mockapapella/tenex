//! Preview/diff scrolling helpers.

use super::{App, Tab};

impl App {
    /// Reset scroll positions for both panes
    /// Preview is pinned to bottom (with follow enabled), Diff is pinned to top
    pub const fn reset_scroll(&mut self) {
        self.ui.reset_scroll();
    }

    /// Scroll up in the active pane by the given amount
    pub fn scroll_up(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => self.ui.scroll_preview_up(amount),
            Tab::Diff => self.ui.scroll_diff_up(amount),
        }
    }

    /// Scroll down in the active pane by the given amount
    pub fn scroll_down(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => self.ui.scroll_preview_down(amount),
            Tab::Diff => self.ui.scroll_diff_down(amount),
        }
    }

    /// Scroll to the top of the active pane
    pub const fn scroll_to_top(&mut self) {
        match self.active_tab {
            Tab::Preview => self.ui.preview_to_top(),
            Tab::Diff => self.ui.diff_to_top(),
        }
    }

    /// Scroll to the bottom of the active pane
    pub const fn scroll_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        match self.active_tab {
            Tab::Preview => self.ui.preview_to_bottom(content_lines, visible_lines),
            Tab::Diff => self.ui.diff_to_bottom(content_lines, visible_lines),
        }
    }

    /// Set the preview pane dimensions for mux window sizing
    pub const fn set_preview_dimensions(&mut self, width: u16, height: u16) {
        self.ui.set_preview_dimensions(width, height);
    }
}
