//! Application state

use crate::agent::{Agent, Status, Storage};
use crate::config::Config;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// Re-export BranchInfo so it's available from app module
pub use crate::git::BranchInfo;

/// Request to attach to a tmux session/window
#[derive(Debug, Clone)]
pub struct AttachRequest {
    /// Tmux session name
    pub session: String,
    /// Optional window index (for child agents)
    pub window_index: Option<u32>,
}

/// Main application state
#[derive(Debug)]
pub struct App {
    /// Application configuration
    pub config: Config,

    /// Agent storage
    pub storage: Storage,

    /// Currently selected agent index (in visible agents list)
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

    /// Session to attach to (when set, TUI should suspend and attach)
    pub attach_request: Option<AttachRequest>,

    /// Number of child agents to spawn (for `ChildCount` mode)
    pub child_count: usize,

    /// Agent ID to spawn children under (None = create new root with children)
    pub spawning_under: Option<uuid::Uuid>,

    /// Whether to use the planning pre-prompt when spawning children
    pub use_plan_prompt: bool,

    /// Cached preview pane dimensions (width, height) for tmux window sizing
    pub preview_dimensions: Option<(u16, u16)>,

    /// Information about a worktree conflict (when creating an agent with existing worktree)
    pub worktree_conflict: Option<WorktreeConflictInfo>,

    // === Review Feature ===
    /// List of branches for the branch selector
    pub review_branches: Vec<BranchInfo>,

    /// Current filter text for branch search
    pub review_branch_filter: String,

    /// Currently selected branch index in filtered list
    pub review_branch_selected: usize,

