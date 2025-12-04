//! Application state

use crate::agent::{Agent, Status, Storage};
use crate::config::Config;
use serde::{Deserialize, Serialize};

/// Main application state
#[derive(Debug)]
pub struct App {
    /// Application configuration
    pub config: Config,

    /// Agent storage
    pub storage: Storage,

    /// Currently selected agent index
    pub selected: usize,

    /// Current application mode
    pub mode: Mode,

    /// Currently active tab in the detail pane
    pub active_tab: Tab,

    /// Whether the application should quit
    pub should_quit: bool,

    /// Input buffer for text input modes
    pub input_buffer: String,

    /// Scroll position in preview pane
    pub preview_scroll: usize,

    /// Scroll position in diff pane
    pub diff_scroll: usize,

    /// Last error message (if any)
    pub last_error: Option<String>,

    /// Status message to display
    pub status_message: Option<String>,

    /// Cached preview content
    pub preview_content: String,

    /// Cached diff content
    pub diff_content: String,
}

impl App {
    /// Create a new application with the given config and storage
    #[must_use]
    pub const fn new(config: Config, storage: Storage) -> Self {
        Self {
            config,
            storage,
            selected: 0,
            mode: Mode::Normal,
            active_tab: Tab::Preview,
            should_quit: false,
            input_buffer: String::new(),
            preview_scroll: 0,
            diff_scroll: 0,
            last_error: None,
            status_message: None,
            preview_content: String::new(),
            diff_content: String::new(),
        }
    }

    /// Get the currently selected agent
    #[must_use]
    pub fn selected_agent(&self) -> Option<&Agent> {
        self.storage.get_by_index(self.selected)
    }

    /// Get a mutable reference to the currently selected agent
    pub fn selected_agent_mut(&mut self) -> Option<&mut Agent> {
        self.storage.get_by_index_mut(self.selected)
    }

    /// Move selection to the next agent
    pub fn select_next(&mut self) {
        if !self.storage.is_empty() {
            self.selected = (self.selected + 1) % self.storage.len();
            self.reset_scroll();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.storage.is_empty() {
            self.selected = self.selected.checked_sub(1).unwrap_or(self.storage.len() - 1);
            self.reset_scroll();
        }
    }

    pub fn switch_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Preview => Tab::Diff,
            Tab::Diff => Tab::Preview,
        };
        self.reset_scroll();
    }

    pub fn reset_scroll(&mut self) {
        self.preview_scroll = 0;
        self.diff_scroll = 0;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_sub(amount);
            }
            Tab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_sub(amount);
            }
        }
    }

    pub fn scroll_down(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_add(amount);
            }
            Tab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_add(amount);
            }
        }
    }

    pub fn scroll_to_top(&mut self) {
        match self.active_tab {
            Tab::Preview => self.preview_scroll = 0,
            Tab::Diff => self.diff_scroll = 0,
        }
    }

    pub fn scroll_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        let max_scroll = content_lines.saturating_sub(visible_lines);
        match self.active_tab {
            Tab::Preview => self.preview_scroll = max_scroll,
            Tab::Diff => self.diff_scroll = max_scroll,
        }
    }

    pub fn enter_mode(&mut self, mode: Mode) {
        let should_clear = matches!(
            mode,
            Mode::Creating | Mode::Prompting | Mode::Confirming(_)
        );
        self.mode = mode;
        if should_clear {
            self.input_buffer.clear();
        }
    }

    pub fn exit_mode(&mut self) {
        self.mode = Mode::Normal;
        self.input_buffer.clear();
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
    }

    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    #[must_use]
    pub fn has_running_agents(&self) -> bool {
        self.storage.iter().any(|a| a.status == Status::Running)
    }

    #[must_use]
    pub fn running_agent_count(&self) -> usize {
        self.storage
            .iter()
            .filter(|a| a.status == Status::Running)
            .count()
    }

    pub fn handle_char(&mut self, c: char) {
        if matches!(
            self.mode,
            Mode::Creating | Mode::Prompting | Mode::Confirming(_)
        ) {
            self.input_buffer.push(c);
        }
    }

    pub fn handle_backspace(&mut self) {
        if matches!(
            self.mode,
            Mode::Creating | Mode::Prompting | Mode::Confirming(_)
        ) {
            self.input_buffer.pop();
        }
    }

    pub fn validate_selection(&mut self) {
        if self.storage.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.storage.len() {
            self.selected = self.storage.len() - 1;
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(Config::default(), Storage::default())
    }
}

/// Application mode/state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal operation mode
    #[default]
    Normal,
    /// Creating a new agent (typing name)
    Creating,
    /// Typing a prompt to send to agent
    Prompting,
    /// Confirming an action
    Confirming(ConfirmAction),
    /// Showing help overlay
    Help,
    /// Scrolling through preview/diff
    Scrolling,
}

/// Actions that require confirmation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Kill an agent
    Kill,
    /// Reset all state
    Reset,
    /// Quit the application
    Quit,
}

/// Input mode for text entry
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal mode (no text input)
    #[default]
    Normal,
    /// Editing text
    Editing,
}

/// Tab in the detail pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Tab {
    /// Terminal preview
    #[default]
    Preview,
    /// Git diff view
    Diff,
}

