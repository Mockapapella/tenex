//! Review state: branch selection for review agents

use crate::git::BranchInfo;

/// State for the review swarm feature
#[derive(Debug, Default)]
pub struct ReviewState {
    /// List of branches for the branch selector
    pub branches: Vec<BranchInfo>,

    /// Current filter text for branch search
    pub filter: String,

    /// Currently selected branch index in filtered list
    pub selected: usize,

    /// Selected base branch for review (after confirmation)
    pub base_branch: Option<String>,
}

impl ReviewState {
    /// Create a new review state with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            branches: Vec::new(),
            filter: String::new(),
            selected: 0,
            base_branch: None,
        }
    }

    /// Start the review flow with the given branches
    pub fn start(&mut self, branches: Vec<BranchInfo>) {
        self.branches = branches;
        self.filter.clear();
        self.selected = 0;
        self.base_branch = None;
    }

    /// Get filtered branches based on current filter
    #[must_use]
    pub fn filtered_branches(&self) -> Vec<&BranchInfo> {
        let filter_lower = self.filter.to_lowercase();
        self.branches
            .iter()
            .filter(|b| filter_lower.is_empty() || b.name.to_lowercase().contains(&filter_lower))
            .collect()
    }

    /// Select next branch in filtered list
    pub fn select_next(&mut self) {
        let count = self.filtered_branches().len();
        if count > 0 {
            self.selected = (self.selected + 1) % count;
        }
    }

    /// Select previous branch in filtered list
    pub fn select_prev(&mut self) {
        let count = self.filtered_branches().len();
        if count > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
        }
    }

    /// Get the currently selected branch
    #[must_use]
    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.filtered_branches().get(self.selected).copied()
    }

    /// Handle character input in branch filter
    pub fn handle_filter_char(&mut self, c: char) {
        self.filter.push(c);
        // Reset selection to 0 when filter changes
        self.selected = 0;
    }

    /// Handle backspace in branch filter
    pub fn handle_filter_backspace(&mut self) {
        self.filter.pop();
        // Reset selection when filter changes
        self.selected = 0;
    }

    /// Confirm branch selection and set `base_branch`
    pub fn confirm_selection(&mut self) -> bool {
        if let Some(branch) = self.selected_branch() {
            self.base_branch = Some(branch.name.clone());
            true
        } else {
            false
        }
    }

    /// Clear all review-related state
    pub fn clear(&mut self) {
        self.branches.clear();
        self.filter.clear();
        self.selected = 0;
        self.base_branch = None;
    }
}

use super::App;
use crate::state::{BranchSelectorMode, ReviewChildCountMode, ReviewInfoMode};

impl App {
    /// Start the review flow - show info if no agent selected, otherwise proceed to count
    pub fn start_review(&mut self, branches: Vec<BranchInfo>) {
        self.data.review.start(branches);
        self.data.spawn.child_count = 3; // Reset to default
        self.apply_mode(ReviewChildCountMode.into());
    }

    /// Show the review info modal (when no agent is selected)
    pub fn show_review_info(&mut self) {
        self.apply_mode(ReviewInfoMode.into());
    }

    /// Proceed from review count to branch selector
    pub fn proceed_to_branch_selector(&mut self) {
        self.apply_mode(BranchSelectorMode.into());
    }

    /// Get filtered branches based on current filter
    #[must_use]
    pub fn filtered_review_branches(&self) -> Vec<&BranchInfo> {
        self.data.review.filtered_branches()
    }

    /// Select next branch in filtered list
    pub fn select_next_branch(&mut self) {
        self.data.review.select_next();
    }

    /// Select previous branch in filtered list
    pub fn select_prev_branch(&mut self) {
        self.data.review.select_prev();
    }

    /// Get the currently selected branch
    #[must_use]
    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.data.review.selected_branch()
    }

    /// Handle character input in branch filter
    pub fn handle_branch_filter_char(&mut self, c: char) {
        self.data.review.handle_filter_char(c);
    }

    /// Handle backspace in branch filter
    pub fn handle_branch_filter_backspace(&mut self) {
        self.data.review.handle_filter_backspace();
    }

    /// Confirm branch selection and set `review_base_branch`
    pub fn confirm_branch_selection(&mut self) -> bool {
        self.data.review.confirm_selection()
    }

    /// Clear all review-related state
    pub fn clear_review_state(&mut self) {
        self.data.review.clear();
    }
}
