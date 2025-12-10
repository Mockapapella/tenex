//! Application state
//!
//! This module contains the main `App` struct and its sub-states,
//! organized into focused modules by domain.

mod git_op;
mod input;
mod review;
mod spawn;
mod ui;

pub use git_op::GitOpState;
pub use input::InputState;
pub use review::ReviewState;
pub use spawn::SpawnState;
pub use ui::UiState;

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
    reason = "Legacy fields for backward compatibility"
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

    // === Composed Sub-States ===
    /// Input state (buffer, cursor, scroll)
    pub input: InputState,

    /// UI state (scroll positions, preview content, dimensions)
    pub ui: UiState,

    /// Git operation state (push, rename, PR)
    pub git_op: GitOpState,

    /// Review state (branch selection)
    pub review: ReviewState,

    /// Spawn state (child agent spawning)
    pub spawn: SpawnState,

    // === Legacy fields for backward compatibility ===
    // These delegate to sub-states but provide direct access for existing code
    /// Input buffer for text input modes (delegates to `input.buffer`)
    pub input_buffer: String,

    /// Cursor position within `input_buffer` (delegates to `input.cursor`)
    pub input_cursor: usize,

    /// Scroll position in input modal (delegates to `input.scroll`)
    pub input_scroll: u16,

    /// Scroll position in preview pane (delegates to `ui.preview_scroll`)
    pub preview_scroll: usize,

    /// Scroll position in diff pane (delegates to `ui.diff_scroll`)
    pub diff_scroll: usize,

    /// Whether preview should auto-scroll (delegates to `ui.preview_follow`)
    pub preview_follow: bool,

    /// Last error message (delegates to `ui.last_error`)
    pub last_error: Option<String>,

    /// Status message to display (delegates to `ui.status_message`)
    pub status_message: Option<String>,

    /// Cached preview content (delegates to `ui.preview_content`)
    pub preview_content: String,

    /// Cached diff content (delegates to `ui.diff_content`)
    pub diff_content: String,

    /// Number of child agents to spawn (delegates to `spawn.child_count`)
    pub child_count: usize,

    /// Number of terminals spawned (delegates to `spawn.terminal_counter`)
    pub terminal_counter: usize,

    /// Agent ID to spawn children under (delegates to `spawn.spawning_under`)
    pub spawning_under: Option<uuid::Uuid>,

    /// Whether to use planning pre-prompt (delegates to `spawn.use_plan_prompt`)
    pub use_plan_prompt: bool,

    /// Preview pane dimensions (delegates to `ui.preview_dimensions`)
    pub preview_dimensions: Option<(u16, u16)>,

    /// Worktree conflict info (delegates to `spawn.worktree_conflict`)
    pub worktree_conflict: Option<WorktreeConflictInfo>,

    /// List of branches for review (delegates to `review.branches`)
    pub review_branches: Vec<BranchInfo>,

    /// Branch filter text (delegates to `review.filter`)
    pub review_branch_filter: String,

    /// Selected branch index (delegates to `review.selected`)
    pub review_branch_selected: usize,

    /// Selected base branch (delegates to `review.base_branch`)
    pub review_base_branch: Option<String>,

    /// Git op agent ID (delegates to `git_op.agent_id`)
    pub git_op_agent_id: Option<uuid::Uuid>,

    /// Git op branch name (delegates to `git_op.branch_name`)
    pub git_op_branch_name: String,

    /// Git op original branch (delegates to `git_op.original_branch`)
    pub git_op_original_branch: String,

    /// Git op base branch (delegates to `git_op.base_branch`)
    pub git_op_base_branch: String,

    /// Git op has unpushed (delegates to `git_op.has_unpushed`)
    pub git_op_has_unpushed: bool,

    /// Git op is root rename (delegates to `git_op.is_root_rename`)
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
            // Sub-states
            input: InputState::new(),
            ui: UiState::new(),
            git_op: GitOpState::new(),
            review: ReviewState::new(),
            spawn: SpawnState::new(),
            // Legacy fields
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

    /// Check if the current mode accepts text input
    ///
    /// This is used to consolidate the mode check that was previously
    /// duplicated across `handle_char`, `handle_backspace`, and `handle_delete`.
    #[must_use]
    pub const fn is_text_input_mode(&self) -> bool {
        matches!(
            self.mode,
            Mode::Creating
                | Mode::Prompting
                | Mode::ChildPrompt
                | Mode::Broadcasting
                | Mode::RenameBranch
                | Mode::ReconnectPrompt
                | Mode::TerminalPrompt
        ) || matches!(self.mode, Mode::Confirming(_))
    }

    /// Handle a character input in text input modes
    pub fn handle_char(&mut self, c: char) {
        if self.is_text_input_mode() {
            // Insert at cursor position
            self.input_buffer.insert(self.input_cursor, c);
            self.input_cursor += c.len_utf8();
        }
    }

    /// Handle backspace in text input modes
    pub fn handle_backspace(&mut self) {
        if self.is_text_input_mode() {
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
        if self.is_text_input_mode() {
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
    fn test_confirm_action_equality() {
        assert_eq!(ConfirmAction::Kill, ConfirmAction::Kill);
        assert_ne!(ConfirmAction::Kill, ConfirmAction::Reset);
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
}
