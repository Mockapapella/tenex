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

use super::Settings;

// Re-export BranchInfo so it's available from app module
pub use crate::git::BranchInfo;

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

    /// User settings (persistent preferences)
    pub settings: Settings,

    /// Whether the terminal supports the keyboard enhancement protocol
    pub keyboard_enhancement_supported: bool,
}

impl App {
    /// Create a new application with the given config, storage, and settings
    #[must_use]
    pub const fn new(
        config: Config,
        storage: Storage,
        settings: Settings,
        keyboard_enhancement_supported: bool,
    ) -> Self {
        Self {
            config,
            storage,
            selected: 0,
            mode: Mode::Normal,
            active_tab: Tab::Preview,
            should_quit: false,
            input: InputState::new(),
            ui: UiState::new(),
            git_op: GitOpState::new(),
            review: ReviewState::new(),
            spawn: SpawnState::new(),
            settings,
            keyboard_enhancement_supported,
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
        self.ui.preview_scroll = usize::MAX;
        self.ui.preview_follow = true;
        // Diff: set to 0 to show from top
        self.ui.diff_scroll = 0;
    }

    /// Scroll up in the active pane by the given amount
    pub fn scroll_up(&mut self, amount: usize) {
        // Normalize scroll position first (handles usize::MAX from auto-bottom)
        self.normalize_scroll();
        match self.active_tab {
            Tab::Preview => {
                self.ui.preview_scroll = self.ui.preview_scroll.saturating_sub(amount);
                // Disable auto-follow when user scrolls up
                self.ui.preview_follow = false;
            }
            Tab::Diff => {
                self.ui.diff_scroll = self.ui.diff_scroll.saturating_sub(amount);
            }
        }
    }

    /// Scroll down in the active pane by the given amount
    pub fn scroll_down(&mut self, amount: usize) {
        // Normalize scroll position first (handles usize::MAX from auto-bottom)
        self.normalize_scroll();
        match self.active_tab {
            Tab::Preview => {
                self.ui.preview_scroll = self.ui.preview_scroll.saturating_add(amount);
                // Re-enable auto-follow if we've scrolled to the bottom
                self.check_preview_follow();
            }
            Tab::Diff => {
                self.ui.diff_scroll = self.ui.diff_scroll.saturating_add(amount);
            }
        }
    }

    /// Check if preview scroll is at bottom and re-enable follow mode if so
    fn check_preview_follow(&mut self) {
        let preview_lines = self.ui.preview_content.lines().count();
        let visible_height = self
            .ui
            .preview_dimensions
            .map_or(20, |(_, h)| usize::from(h));
        let preview_max = preview_lines.saturating_sub(visible_height);

        if self.ui.preview_scroll >= preview_max {
            self.ui.preview_follow = true;
        }
    }

    /// Normalize scroll positions to be within valid range
    /// This handles the case where scroll is set to `usize::MAX` for auto-bottom
    fn normalize_scroll(&mut self) {
        let preview_lines = self.ui.preview_content.lines().count();
        let diff_lines = self.ui.diff_content.lines().count();

        // Use preview_dimensions if available, otherwise use a reasonable default
        let visible_height = self
            .ui
            .preview_dimensions
            .map_or(20, |(_, h)| usize::from(h));

        let preview_max = preview_lines.saturating_sub(visible_height);
        let diff_max = diff_lines.saturating_sub(visible_height);

        if self.ui.preview_scroll > preview_max {
            self.ui.preview_scroll = preview_max;
        }
        if self.ui.diff_scroll > diff_max {
            self.ui.diff_scroll = diff_max;
        }
    }

    /// Scroll to the top of the active pane
    pub const fn scroll_to_top(&mut self) {
        match self.active_tab {
            Tab::Preview => {
                self.ui.preview_scroll = 0;
                self.ui.preview_follow = false;
            }
            Tab::Diff => self.ui.diff_scroll = 0,
        }
    }

    /// Scroll to the bottom of the active pane
    pub const fn scroll_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        let max_scroll = content_lines.saturating_sub(visible_lines);
        match self.active_tab {
            Tab::Preview => {
                self.ui.preview_scroll = max_scroll;
                self.ui.preview_follow = true;
            }
            Tab::Diff => self.ui.diff_scroll = max_scroll,
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
            self.input.buffer.clear();
            self.input.cursor = 0;
            self.input.scroll = 0;
        }
    }

    /// Exit the current mode and return to normal mode
    pub fn exit_mode(&mut self) {
        debug!(old_mode = ?self.mode, "Exiting mode");
        self.mode = Mode::Normal;
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.input.scroll = 0;
    }