impl std::fmt::Display for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview => write!(f, "Preview"),
            Self::Diff => write!(f, "Diff"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;
    use std::path::PathBuf;

    fn create_test_agent(title: &str) -> Agent {
        Agent::new(
            title.to_string(),
            "claude".to_string(),
            format!("muster/{title}"),
            PathBuf::from("/tmp/worktree"),
            None,
        )
    }

    #[test]
    fn test_app_new() {
        let app = App::default();
        assert_eq!(app.selected, 0);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.active_tab, Tab::Preview);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_select_next() {
        let mut app = App::default();
        app.storage.add(create_test_agent("agent1"));
        app.storage.add(create_test_agent("agent2"));
        app.storage.add(create_test_agent("agent3"));

        assert_eq!(app.selected, 0);
        app.select_next();
        assert_eq!(app.selected, 1);
        app.select_next();
        assert_eq!(app.selected, 2);
        app.select_next();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_select_prev() {
        let mut app = App::default();
        app.storage.add(create_test_agent("agent1"));
        app.storage.add(create_test_agent("agent2"));
        app.storage.add(create_test_agent("agent3"));

        assert_eq!(app.selected, 0);
        app.select_prev();
        assert_eq!(app.selected, 2);
        app.select_prev();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_select_empty_storage() {
        let mut app = App::default();
        app.select_next();
        assert_eq!(app.selected, 0);
        app.select_prev();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_switch_tab() {
        let mut app = App::default();
        assert_eq!(app.active_tab, Tab::Preview);

        app.switch_tab();
        assert_eq!(app.active_tab, Tab::Diff);

        app.switch_tab();
        assert_eq!(app.active_tab, Tab::Preview);
    }

    #[test]
    fn test_scroll() {
        let mut app = App::default();

        app.scroll_down(10);
        assert_eq!(app.preview_scroll, 10);

        app.scroll_up(5);
        assert_eq!(app.preview_scroll, 5);

        app.scroll_up(10);
        assert_eq!(app.preview_scroll, 0);

        app.switch_tab();
        app.scroll_down(20);
        assert_eq!(app.diff_scroll, 20);
    }

    #[test]
    fn test_scroll_to_top() {
        let mut app = App {
            preview_scroll: 100,
            ..Default::default()
        };
        app.scroll_to_top();
        assert_eq!(app.preview_scroll, 0);
    }

    #[test]
    fn test_scroll_to_bottom() {
        let mut app = App::default();
        app.scroll_to_bottom(100, 20);
        assert_eq!(app.preview_scroll, 80);
    }

    #[test]
    fn test_enter_exit_mode() {
        let mut app = App::default();

        app.enter_mode(Mode::Creating);
        assert_eq!(app.mode, Mode::Creating);
        assert!(app.input_buffer.is_empty());

        app.input_buffer.push_str("test");
        app.exit_mode();
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.input_buffer.is_empty());
    }

    #[test]
    fn test_error_handling() {
        let mut app = App::default();

        app.set_error("Test error");
        assert_eq!(app.last_error, Some("Test error".to_string()));

        app.clear_error();
        assert!(app.last_error.is_none());
    }

    #[test]
    fn test_status_handling() {
        let mut app = App::default();

        app.set_status("Test status");
        assert_eq!(app.status_message, Some("Test status".to_string()));

        app.clear_status();
        assert!(app.status_message.is_none());
    }

    #[test]
    fn test_selected_agent() {
        let mut app = App::default();
        assert!(app.selected_agent().is_none());

        app.storage.add(create_test_agent("test"));
        assert!(app.selected_agent().is_some());
        assert_eq!(app.selected_agent().unwrap().title, "test");
    }

    #[test]
    fn test_selected_agent_mut() {
        let mut app = App::default();
        app.storage.add(create_test_agent("test"));

        if let Some(agent) = app.selected_agent_mut() {
            agent.title = "modified".to_string();
        }

        assert_eq!(app.selected_agent().unwrap().title, "modified");
    }

    #[test]
    fn test_handle_char() {
        let mut app = App::default();

        app.handle_char('a');
        assert!(app.input_buffer.is_empty());

        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');
        assert_eq!(app.input_buffer, "test");
    }

    #[test]
    fn test_handle_backspace() {
        let mut app = App::default();
        app.enter_mode(Mode::Creating);
        app.input_buffer = "test".to_string();

        app.handle_backspace();
        assert_eq!(app.input_buffer, "tes");

        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        assert!(app.input_buffer.is_empty());

        app.handle_backspace();
        assert!(app.input_buffer.is_empty());
    }

    #[test]
    fn test_running_agent_count() {
        let mut app = App::default();
        assert_eq!(app.running_agent_count(), 0);
        assert!(!app.has_running_agents());

        let mut agent = create_test_agent("test");
        agent.set_status(Status::Running);
        app.storage.add(agent);

        assert_eq!(app.running_agent_count(), 1);
        assert!(app.has_running_agents());
    }

    #[test]
    fn test_validate_selection() {
        let mut app = App::default();
        app.storage.add(create_test_agent("test"));
        app.selected = 10;

        app.validate_selection();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_tab_display() {
        assert_eq!(format!("{}", Tab::Preview), "Preview");
        assert_eq!(format!("{}", Tab::Diff), "Diff");
    }

    #[test]
    fn test_app_mode_default() {
        assert_eq!(Mode::default(), Mode::Normal);
    }

    #[test]
    fn test_input_mode_default() {
        assert_eq!(InputMode::default(), InputMode::Normal);
    }

    #[test]
    fn test_tab_serde() {
        let tab = Tab::Preview;
        let json = serde_json::to_string(&tab).unwrap();
        let parsed: Tab = serde_json::from_str(&json).unwrap();
        assert_eq!(tab, parsed);
    }

    #[test]
    fn test_confirm_action_equality() {
        assert_eq!(ConfirmAction::Kill, ConfirmAction::Kill);
        assert_ne!(ConfirmAction::Kill, ConfirmAction::Reset);
    }

    #[test]
    fn test_reset_scroll() {
        let mut app = App {
            preview_scroll: 50,
            diff_scroll: 30,
            ..Default::default()
        };

        app.reset_scroll();

        assert_eq!(app.preview_scroll, 0);
        assert_eq!(app.diff_scroll, 0);
    }
}
