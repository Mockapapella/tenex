//! Application state

use crate::agent::{Agent, Status, Storage};
use crate::config::Config;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// Re-export BranchInfo so it's available from app module
pub use crate::git::BranchInfo;

/// Main application state
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "app state naturally has multiple boolean flags"
)]
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

    /// Cursor position within `input_buffer` (byte offset)
    pub input_cursor: usize,

    /// Scroll position in input modal (for multiline text)
    pub input_scroll: u16,

    /// Scroll position in preview pane
    pub preview_scroll: usize,

    /// Scroll position in diff pane
    pub diff_scroll: usize,

    /// Whether preview should auto-scroll to bottom on content updates
    /// Set to false when user manually scrolls up, true when they scroll to bottom
    pub preview_follow: bool,

    /// Last error message (if any)
    pub last_error: Option<String>,

    /// Status message to display
    pub status_message: Option<String>,

    /// Cached preview content
    pub preview_content: String,

    /// Cached diff content
    pub diff_content: String,

    /// Number of child agents to spawn (for `ChildCount` mode)
    pub child_count: usize,

    /// Number of terminals spawned so far (for naming "Terminal 1", "Terminal 2", etc.)
    pub terminal_counter: usize,

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

    // === Git Operations (Push, Rename, PR) ===
    /// Agent ID for git operations (push, rename, PR)
    pub git_op_agent_id: Option<uuid::Uuid>,

    /// Branch name for operations (current or new name when renaming)
    pub git_op_branch_name: String,

    /// Original branch name (for rename operations)
    pub git_op_original_branch: String,

    /// Base branch for PR (detected from git history)
    pub git_op_base_branch: String,

    /// Whether there are unpushed commits (for PR flow)
    pub git_op_has_unpushed: bool,

    /// Whether this rename is for a root agent (includes branch rename) or sub-agent (title only)
    pub git_op_is_root_rename: bool,
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
            input_cursor: 0,
            input_scroll: 0,
            preview_scroll: 0,
            diff_scroll: 0,
            preview_follow: true,
            last_error: None,
            status_message: None,
            preview_content: String::new(),
            diff_content: String::new(),
            child_count: 3,
            terminal_counter: 0,
            spawning_under: None,
            use_plan_prompt: false,
            preview_dimensions: None,
            worktree_conflict: None,
            review_branches: Vec::new(),
            review_branch_filter: String::new(),
            review_branch_selected: 0,
            review_base_branch: None,
            git_op_agent_id: None,
            git_op_branch_name: String::new(),
            git_op_original_branch: String::new(),
            git_op_base_branch: String::new(),
            git_op_has_unpushed: false,
            git_op_is_root_rename: false,
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

    /// Reset scroll positions for both panes
    /// Preview is pinned to bottom (with follow enabled), Diff is pinned to top
    pub const fn reset_scroll(&mut self) {
        // Preview: set to max so render functions clamp to bottom of content
        self.preview_scroll = usize::MAX;
        self.preview_follow = true;
        // Diff: set to 0 to show from top
        self.diff_scroll = 0;
    }

    /// Scroll up in the active pane by the given amount
    pub fn scroll_up(&mut self, amount: usize) {
        // Normalize scroll position first (handles usize::MAX from auto-bottom)
        self.normalize_scroll();
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_sub(amount);
                // Disable auto-follow when user scrolls up
                self.preview_follow = false;
            }
            Tab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_sub(amount);
            }
        }
    }

    /// Scroll down in the active pane by the given amount
    pub fn scroll_down(&mut self, amount: usize) {
        // Normalize scroll position first (handles usize::MAX from auto-bottom)
        self.normalize_scroll();
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = self.preview_scroll.saturating_add(amount);
                // Re-enable auto-follow if we've scrolled to the bottom
                self.check_preview_follow();
            }
            Tab::Diff => {
                self.diff_scroll = self.diff_scroll.saturating_add(amount);
            }
        }
    }

    /// Check if preview scroll is at bottom and re-enable follow mode if so
    fn check_preview_follow(&mut self) {
        let preview_lines = self.preview_content.lines().count();
        let visible_height = self.preview_dimensions.map_or(20, |(_, h)| usize::from(h));
        let preview_max = preview_lines.saturating_sub(visible_height);

        if self.preview_scroll >= preview_max {
            self.preview_follow = true;
        }
    }

    /// Normalize scroll positions to be within valid range
    /// This handles the case where scroll is set to `usize::MAX` for auto-bottom
    fn normalize_scroll(&mut self) {
        let preview_lines = self.preview_content.lines().count();
        let diff_lines = self.diff_content.lines().count();

        // Use preview_dimensions if available, otherwise use a reasonable default
        let visible_height = self.preview_dimensions.map_or(20, |(_, h)| usize::from(h));

        let preview_max = preview_lines.saturating_sub(visible_height);
        let diff_max = diff_lines.saturating_sub(visible_height);

        if self.preview_scroll > preview_max {
            self.preview_scroll = preview_max;
        }
        if self.diff_scroll > diff_max {
            self.diff_scroll = diff_max;
        }
    }

    /// Scroll to the top of the active pane
    pub const fn scroll_to_top(&mut self) {
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = 0;
                self.preview_follow = false;
            }
            Tab::Diff => self.diff_scroll = 0,
        }
    }

    /// Scroll to the bottom of the active pane
    pub const fn scroll_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        let max_scroll = content_lines.saturating_sub(visible_lines);
        match self.active_tab {
            Tab::Preview => {
                self.preview_scroll = max_scroll;
                self.preview_follow = true;
            }
            Tab::Diff => self.diff_scroll = max_scroll,
        }
    }

    /// Enter a new application mode
    pub fn enter_mode(&mut self, mode: Mode) {
        debug!(new_mode = ?mode, old_mode = ?self.mode, "Entering mode");
        // Don't clear for PushRenameBranch - we pre-fill it with the branch name
        let should_clear = matches!(
            mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::Confirming(_)
                | Mode::ChildPrompt
                | Mode::Broadcasting
                | Mode::TerminalPrompt
        );
        self.mode = mode;
        if should_clear {
            self.input_buffer.clear();
            self.input_cursor = 0;
            self.input_scroll = 0;
        }
    }

    /// Exit the current mode and return to normal mode
    pub fn exit_mode(&mut self) {
        debug!(old_mode = ?self.mode, "Exiting mode");
        self.mode = Mode::Normal;
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.input_scroll = 0;
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
                | Mode::RenameBranch
                | Mode::ReconnectPrompt
                | Mode::TerminalPrompt
        ) {
            // Insert at cursor position
            self.input_buffer.insert(self.input_cursor, c);
            self.input_cursor += c.len_utf8();
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
                | Mode::RenameBranch
                | Mode::ReconnectPrompt
                | Mode::TerminalPrompt
        ) {
            // Delete character before cursor
            if self.input_cursor > 0 {
                // Find the previous character boundary
                let prev_char_boundary = self.input_buffer[..self.input_cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
                self.input_buffer.remove(prev_char_boundary);
                self.input_cursor = prev_char_boundary;
            }
        }
    }

    /// Handle delete key in text input modes (delete char at cursor)
    pub fn handle_delete(&mut self) {
        if matches!(
            self.mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::Confirming(_)
                | Mode::ChildPrompt
                | Mode::Broadcasting
                | Mode::RenameBranch
                | Mode::ReconnectPrompt
                | Mode::TerminalPrompt
        ) {
            // Delete character at cursor (if not at end)
            if self.input_cursor < self.input_buffer.len() {
                self.input_buffer.remove(self.input_cursor);
            }
        }
    }

    /// Move cursor left in text input
    pub fn input_cursor_left(&mut self) {
        if self.input_cursor > 0 {
            // Find previous character boundary
            self.input_cursor = self.input_buffer[..self.input_cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    /// Move cursor right in text input
    pub fn input_cursor_right(&mut self) {
        if self.input_cursor < self.input_buffer.len() {
            // Find next character boundary
            self.input_cursor = self.input_buffer[self.input_cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.input_buffer.len(), |(i, _)| self.input_cursor + i);
        }
    }

    /// Move cursor up one line in text input
    pub fn input_cursor_up(&mut self) {
        let text = &self.input_buffer[..self.input_cursor];
        // Find current line start and column
        let current_line_start = text.rfind('\n').map_or(0, |i| i + 1);
        let column = self.input_cursor - current_line_start;

        if current_line_start > 0 {
            // Find previous line
            let prev_text = &self.input_buffer[..current_line_start - 1];
            let prev_line_start = prev_text.rfind('\n').map_or(0, |i| i + 1);
            let prev_line_len = current_line_start - 1 - prev_line_start;

            // Move to same column or end of previous line
            self.input_cursor = prev_line_start + column.min(prev_line_len);
        }
    }

    /// Move cursor down one line in text input
    pub fn input_cursor_down(&mut self) {
        let text = &self.input_buffer;
        // Find current line start and column
        let before_cursor = &text[..self.input_cursor];
        let current_line_start = before_cursor.rfind('\n').map_or(0, |i| i + 1);
        let column = self.input_cursor - current_line_start;

        // Find next line
        if let Some(next_newline) = text[self.input_cursor..].find('\n') {
            let next_line_start = self.input_cursor + next_newline + 1;
            let next_line_end = text[next_line_start..]
                .find('\n')
                .map_or(text.len(), |i| next_line_start + i);
            let next_line_len = next_line_end - next_line_start;

            // Move to same column or end of next line
            self.input_cursor = next_line_start + column.min(next_line_len);
        }
    }

    /// Move cursor to start of line
    pub fn input_cursor_home(&mut self) {
        let text = &self.input_buffer[..self.input_cursor];
        self.input_cursor = text.rfind('\n').map_or(0, |i| i + 1);
    }

    /// Move cursor to end of line
    pub fn input_cursor_end(&mut self) {
        let text = &self.input_buffer[self.input_cursor..];
        self.input_cursor += text.find('\n').unwrap_or(text.len());
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

    /// Get the next terminal name and increment the counter
    pub fn next_terminal_name(&mut self) -> String {
        self.terminal_counter += 1;
        format!("Terminal {}", self.terminal_counter)
    }

    /// Start prompting for a terminal startup command
    pub fn start_terminal_prompt(&mut self) {
        self.enter_mode(Mode::TerminalPrompt);
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

    // === Git Operations Methods (Push, Rename, PR) ===

    /// Start the push flow - show confirmation dialog
    pub fn start_push(&mut self, agent_id: uuid::Uuid, branch_name: String) {
        self.git_op_agent_id = Some(agent_id);
        self.git_op_branch_name = branch_name;
        self.enter_mode(Mode::ConfirmPush);
    }

    /// Start the rename flow
    ///
    /// For root agents (`is_root=true`): Renames branch + agent title + tmux session
    /// For sub-agents (`is_root=false`): Renames agent title + tmux window only
    pub fn start_rename(&mut self, agent_id: uuid::Uuid, current_name: String, is_root: bool) {
        self.git_op_agent_id = Some(agent_id);
        self.git_op_original_branch = current_name.clone();
        self.git_op_branch_name.clone_from(&current_name);
        self.git_op_is_root_rename = is_root;
        self.input_buffer = current_name;
        self.input_cursor = self.input_buffer.len(); // Cursor at end
        self.enter_mode(Mode::RenameBranch);
    }

    /// Confirm the branch rename (update `branch_name` from `input_buffer`)
    pub fn confirm_rename_branch(&mut self) -> bool {
        let new_name = self.input_buffer.trim().to_string();
        if new_name.is_empty() {
            return false;
        }
        self.git_op_branch_name = new_name;
        true
    }

    /// Start the open PR flow - may show push confirmation first
    pub fn start_open_pr(
        &mut self,
        agent_id: uuid::Uuid,
        branch_name: String,
        base_branch: String,
        has_unpushed: bool,
    ) {
        self.git_op_agent_id = Some(agent_id);
        self.git_op_branch_name = branch_name;
        self.git_op_base_branch = base_branch;
        self.git_op_has_unpushed = has_unpushed;

        if has_unpushed {
            self.enter_mode(Mode::ConfirmPushForPR);
        } else {
            // No unpushed commits, will open PR directly (handled in handler)
        }
    }

    /// Clear all git operation state
    pub fn clear_git_op_state(&mut self) {
        self.git_op_agent_id = None;
        self.git_op_branch_name.clear();
        self.git_op_original_branch.clear();
        self.git_op_base_branch.clear();
        self.git_op_has_unpushed = false;
        self.git_op_is_root_rename = false;
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
    /// Preview pane is focused - keystrokes are forwarded to tmux
    PreviewFocused,
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
    /// Confirming push to remote (Y/N)
    ConfirmPush,
    /// Renaming branch (input mode) - triggered by 'r' key
    RenameBranch,
    /// Confirming push before opening PR (Y/N) - triggered by Ctrl+o
    ConfirmPushForPR,
    /// Typing a startup command for a new terminal - triggered by 'T' key
    TerminalPrompt,
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
            input_cursor,
            input_scroll,
            preview_scroll: _,
            diff_scroll: _,
            preview_follow: _,
            last_error,
            status_message,
            preview_content: _,
            diff_content: _,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions: _,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
            git_op_agent_id,
            git_op_branch_name,
            git_op_original_branch,
            git_op_base_branch,
            git_op_has_unpushed,
            git_op_is_root_rename,
            terminal_counter,
        } = App::default();

        // Start at 0 to test scroll operations
        // Need content and dimensions for scroll normalization to work
        let content = (0..100)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut app = App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            input_cursor,
            input_scroll,
            preview_scroll: 0,
            preview_follow: true,
            diff_scroll: 0,
            last_error,
            status_message,
            preview_content: content.clone(),
            diff_content: content,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions: Some((80, 20)), // 20 visible lines
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
            git_op_agent_id,
            git_op_branch_name,
            git_op_original_branch,
            git_op_base_branch,
            git_op_has_unpushed,
            git_op_is_root_rename,
            terminal_counter,
        };

        app.scroll_down(10);
        assert_eq!(app.preview_scroll, 10);

        app.scroll_up(5);
        assert_eq!(app.preview_scroll, 5);

        app.scroll_up(10);
        assert_eq!(app.preview_scroll, 0);

        app.switch_tab();
        // switch_tab resets scroll: preview to MAX (bottom), diff to 0 (top)
        assert_eq!(app.diff_scroll, 0);
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
            input_cursor,
            input_scroll,
            preview_scroll: _,
            diff_scroll,
            preview_follow,
            last_error,
            status_message,
            preview_content,
            diff_content,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
            git_op_agent_id,
            git_op_branch_name,
            git_op_original_branch,
            git_op_base_branch,
            git_op_has_unpushed,
            git_op_is_root_rename,
            terminal_counter,
        } = App::default();

        let mut app = App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            input_cursor,
            input_scroll,
            preview_scroll: 100,
            diff_scroll,
            preview_follow,
            last_error,
            status_message,
            preview_content,
            diff_content,
            child_count,
            terminal_counter,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
            git_op_agent_id,
            git_op_branch_name,
            git_op_original_branch,
            git_op_base_branch,
            git_op_has_unpushed,
            git_op_is_root_rename,
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
        // Need content and dimensions for scroll normalization to work
        let content = (0..100)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut app = App {
            active_tab: Tab::Diff,
            diff_scroll: 0,
            diff_content: content,
            preview_dimensions: Some((80, 20)), // 20 visible lines
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
        app.input_cursor = 4; // Cursor at end

        app.handle_backspace();
        assert_eq!(app.input_buffer, "tes");
        assert_eq!(app.input_cursor, 3);

        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        assert!(app.input_buffer.is_empty());
        assert_eq!(app.input_cursor, 0);

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
            input_cursor,
            input_scroll,
            preview_scroll: _,
            diff_scroll: _,
            preview_follow: _,
            last_error,
            status_message,
            preview_content,
            diff_content,
            child_count,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
            git_op_agent_id,
            git_op_branch_name,
            git_op_original_branch,
            git_op_base_branch,
            git_op_has_unpushed,
            git_op_is_root_rename,
            terminal_counter,
        } = App::default();

        let mut app = App {
            config,
            storage,
            selected,
            mode,
            active_tab,
            should_quit,
            input_buffer,
            input_cursor,
            input_scroll,
            preview_scroll: 50,
            diff_scroll: 30,
            preview_follow: false,
            last_error,
            status_message,
            preview_content,
            diff_content,
            child_count,
            terminal_counter,
            spawning_under,
            use_plan_prompt,
            preview_dimensions,
            worktree_conflict,
            review_branches,
            review_branch_filter,
            review_branch_selected,
            review_base_branch,
            git_op_agent_id,
            git_op_branch_name,
            git_op_original_branch,
            git_op_base_branch,
            git_op_has_unpushed,
            git_op_is_root_rename,
        };

        app.reset_scroll();

        // reset_scroll: preview pinned to bottom with follow enabled, diff pinned to top
        assert_eq!(app.preview_scroll, usize::MAX);
        assert_eq!(app.diff_scroll, 0);
        assert!(app.preview_follow);
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

    // === Git Operations Tests ===

    #[test]
    fn test_start_push() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();

        app.start_push(agent_id, "feature/test".to_string());

        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "feature/test");
    }

    #[test]
    fn test_start_rename_root() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();

        app.start_rename(agent_id, "my-agent".to_string(), true);

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_original_branch, "my-agent");
        assert_eq!(app.git_op_branch_name, "my-agent");
        assert_eq!(app.input_buffer, "my-agent");
        assert!(app.git_op_is_root_rename);
    }

    #[test]
    fn test_start_rename_subagent() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();

        app.start_rename(agent_id, "my-subagent".to_string(), false);

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_original_branch, "my-subagent");
        assert_eq!(app.git_op_branch_name, "my-subagent");
        assert_eq!(app.input_buffer, "my-subagent");
        assert!(!app.git_op_is_root_rename);
    }

    #[test]
    fn test_confirm_rename_branch() {
        let mut app = App {
            input_buffer: "feature/new-name".to_string(),
            git_op_branch_name: "feature/old-name".to_string(),
            ..App::default()
        };

        let result = app.confirm_rename_branch();

        assert!(result);
        assert_eq!(app.git_op_branch_name, "feature/new-name");
    }

    #[test]
    fn test_confirm_rename_branch_empty() {
        let mut app = App {
            input_buffer: "   ".to_string(),
            git_op_branch_name: "feature/old-name".to_string(),
            ..App::default()
        };

        let result = app.confirm_rename_branch();

        assert!(!result);
        assert_eq!(app.git_op_branch_name, "feature/old-name"); // Unchanged
    }

    #[test]
    fn test_start_open_pr_with_unpushed() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();

        app.start_open_pr(
            agent_id,
            "feature/test".to_string(),
            "main".to_string(),
            true, // has unpushed commits
        );

        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "feature/test");
        assert_eq!(app.git_op_base_branch, "main");
        assert!(app.git_op_has_unpushed);
    }

    #[test]
    fn test_start_open_pr_no_unpushed() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();

        app.start_open_pr(
            agent_id,
            "feature/test".to_string(),
            "main".to_string(),
            false, // no unpushed commits
        );

        // Mode is not changed when no unpushed commits (handler opens PR directly)
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "feature/test");
        assert_eq!(app.git_op_base_branch, "main");
        assert!(!app.git_op_has_unpushed);
    }

    #[test]
    fn test_clear_git_op_state() {
        let mut app = App {
            git_op_agent_id: Some(uuid::Uuid::new_v4()),
            git_op_branch_name: "feature/test".to_string(),
            git_op_original_branch: "feature/old".to_string(),
            git_op_base_branch: "main".to_string(),
            git_op_has_unpushed: true,
            git_op_is_root_rename: true,
            ..App::default()
        };

        app.clear_git_op_state();

        assert!(app.git_op_agent_id.is_none());
        assert!(app.git_op_branch_name.is_empty());
        assert!(app.git_op_original_branch.is_empty());
        assert!(app.git_op_base_branch.is_empty());
        assert!(!app.git_op_has_unpushed);
        assert!(!app.git_op_is_root_rename);
    }

    #[test]
    fn test_handle_char_rename_branch_mode() {
        let mut app = App {
            mode: Mode::RenameBranch,
            input_buffer: "feature/".to_string(),
            input_cursor: 8, // Cursor at end of "feature/"
            ..App::default()
        };

        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        assert_eq!(app.input_buffer, "feature/test");
        assert_eq!(app.input_cursor, 12);
    }

    #[test]
    fn test_handle_backspace_rename_branch_mode() {
        let mut app = App {
            mode: Mode::RenameBranch,
            input_buffer: "feature/test".to_string(),
            input_cursor: 12, // Cursor at end
            ..App::default()
        };

        app.handle_backspace();
        app.handle_backspace();

        assert_eq!(app.input_buffer, "feature/te");
        assert_eq!(app.input_cursor, 10);
    }
}