    /// Set an error message and show the error modal
    pub fn set_error(&mut self, message: impl Into<String>) {
        let msg = message.into();
        warn!(error = %msg, "Application error");
        self.ui.last_error = Some(msg.clone());
        self.mode = Mode::ErrorModal(msg);
    }

    /// Clear the current error message
    pub fn clear_error(&mut self) {
        self.ui.last_error = None;
    }

    /// Dismiss the error modal (returns to normal mode)
    pub fn dismiss_error(&mut self) {
        if matches!(self.mode, Mode::ErrorModal(_)) {
            self.mode = Mode::Normal;
        }
        self.ui.last_error = None;
    }

    /// Set a status message to display
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.ui.status_message = Some(message.into());
    }

    /// Clear the current status message
    pub fn clear_status(&mut self) {
        self.ui.status_message = None;
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
            self.input.buffer.insert(self.input.cursor, c);
            self.input.cursor += c.len_utf8();
        }
    }

    /// Handle backspace in text input modes
    pub fn handle_backspace(&mut self) {
        if self.is_text_input_mode() {
            // Delete character before cursor
            if self.input.cursor > 0 {
                // Find the previous character boundary
                let prev_char_boundary = self.input.buffer[..self.input.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
                self.input.buffer.remove(prev_char_boundary);
                self.input.cursor = prev_char_boundary;
            }
        }
    }

    /// Handle delete key in text input modes (delete char at cursor)
    pub fn handle_delete(&mut self) {
        if self.is_text_input_mode() {
            // Delete character at cursor (if not at end)
            if self.input.cursor < self.input.buffer.len() {
                self.input.buffer.remove(self.input.cursor);
            }
        }
    }

    /// Move cursor left in text input
    pub fn input_cursor_left(&mut self) {
        if self.input.cursor > 0 {
            // Find previous character boundary
            self.input.cursor = self.input.buffer[..self.input.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    /// Move cursor right in text input
    pub fn input_cursor_right(&mut self) {
        if self.input.cursor < self.input.buffer.len() {
            // Find next character boundary
            self.input.cursor = self.input.buffer[self.input.cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.input.buffer.len(), |(i, _)| self.input.cursor + i);
        }
    }

    /// Move cursor up one line in text input
    pub fn input_cursor_up(&mut self) {
        let text = &self.input.buffer[..self.input.cursor];
        // Find current line start and column
        let current_line_start = text.rfind('\n').map_or(0, |i| i + 1);
        let column = self.input.cursor - current_line_start;

        if current_line_start > 0 {
            // Find previous line
            let prev_text = &self.input.buffer[..current_line_start - 1];
            let prev_line_start = prev_text.rfind('\n').map_or(0, |i| i + 1);
            let prev_line_len = current_line_start - 1 - prev_line_start;

            // Move to same column or end of previous line
            self.input.cursor = prev_line_start + column.min(prev_line_len);
        }
    }

    /// Move cursor down one line in text input
    pub fn input_cursor_down(&mut self) {
        let text = &self.input.buffer;
        // Find current line start and column
        let before_cursor = &text[..self.input.cursor];
        let current_line_start = before_cursor.rfind('\n').map_or(0, |i| i + 1);
        let column = self.input.cursor - current_line_start;

        // Find next line
        if let Some(next_newline) = text[self.input.cursor..].find('\n') {
            let next_line_start = self.input.cursor + next_newline + 1;
            let next_line_end = text[next_line_start..]
                .find('\n')
                .map_or(text.len(), |i| next_line_start + i);
            let next_line_len = next_line_end - next_line_start;

            // Move to same column or end of next line
            self.input.cursor = next_line_start + column.min(next_line_len);
        }
    }

    /// Move cursor to start of line
    pub fn input_cursor_home(&mut self) {
        let text = &self.input.buffer[..self.input.cursor];
        self.input.cursor = text.rfind('\n').map_or(0, |i| i + 1);
    }

    /// Move cursor to end of line
    pub fn input_cursor_end(&mut self) {
        let text = &self.input.buffer[self.input.cursor..];
        self.input.cursor += text.find('\n').unwrap_or(text.len());
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
        self.spawn.child_count = self.spawn.child_count.saturating_add(1);
    }

    /// Decrement child count (minimum 1)
    pub const fn decrement_child_count(&mut self) {
        if self.spawn.child_count > 1 {
            self.spawn.child_count -= 1;
        }
    }

    /// Start spawning children under a specific agent
    pub fn start_spawning_under(&mut self, parent_id: uuid::Uuid) {
        self.spawn.spawning_under = Some(parent_id);
        self.spawn.child_count = 3; // Reset to default
        self.spawn.use_plan_prompt = false;
        self.enter_mode(Mode::ChildCount);
    }

    /// Start spawning a new root agent with children (no plan prompt)
    pub fn start_spawning_root(&mut self) {
        self.spawn.spawning_under = None;
        self.spawn.child_count = 3; // Reset to default
        self.spawn.use_plan_prompt = false;
        self.enter_mode(Mode::ChildCount);
    }

    /// Start spawning a new root agent with children (with planning pre-prompt)
    pub fn start_planning_swarm(&mut self) {
        self.spawn.spawning_under = None;
        self.spawn.child_count = 3; // Reset to default
        self.spawn.use_plan_prompt = true;
        self.enter_mode(Mode::ChildCount);
    }

    /// Proceed from `ChildCount` to `ChildPrompt` mode
    pub fn proceed_to_child_prompt(&mut self) {
        self.enter_mode(Mode::ChildPrompt);
    }

    /// Get the next terminal name and increment the counter
    pub fn next_terminal_name(&mut self) -> String {
        self.spawn.terminal_counter += 1;
        format!("Terminal {}", self.spawn.terminal_counter)
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
        self.ui.preview_dimensions = Some((width, height));
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
        self.review.branches = branches;
        self.review.filter.clear();
        self.review.selected = 0;
        self.review.base_branch = None;
        self.spawn.child_count = 3; // Reset to default
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
        let filter_lower = self.review.filter.to_lowercase();
        self.review
            .branches
            .iter()
            .filter(|b| filter_lower.is_empty() || b.name.to_lowercase().contains(&filter_lower))
            .collect()
    }

    /// Select next branch in filtered list
    pub fn select_next_branch(&mut self) {
        let count = self.filtered_review_branches().len();
        if count > 0 {
            self.review.selected = (self.review.selected + 1) % count;
        }
    }

    /// Select previous branch in filtered list
    pub fn select_prev_branch(&mut self) {
        let count = self.filtered_review_branches().len();
        if count > 0 {
            self.review.selected = self.review.selected.checked_sub(1).unwrap_or(count - 1);
        }
    }

    /// Get the currently selected branch
    #[must_use]
    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.filtered_review_branches()
            .get(self.review.selected)
            .copied()
    }

    /// Handle character input in branch filter
    pub fn handle_branch_filter_char(&mut self, c: char) {
        self.review.filter.push(c);
        // Reset selection to 0 when filter changes
        self.review.selected = 0;
    }

    /// Handle backspace in branch filter
    pub fn handle_branch_filter_backspace(&mut self) {
        self.review.filter.pop();
        // Reset selection when filter changes
        self.review.selected = 0;
    }

    /// Confirm branch selection and set `review_base_branch`
    pub fn confirm_branch_selection(&mut self) -> bool {
        if let Some(branch) = self.selected_branch() {
            self.review.base_branch = Some(branch.name.clone());
            true
        } else {
            false
        }
    }

    /// Clear all review-related state
    pub fn clear_review_state(&mut self) {
        self.review.branches.clear();
        self.review.filter.clear();
        self.review.selected = 0;
        self.review.base_branch = None;
    }

    // === Git Operations Methods (Push, Rename, PR) ===

    /// Start the push flow - show confirmation dialog
    pub fn start_push(&mut self, agent_id: uuid::Uuid, branch_name: String) {
        self.git_op.agent_id = Some(agent_id);
        self.git_op.branch_name = branch_name;
        self.enter_mode(Mode::ConfirmPush);
    }

    /// Start the rename flow
    ///
    /// For root agents (`is_root=true`): Renames branch + agent title + tmux session
    /// For sub-agents (`is_root=false`): Renames agent title + tmux window only
    pub fn start_rename(&mut self, agent_id: uuid::Uuid, current_name: String, is_root: bool) {
        self.git_op.agent_id = Some(agent_id);
        self.git_op.original_branch = current_name.clone();
        self.git_op.branch_name.clone_from(&current_name);
        self.git_op.is_root_rename = is_root;
        self.input.buffer = current_name;
        self.input.cursor = self.input.buffer.len(); // Cursor at end
        self.enter_mode(Mode::RenameBranch);
    }

    /// Confirm the branch rename (update `branch_name` from `input_buffer`)
    pub fn confirm_rename_branch(&mut self) -> bool {
        let new_name = self.input.buffer.trim().to_string();
        if new_name.is_empty() {
            return false;
        }
        self.git_op.branch_name = new_name;
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
        self.git_op.agent_id = Some(agent_id);
        self.git_op.branch_name = branch_name;
        self.git_op.base_branch = base_branch;
        self.git_op.has_unpushed = has_unpushed;

        if has_unpushed {
            self.enter_mode(Mode::ConfirmPushForPR);
        } else {
            // No unpushed commits, will open PR directly (handled in handler)
        }
    }

    /// Clear all git operation state
    pub fn clear_git_op_state(&mut self) {
        self.git_op.clear();
    }

    /// Start the rebase flow - show branch selector to choose target branch
    pub fn start_rebase(
        &mut self,
        agent_id: uuid::Uuid,
        current_branch: String,
        branches: Vec<BranchInfo>,
    ) {
        self.git_op.start_rebase(agent_id, current_branch);
        self.review.branches = branches;
        self.review.filter.clear();
        self.review.selected = 0;
        self.review.base_branch = None;
        self.enter_mode(Mode::RebaseBranchSelector);
    }

    /// Start the merge flow - show branch selector to choose source branch
    pub fn start_merge(
        &mut self,
        agent_id: uuid::Uuid,
        current_branch: String,
        branches: Vec<BranchInfo>,
    ) {
        self.git_op.start_merge(agent_id, current_branch);
        self.review.branches = branches;
        self.review.filter.clear();
        self.review.selected = 0;
        self.review.base_branch = None;
        self.enter_mode(Mode::MergeBranchSelector);
    }

    /// Confirm branch selection for rebase/merge and set target branch
    pub fn confirm_rebase_merge_branch(&mut self) -> bool {
        if let Some(branch) = self.selected_branch() {
            self.git_op.set_target_branch(branch.name.clone());
            true
        } else {
            false
        }
    }

    /// Show success modal with message
    pub fn show_success(&mut self, message: impl Into<String>) {
        self.mode = Mode::SuccessModal(message.into());
    }

    /// Dismiss success modal
    pub fn dismiss_success(&mut self) {
        if matches!(self.mode, Mode::SuccessModal(_)) {
            self.mode = Mode::Normal;
        }
    }

    /// Check if keyboard remap prompt should be shown at startup
    /// Returns true if terminal doesn't support enhancement AND user hasn't been asked yet
    #[must_use]
    pub const fn should_show_keyboard_remap_prompt(&self) -> bool {
        !self.keyboard_enhancement_supported && !self.settings.keyboard_remap_asked
    }

    /// Show the keyboard remap prompt modal
    pub fn show_keyboard_remap_prompt(&mut self) {
        self.mode = Mode::KeyboardRemapPrompt;
    }

    /// Accept the keyboard remap (Ctrl+M -> Ctrl+N)
    pub fn accept_keyboard_remap(&mut self) {
        if let Err(e) = self.settings.enable_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        self.mode = Mode::Normal;
    }

    /// Decline the keyboard remap
    pub fn decline_keyboard_remap(&mut self) {
        if let Err(e) = self.settings.decline_merge_remap() {
            warn!("Failed to save keyboard remap setting: {}", e);
        }
        self.mode = Mode::Normal;
    }

    /// Check if merge key should use Ctrl+N instead of Ctrl+M
    #[must_use]
    pub const fn is_merge_key_remapped(&self) -> bool {
        self.settings.merge_key_remapped
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        )
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
    /// Selecting branch to rebase onto - triggered by Alt+r
    RebaseBranchSelector,
    /// Selecting branch to merge from - triggered by Alt+m
    MergeBranchSelector,
    /// Showing success modal after git operation
    SuccessModal(String),
    /// Prompting user to remap Ctrl+M due to terminal incompatibility
    KeyboardRemapPrompt,
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
        assert!(app.input.buffer.is_empty());

        app.input.buffer.push_str("test");
        app.exit_mode();
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.input.buffer.is_empty());
    }

    #[test]
    fn test_error_handling() {
        let mut app = App::default();

        app.set_error("Test error");
        assert_eq!(app.ui.last_error, Some("Test error".to_string()));

        app.clear_error();
        assert!(app.ui.last_error.is_none());
    }

    #[test]
    fn test_status_handling() {
        let mut app = App::default();

        app.set_status("Test status");
        assert_eq!(app.ui.status_message, Some("Test status".to_string()));

        app.clear_status();
        assert!(app.ui.status_message.is_none());
    }

    #[test]
    fn test_handle_char() {
        let mut app = App::default();

        app.handle_char('a');
        assert!(app.input.buffer.is_empty());

        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');
        assert_eq!(app.input.buffer, "test");
    }

    #[test]
    fn test_handle_backspace() {
        let mut app = App::default();
        app.enter_mode(Mode::Creating);
        app.input.buffer = "test".to_string();
        app.input.cursor = 4; // Cursor at end

        app.handle_backspace();
        assert_eq!(app.input.buffer, "tes");
        assert_eq!(app.input.cursor, 3);

        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        assert!(app.input.buffer.is_empty());
        assert_eq!(app.input.cursor, 0);

        app.handle_backspace();
        assert!(app.input.buffer.is_empty());
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
        assert_eq!(app.spawn.child_count, 3);
        app.increment_child_count();
        assert_eq!(app.spawn.child_count, 4);
    }

    #[test]
    fn test_decrement_child_count() {
        let mut app = App::default();
        app.decrement_child_count();
        assert_eq!(app.spawn.child_count, 2);
        app.spawn.child_count = 1;
        app.decrement_child_count();
        assert_eq!(app.spawn.child_count, 1); // Minimum is 1
    }

    #[test]
    fn test_start_spawning_under() {
        let mut app = App::default();
        let id = uuid::Uuid::new_v4();
        app.start_spawning_under(id);
        assert_eq!(app.spawn.spawning_under, Some(id));
        assert_eq!(app.spawn.child_count, 3);
        assert_eq!(app.mode, Mode::ChildCount);
    }

    #[test]
    fn test_start_spawning_root() {
        let mut app = App::default();
        app.start_spawning_root();
        assert!(app.spawn.spawning_under.is_none());
        assert_eq!(app.spawn.child_count, 3);
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
            ..App::default()
        };
        app.ui.last_error = Some("Test error".to_string());

        // Dismiss it
        app.dismiss_error();
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.ui.last_error.is_none());

        // Calling dismiss_error in normal mode should be a no-op for mode
        app.dismiss_error();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_selected_agent_mut() {
        let mut app = App::default();
        // No agents - should return None
        assert!(app.selected_agent_mut().is_none());

        // Add an agent
        app.storage.add(create_test_agent("test"));
        let agent = app.selected_agent_mut();
        assert!(agent.is_some());
        if let Some(a) = agent {
            a.collapsed = true;
        }

        // Verify the change persisted
        assert!(app.selected_agent().is_some_and(|a| a.collapsed));
    }

    #[test]
    fn test_handle_delete() {
        let mut app = App::default();
        app.enter_mode(Mode::Creating);
        app.input.buffer = "test".to_string();
        app.input.cursor = 2; // Cursor at 'st'

        app.handle_delete();
        assert_eq!(app.input.buffer, "tet");
        assert_eq!(app.input.cursor, 2);

        // Delete at end does nothing
        app.input.cursor = 3;
        app.handle_delete();
        assert_eq!(app.input.buffer, "tet");
    }

    #[test]
    fn test_input_cursor_left_right() {
        let mut app = App::default();
        app.input.buffer = "hello".to_string();
        app.input.cursor = 3;

        app.input_cursor_left();
        assert_eq!(app.input.cursor, 2);

        app.input_cursor_right();
        assert_eq!(app.input.cursor, 3);

        // At start, left does nothing
        app.input.cursor = 0;
        app.input_cursor_left();
        assert_eq!(app.input.cursor, 0);

        // At end, right does nothing
        app.input.cursor = 5;
        app.input_cursor_right();
        assert_eq!(app.input.cursor, 5);
    }

    #[test]
    fn test_input_cursor_up_down() {
        let mut app = App::default();
        app.input.buffer = "line1\nline2\nline3".to_string();

        // Start at end of line2
        app.input.cursor = 11; // "line1\nline2" length

        // Move up to line1
        app.input_cursor_up();
        assert_eq!(app.input.cursor, 5); // End of "line1"

        // Move up from line1 does nothing (already at first line)
        app.input_cursor_up();
        assert_eq!(app.input.cursor, 5);

        // Move down to line2
        app.input_cursor_down();
        assert_eq!(app.input.cursor, 11); // End of "line1\nline2"

        // Move down to line3
        app.input_cursor_down();
        assert_eq!(app.input.cursor, 17); // End of string

        // Move down from last line does nothing
        app.input_cursor_down();
        assert_eq!(app.input.cursor, 17);
    }

    #[test]
    fn test_input_cursor_home_end() {
        let mut app = App::default();
        app.input.buffer = "line1\nline2\nline3".to_string();
        app.input.cursor = 8; // Middle of "line2"

        app.input_cursor_home();
        assert_eq!(app.input.cursor, 6); // Start of "line2"

        app.input_cursor_end();
        assert_eq!(app.input.cursor, 11); // End of "line2"

        // Test on first line
        app.input.cursor = 3;
        app.input_cursor_home();
        assert_eq!(app.input.cursor, 0);
    }

    #[test]
    fn test_scroll_methods() {
        let mut app = App::default();
        app.ui.preview_content = "line1\nline2\nline3\nline4\nline5".to_string();
        app.ui.diff_content = "diff1\ndiff2\ndiff3".to_string();
        app.ui.preview_dimensions = Some((80, 2));

        // Test scroll_up in Preview mode
        app.ui.preview_scroll = 2;
        app.scroll_up(1);
        assert_eq!(app.ui.preview_scroll, 1);
        assert!(!app.ui.preview_follow);

        // Test scroll_down in Preview mode
        app.scroll_down(1);
        assert_eq!(app.ui.preview_scroll, 2);

        // Test scroll_to_top in Preview mode
        app.scroll_to_top();
        assert_eq!(app.ui.preview_scroll, 0);
        assert!(!app.ui.preview_follow);

        // Test scroll_to_bottom in Preview mode
        app.scroll_to_bottom(5, 2);
        assert_eq!(app.ui.preview_scroll, 3);
        assert!(app.ui.preview_follow);

        // Switch to Diff tab and test
        app.active_tab = Tab::Diff;
        app.ui.diff_scroll = 2;
        app.scroll_up(1);
        // normalize_scroll clamps to max (1 for 3 lines with 2 visible)
        assert!(app.ui.diff_scroll <= 1);

        app.ui.diff_scroll = 0;
        app.scroll_down(1);
        assert_eq!(app.ui.diff_scroll, 1);

        app.scroll_to_top();
        assert_eq!(app.ui.diff_scroll, 0);

        app.scroll_to_bottom(3, 2);
        assert_eq!(app.ui.diff_scroll, 1);
    }

    #[test]
    fn test_start_planning_swarm() {
        let mut app = App::default();
        app.start_planning_swarm();
        assert!(app.spawn.spawning_under.is_none());
        assert_eq!(app.spawn.child_count, 3);
        assert!(app.spawn.use_plan_prompt);
        assert_eq!(app.mode, Mode::ChildCount);
    }

    #[test]
    fn test_toggle_selected_collapse() {
        let mut app = App::default();
        app.storage.add(create_test_agent("test"));

        // Initially collapsed (default is true)
        assert!(app.selected_agent().is_some_and(|a| a.collapsed));

        app.toggle_selected_collapse();
        assert!(app.selected_agent().is_some_and(|a| !a.collapsed));

        app.toggle_selected_collapse();
        assert!(app.selected_agent().is_some_and(|a| a.collapsed));
    }

    #[test]
    fn test_selected_has_children() {
        let mut app = App::default();
        let parent = create_test_agent("parent");
        let parent_id = parent.id;
        app.storage.add(parent);

        // No children initially
        assert!(!app.selected_has_children());

        // Add a child
        let mut child = create_test_agent("child");
        child.parent_id = Some(parent_id);
        app.storage.add(child);

        // Now has children
        assert!(app.selected_has_children());
    }

    #[test]
    fn test_set_preview_dimensions() {
        let mut app = App::default();
        assert!(app.ui.preview_dimensions.is_none());

        app.set_preview_dimensions(100, 50);
        assert_eq!(app.ui.preview_dimensions, Some((100, 50)));
    }

    #[test]
    fn test_selected_depth() {
        let mut app = App::default();
        // No agent selected
        assert_eq!(app.selected_depth(), 0);

        // Root agent has depth 0
        app.storage.add(create_test_agent("root"));
        assert_eq!(app.selected_depth(), 0);
    }

    #[test]
    fn test_confirm_rename_branch() {
        let mut app = App::default();

        // Empty input returns false
        app.input.buffer = "   ".to_string();
        assert!(!app.confirm_rename_branch());

        // Valid input returns true and sets branch name
        app.input.buffer = "  new-branch  ".to_string();
        assert!(app.confirm_rename_branch());
        assert_eq!(app.git_op.branch_name, "new-branch");
    }
}