    /// Selected base branch for review
    pub review_base_branch: Option<String>,
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
            attach_request: None,
            child_count: 3,
            spawning_under: None,
            use_plan_prompt: false,
            preview_dimensions: None,
            worktree_conflict: None,
            review_branches: Vec::new(),
            review_branch_filter: String::new(),
            review_branch_selected: 0,
            review_base_branch: None,
        }
    }

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
        }
    }

    /// Move selection to the previous agent (in visible list)
    pub fn select_prev(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(visible_count - 1);
            self.reset_scroll();
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

    /// Reset scroll positions for both panes (to bottom)
    pub const fn reset_scroll(&mut self) {
        // Set to max so render functions clamp to bottom of content
        self.preview_scroll = usize::MAX;
        self.diff_scroll = usize::MAX;
    }

    /// Scroll up in the active pane by the given amount
    pub const fn scroll_up(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_sub(amount);
            }
            Tab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_sub(amount);
            }
        }
    }

    /// Scroll down in the active pane by the given amount
    pub const fn scroll_down(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_add(amount);
            }
            Tab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_add(amount);
            }
        }
    }

    /// Scroll to the top of the active pane
    pub const fn scroll_to_top(&mut self) {
        match self.active_tab {
            Tab::Preview => self.preview_scroll = 0,
            Tab::Diff => self.diff_scroll = 0,
        }
    }

    /// Scroll to the bottom of the active pane
    pub const fn scroll_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        let max_scroll = content_lines.saturating_sub(visible_lines);
        match self.active_tab {
            Tab::Preview => self.preview_scroll = max_scroll,
            Tab::Diff => self.diff_scroll = max_scroll,
        }
    }

    /// Enter a new application mode
    pub fn enter_mode(&mut self, mode: Mode) {
        debug!(new_mode = ?mode, old_mode = ?self.mode, "Entering mode");
        let should_clear = matches!(
            mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::Confirming(_)
                | Mode::ChildPrompt
                | Mode::Broadcasting
        );
        self.mode = mode;
        if should_clear {
            self.input_buffer.clear();
        }
    }

    /// Exit the current mode and return to normal mode
    pub fn exit_mode(&mut self) {
        debug!(old_mode = ?self.mode, "Exiting mode");
        self.mode = Mode::Normal;
        self.input_buffer.clear();
    }

    /// Set an error message and show the error modal
    pub fn set_error(&mut self, message: impl Into<String>) {
        let msg = message.into();
        warn!(error = %msg, "Application error");
        self.last_error = Some(msg.clone());
        self.mode = Mode::ErrorModal(msg);
    }

    /// Clear the current error message
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    /// Dismiss the error modal (returns to normal mode)
    pub fn dismiss_error(&mut self) {
        if matches!(self.mode, Mode::ErrorModal(_)) {
            self.mode = Mode::Normal;
        }
        self.last_error = None;
    }

    /// Set a status message to display
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
    }

    /// Clear the current status message
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Request to attach to a tmux session/window
    pub fn request_attach(&mut self, session: String, window_index: Option<u32>) {
        self.attach_request = Some(AttachRequest {
            session,
            window_index,
        });
    }

    /// Clear the attach request after attaching
    pub fn clear_attach_request(&mut self) {
        self.attach_request = None;
    }

    /// Check if there's a pending attach request
    #[must_use]
    pub const fn has_attach_request(&self) -> bool {
        self.attach_request.is_some()
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

    /// Handle a character input in text input modes
    pub fn handle_char(&mut self, c: char) {
        if matches!(
            self.mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::Confirming(_)
                | Mode::ChildPrompt
                | Mode::Broadcasting
        ) {
            self.input_buffer.push(c);
        }
    }

    /// Handle backspace in text input modes
    pub fn handle_backspace(&mut self) {
        if matches!(
            self.mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::Confirming(_)
                | Mode::ChildPrompt
                | Mode::Broadcasting
        ) {
            self.input_buffer.pop();
        }
    }

    /// Ensure the selection index is valid for the current visible agents
    pub fn validate_selection(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count == 0 {
            self.selected = 0;
        } else if self.selected >= visible_count {
            self.selected = visible_count - 1;
        }
    }

    // === Child Spawning Methods ===

    /// Increment child count (for `ChildCount` mode)
    pub const fn increment_child_count(&mut self) {
        self.child_count = self.child_count.saturating_add(1);
    }

    /// Decrement child count (minimum 1)
    pub const fn decrement_child_count(&mut self) {
        if self.child_count > 1 {
            self.child_count -= 1;
        }
    }

    /// Start spawning children under a specific agent
    pub fn start_spawning_under(&mut self, parent_id: uuid::Uuid) {
        self.spawning_under = Some(parent_id);
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = false;
        self.enter_mode(Mode::ChildCount);
    }

    /// Start spawning a new root agent with children (no plan prompt)
    pub fn start_spawning_root(&mut self) {
        self.spawning_under = None;
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = false;
        self.enter_mode(Mode::ChildCount);
    }

    /// Start spawning a new root agent with children (with planning pre-prompt)
    pub fn start_planning_swarm(&mut self) {
        self.spawning_under = None;
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = true;
        self.enter_mode(Mode::ChildCount);
    }

    /// Proceed from `ChildCount` to `ChildPrompt` mode
    pub fn proceed_to_child_prompt(&mut self) {
        self.enter_mode(Mode::ChildPrompt);
    }

    /// Toggle collapse state of the selected agent
    pub fn toggle_selected_collapse(&mut self) {
        if let Some(agent) = self.selected_agent_mut() {
            agent.collapsed = !agent.collapsed;
        }
    }

    /// Check if selected agent has children (for UI)
    #[must_use]
    pub fn selected_has_children(&self) -> bool {
        self.selected_agent()
            .is_some_and(|a| self.storage.has_children(a.id))
    }

    /// Set the preview pane dimensions for tmux window sizing
    pub const fn set_preview_dimensions(&mut self, width: u16, height: u16) {
        self.preview_dimensions = Some((width, height));
    }

    /// Get depth of the selected agent (for UI)
    #[must_use]
    pub fn selected_depth(&self) -> usize {
        self.selected_agent()
            .map_or(0, |a| self.storage.depth(a.id))
    }

    // === Review Feature Methods ===

    /// Start the review flow - show info if no agent selected, otherwise proceed to count
    pub fn start_review(&mut self, branches: Vec<BranchInfo>) {
        self.review_branches = branches;
        self.review_branch_filter.clear();
        self.review_branch_selected = 0;
        self.review_base_branch = None;
        self.child_count = 3; // Reset to default
        self.enter_mode(Mode::ReviewChildCount);
    }

    /// Show the review info modal (when no agent is selected)
    pub fn show_review_info(&mut self) {
        self.enter_mode(Mode::ReviewInfo);
    }

    /// Proceed from review count to branch selector
    pub fn proceed_to_branch_selector(&mut self) {
        self.enter_mode(Mode::BranchSelector);
    }

    /// Get filtered branches based on current filter
    #[must_use]
    pub fn filtered_review_branches(&self) -> Vec<&BranchInfo> {
        let filter_lower = self.review_branch_filter.to_lowercase();
        self.review_branches
            .iter()
            .filter(|b| filter_lower.is_empty() || b.name.to_lowercase().contains(&filter_lower))
            .collect()
    }

    /// Select next branch in filtered list
    pub fn select_next_branch(&mut self) {
        let count = self.filtered_review_branches().len();
        if count > 0 {
            self.review_branch_selected = (self.review_branch_selected + 1) % count;
        }
    }

    /// Select previous branch in filtered list
    pub fn select_prev_branch(&mut self) {
        let count = self.filtered_review_branches().len();
        if count > 0 {
            self.review_branch_selected = self
                .review_branch_selected
                .checked_sub(1)
                .unwrap_or(count - 1);
        }
    }

    /// Get the currently selected branch
    #[must_use]
    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.filtered_review_branches()
            .get(self.review_branch_selected)
            .copied()
    }

    /// Handle character input in branch filter
    pub fn handle_branch_filter_char(&mut self, c: char) {
        self.review_branch_filter.push(c);
        // Reset selection to 0 when filter changes
        self.review_branch_selected = 0;
    }

    /// Handle backspace in branch filter
    pub fn handle_branch_filter_backspace(&mut self) {
        self.review_branch_filter.pop();
        // Reset selection when filter changes
        self.review_branch_selected = 0;
    }

    /// Confirm branch selection and set `review_base_branch`
    pub fn confirm_branch_selection(&mut self) -> bool {
        if let Some(branch) = self.selected_branch() {
            self.review_base_branch = Some(branch.name.clone());
            true
        } else {
            false
        }
    }

    /// Clear all review-related state
    pub fn clear_review_state(&mut self) {
        self.review_branches.clear();
        self.review_branch_filter.clear();
        self.review_branch_selected = 0;
        self.review_base_branch = None;
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
    /// Selecting number of child agents to spawn
    ChildCount,
    /// Typing the task/prompt for child agents
    ChildPrompt,
    /// Typing a message to broadcast to agent and leaf descendants
    Broadcasting,
    /// Showing an error modal
    ErrorModal(String),
    /// Editing prompt after choosing to reconnect to existing worktree
    ReconnectPrompt,
    /// Showing info that an agent must be selected before review
    ReviewInfo,
    /// Selecting number of review agents
    ReviewChildCount,
    /// Selecting base branch for review
    BranchSelector,
}

/// Actions that require confirmation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Kill an agent
    Kill,
    /// Reset all state
    Reset,
    /// Quit the application
    Quit,
    /// Synthesize children into parent
    Synthesize,
    /// Worktree already exists - ask to reconnect or recreate
    WorktreeConflict,
}

/// Information about an existing worktree that conflicts with a new agent
#[derive(Debug, Clone)]
pub struct WorktreeConflictInfo {
    /// The title the user entered for the new agent
    pub title: String,
    /// Optional prompt for the new agent
    pub prompt: Option<String>,
    /// The generated branch name
    pub branch: String,
    /// The path to the existing worktree
    pub worktree_path: std::path::PathBuf,
    /// The branch the existing worktree is based on (if available)
    pub existing_branch: Option<String>,
    /// The commit hash of the existing worktree's HEAD (short form)
    pub existing_commit: Option<String>,
    /// The current HEAD branch that would be used for a new worktree
    pub current_branch: String,
    /// The current HEAD commit hash (short form)
    pub current_commit: String,
    /// If this is a swarm creation, the number of children to spawn
    pub swarm_child_count: Option<usize>,
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
            format!("tenex/{title}"),
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
        // Destructure default to ensure all fields are explicitly handled
        let App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            preview_scroll: _,
            diff_scroll: _,
            last_error,
            status_message,
            preview_content,
            diff_content,
            attach_request,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
        } = App::default();

        // Start at 0 to test scroll operations
        let mut app = App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            preview_scroll: 0,
            diff_scroll: 0,
            last_error,
            status_message,
            preview_content,
            diff_content,
            attach_request,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
        };

        app.scroll_down(10);
        assert_eq!(app.preview_scroll, 10);

        app.scroll_up(5);
        assert_eq!(app.preview_scroll, 5);

        app.scroll_up(10);
        assert_eq!(app.preview_scroll, 0);

        app.switch_tab();
        // switch_tab resets scroll to MAX (bottom), scroll_down saturates at MAX
        assert_eq!(app.diff_scroll, usize::MAX);
    }

    #[test]
    fn test_scroll_to_top() {
        // Destructure default to ensure all fields are explicitly handled
        let App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            preview_scroll: _,
            diff_scroll,
            last_error,
            status_message,
            preview_content,
            diff_content,
            attach_request,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
        } = App::default();

        let mut app = App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            preview_scroll: 100,
            diff_scroll,
            last_error,
            status_message,
            preview_content,
            diff_content,
            attach_request,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
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
    fn test_scroll_diff_tab() {
        let mut app = App {
            active_tab: Tab::Diff,
            diff_scroll: 0,
            ..App::default()
        };

        // Test scroll operations on Diff tab
        app.scroll_down(10);
        assert_eq!(app.diff_scroll, 10);

        app.scroll_up(5);
        assert_eq!(app.diff_scroll, 5);

        app.scroll_up(10);
        assert_eq!(app.diff_scroll, 0); // saturating_sub

        app.scroll_to_top();
        assert_eq!(app.diff_scroll, 0);

        app.scroll_to_bottom(100, 20);
        assert_eq!(app.diff_scroll, 80);
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
    fn test_selected_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = App::default();
        assert!(app.selected_agent().is_none());

        app.storage.add(create_test_agent("test"));
        assert!(app.selected_agent().is_some());
        assert_eq!(app.selected_agent().ok_or("Expected agent")?.title, "test");
        Ok(())
    }

    #[test]
    fn test_selected_agent_mut() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = App::default();
        app.storage.add(create_test_agent("test"));

        if let Some(agent) = app.selected_agent_mut() {
            agent.title = "modified".to_string();
        }

        assert_eq!(
            app.selected_agent().ok_or("Expected agent")?.title,
            "modified"
        );
        Ok(())
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
    fn test_tab_serde() -> Result<(), Box<dyn std::error::Error>> {
        let tab = Tab::Preview;
        let json = serde_json::to_string(&tab)?;
        let parsed: Tab = serde_json::from_str(&json)?;
        assert_eq!(tab, parsed);
        Ok(())
    }

    #[test]
    fn test_confirm_action_equality() {
        assert_eq!(ConfirmAction::Kill, ConfirmAction::Kill);
        assert_ne!(ConfirmAction::Kill, ConfirmAction::Reset);
    }

    #[test]
    fn test_reset_scroll() {
        // Destructure default to ensure all fields are explicitly handled
        let App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            preview_scroll: _,
            diff_scroll: _,
            last_error,
            status_message,
            preview_content,
            diff_content,
            attach_request,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
        } = App::default();

        let mut app = App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            preview_scroll: 50,
            diff_scroll: 30,
            last_error,
            status_message,
            preview_content,
            diff_content,
            attach_request,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
        };

        app.reset_scroll();

        // reset_scroll sets to max (bottom) so render functions clamp appropriately
        assert_eq!(app.preview_scroll, usize::MAX);
        assert_eq!(app.diff_scroll, usize::MAX);
    }

    #[test]
    fn test_increment_child_count() {
        let mut app = App::default();
        assert_eq!(app.child_count, 3);
        app.increment_child_count();
        assert_eq!(app.child_count, 4);
    }

    #[test]
    fn test_decrement_child_count() {
        let mut app = App::default();
        app.decrement_child_count();
        assert_eq!(app.child_count, 2);
        app.child_count = 1;
        app.decrement_child_count();
        assert_eq!(app.child_count, 1); // Minimum is 1
    }

    #[test]
    fn test_start_spawning_under() {
        let mut app = App::default();
        let id = uuid::Uuid::new_v4();
        app.start_spawning_under(id);
        assert_eq!(app.spawning_under, Some(id));
        assert_eq!(app.child_count, 3);
        assert_eq!(app.mode, Mode::ChildCount);
    }

    #[test]
    fn test_start_spawning_root() {
        let mut app = App::default();
        app.start_spawning_root();
        assert!(app.spawning_under.is_none());
        assert_eq!(app.child_count, 3);
        assert_eq!(app.mode, Mode::ChildCount);
    }

    #[test]
    fn test_proceed_to_child_prompt() {
        let mut app = App::default();
        app.proceed_to_child_prompt();
        assert_eq!(app.mode, Mode::ChildPrompt);
    }

    #[test]
    fn test_toggle_selected_collapse() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = App::default();
        let mut agent = create_test_agent("test");
        agent.collapsed = false;
        app.storage.add(agent);

        app.toggle_selected_collapse();
        assert!(app.selected_agent().ok_or("Expected agent")?.collapsed);

        app.toggle_selected_collapse();
        assert!(!app.selected_agent().ok_or("Expected agent")?.collapsed);
        Ok(())
    }

    #[test]
    fn test_selected_has_children() {
        let app = App::default();
        assert!(!app.selected_has_children());
    }

    #[test]
    fn test_selected_depth() {
        let app = App::default();
        assert_eq!(app.selected_depth(), 0);
    }

    #[test]
    fn test_set_preview_dimensions() {
        let mut app = App::default();

        // Initially None
        assert!(app.preview_dimensions.is_none());

        // Set dimensions
        app.set_preview_dimensions(100, 50);
        assert_eq!(app.preview_dimensions, Some((100, 50)));

        // Update dimensions
        app.set_preview_dimensions(80, 40);
        assert_eq!(app.preview_dimensions, Some((80, 40)));
    }

    #[test]
    fn test_dismiss_error() {
        let mut app = App {
            mode: Mode::ErrorModal("Test error".to_string()),
            last_error: Some("Test error".to_string()),
            ..App::default()
        };

        // Dismiss it
        app.dismiss_error();
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.last_error.is_none());

        // Calling dismiss_error in normal mode should be a no-op for mode
        app.dismiss_error();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_attach_request() {
        let mut app = App::default();

        // Initially no attach request
        assert!(!app.has_attach_request());
        assert!(app.attach_request.is_none());

        // Request attach without window index
        app.request_attach("test-session".to_string(), None);
        assert!(app.has_attach_request());
        assert_eq!(
            app.attach_request.as_ref().map(|r| r.session.as_str()),
            Some("test-session")
        );
        assert_eq!(
            app.attach_request.as_ref().and_then(|r| r.window_index),
            None
        );

        // Clear attach request
        app.clear_attach_request();
        assert!(!app.has_attach_request());
        assert!(app.attach_request.is_none());

        // Request attach with window index
        app.request_attach("another-session".to_string(), Some(5));
        assert!(app.has_attach_request());
        assert_eq!(
            app.attach_request.as_ref().and_then(|r| r.window_index),
            Some(5)
        );
    }

    fn create_test_branch_info(name: &str, is_remote: bool) -> crate::git::BranchInfo {
        crate::git::BranchInfo {
            name: name.to_string(),
            full_name: if is_remote {
                format!("refs/remotes/origin/{name}")
            } else {
                format!("refs/heads/{name}")
            },
            is_remote,
            remote: if is_remote {
                Some("origin".to_string())
            } else {
                None
            },
            last_commit_time: None,
        }
    }

    #[test]
    fn test_start_review() {
        let mut app = App::default();
        let branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
        ];

        app.start_review(branches);

        assert_eq!(app.mode, Mode::ReviewChildCount);
        assert_eq!(app.review_branches.len(), 2);
        assert!(app.review_branch_filter.is_empty());
        assert_eq!(app.review_branch_selected, 0);
    }

    #[test]
    fn test_show_review_info() {
        let mut app = App::default();
        app.show_review_info();
        assert_eq!(app.mode, Mode::ReviewInfo);
    }

    #[test]
    fn test_proceed_to_branch_selector() {
        let mut app = App {
            mode: Mode::ReviewChildCount,
            ..App::default()
        };
        app.proceed_to_branch_selector();
        assert_eq!(app.mode, Mode::BranchSelector);
    }

    #[test]
    fn test_filtered_review_branches_no_filter() {
        let app = App {
            review_branches: vec![
                create_test_branch_info("main", false),
                create_test_branch_info("feature", false),
                create_test_branch_info("develop", false),
            ],
            ..App::default()
        };

        let filtered = app.filtered_review_branches();
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_filtered_review_branches_with_filter() {
        let app = App {
            review_branches: vec![
                create_test_branch_info("main", false),
                create_test_branch_info("feature", false),
                create_test_branch_info("main", true),
            ],
            review_branch_filter: "main".to_string(),
            ..App::default()
        };

        let filtered = app.filtered_review_branches();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_review_branches_case_insensitive() {
        let app = App {
            review_branches: vec![
                create_test_branch_info("Main", false),
                create_test_branch_info("MAIN", true),
            ],
            review_branch_filter: "main".to_string(),
            ..App::default()
        };

        let filtered = app.filtered_review_branches();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_select_next_branch() {
        let mut app = App {
            review_branches: vec![
                create_test_branch_info("branch1", false),
                create_test_branch_info("branch2", false),
                create_test_branch_info("branch3", false),
            ],
            ..App::default()
        };

        assert_eq!(app.review_branch_selected, 0);
        app.select_next_branch();
        assert_eq!(app.review_branch_selected, 1);
        app.select_next_branch();
        assert_eq!(app.review_branch_selected, 2);
        // Wrap around
        app.select_next_branch();
        assert_eq!(app.review_branch_selected, 0);
    }

    #[test]
    fn test_select_prev_branch() {
        let mut app = App {
            review_branches: vec![
                create_test_branch_info("branch1", false),
                create_test_branch_info("branch2", false),
                create_test_branch_info("branch3", false),
            ],
            ..App::default()
        };

        assert_eq!(app.review_branch_selected, 0);
        // Wrap to end
        app.select_prev_branch();
        assert_eq!(app.review_branch_selected, 2);
        app.select_prev_branch();
        assert_eq!(app.review_branch_selected, 1);
    }

    #[test]
    fn test_select_branch_empty() {
        let mut app = App {
            review_branches: vec![],
            ..App::default()
        };

        // Should not panic with empty list
        app.select_next_branch();
        assert_eq!(app.review_branch_selected, 0);
        app.select_prev_branch();
        assert_eq!(app.review_branch_selected, 0);
    }

    #[test]
    fn test_selected_branch() {
        let mut app = App {
            review_branches: vec![
                create_test_branch_info("main", false),
                create_test_branch_info("feature", false),
            ],
            ..App::default()
        };

        let branch = app.selected_branch();
        assert!(branch.is_some());
        assert_eq!(branch.map(|b| b.name.as_str()), Some("main"));

        app.review_branch_selected = 1;
        let branch = app.selected_branch();
        assert_eq!(branch.map(|b| b.name.as_str()), Some("feature"));
    }

    #[test]
    fn test_selected_branch_empty() {
        let app = App::default();
        assert!(app.selected_branch().is_none());
    }

    #[test]
    fn test_handle_branch_filter_char() {
        let mut app = App {
            review_branches: vec![create_test_branch_info("main", false)],
            ..App::default()
        };

        app.handle_branch_filter_char('m');
        assert_eq!(app.review_branch_filter, "m");
        assert_eq!(app.review_branch_selected, 0);

        app.handle_branch_filter_char('a');
        assert_eq!(app.review_branch_filter, "ma");
    }

    #[test]
    fn test_handle_branch_filter_backspace() {
        let mut app = App {
            review_branch_filter: "main".to_string(),
            ..App::default()
        };

        app.handle_branch_filter_backspace();
        assert_eq!(app.review_branch_filter, "mai");
        assert_eq!(app.review_branch_selected, 0);

        app.handle_branch_filter_backspace();
        app.handle_branch_filter_backspace();
        app.handle_branch_filter_backspace();
        assert!(app.review_branch_filter.is_empty());

        // Backspace on empty should not panic
        app.handle_branch_filter_backspace();
        assert!(app.review_branch_filter.is_empty());
    }

    #[test]
    fn test_confirm_branch_selection() {
        let mut app = App {
            review_branches: vec![
                create_test_branch_info("main", false),
                create_test_branch_info("develop", false),
            ],
            review_branch_selected: 1,
            ..App::default()
        };

        let result = app.confirm_branch_selection();
        assert!(result);
        assert_eq!(app.review_base_branch, Some("develop".to_string()));
    }

    #[test]
    fn test_confirm_branch_selection_empty() {
        let mut app = App::default();

        let result = app.confirm_branch_selection();
        assert!(!result);
        assert!(app.review_base_branch.is_none());
    }

    #[test]
    fn test_clear_review_state() {
        let mut app = App {
            review_branches: vec![create_test_branch_info("main", false)],
            review_branch_filter: "filter".to_string(),
            review_branch_selected: 5,
            review_base_branch: Some("main".to_string()),
            ..App::default()
        };

        app.clear_review_state();

        assert!(app.review_branches.is_empty());
        assert!(app.review_branch_filter.is_empty());
        assert_eq!(app.review_branch_selected, 0);
        assert!(app.review_base_branch.is_none());
    }
}
