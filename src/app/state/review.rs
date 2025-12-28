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

use super::{App, BranchPickerKind, CountPickerKind, Mode, OverlayMode};

impl App {
    /// Start the review flow - show info if no agent selected, otherwise proceed to count
    pub fn start_review(&mut self, branches: Vec<BranchInfo>) {
        self.review.start(branches);
        self.spawn.child_count = 3; // Reset to default
        self.enter_mode(Mode::Overlay(OverlayMode::CountPicker(
            CountPickerKind::ReviewChildCount,
        )));
    }

    /// Show the review info modal (when no agent is selected)
    pub fn show_review_info(&mut self) {
        self.enter_mode(Mode::Overlay(OverlayMode::ReviewInfo));
    }

    /// Proceed from review count to branch selector
    pub fn proceed_to_branch_selector(&mut self) {
        self.enter_mode(Mode::Overlay(OverlayMode::BranchPicker(
            BranchPickerKind::ReviewBaseBranch,
        )));
    }

    /// Get filtered branches based on current filter
    #[must_use]
    pub fn filtered_review_branches(&self) -> Vec<&BranchInfo> {
        self.review.filtered_branches()
    }

    /// Select next branch in filtered list
    pub fn select_next_branch(&mut self) {
        self.review.select_next();
    }

    /// Select previous branch in filtered list
    pub fn select_prev_branch(&mut self) {
        self.review.select_prev();
    }

    /// Get the currently selected branch
    #[must_use]
    pub fn selected_branch(&self) -> Option<&BranchInfo> {
        self.review.selected_branch()
    }

    /// Handle character input in branch filter
    pub fn handle_branch_filter_char(&mut self, c: char) {
        self.review.handle_filter_char(c);
    }

    /// Handle backspace in branch filter
    pub fn handle_branch_filter_backspace(&mut self) {
        self.review.handle_filter_backspace();
    }

    /// Confirm branch selection and set `review_base_branch`
    pub fn confirm_branch_selection(&mut self) -> bool {
        self.review.confirm_selection()
    }

    /// Clear all review-related state
    pub fn clear_review_state(&mut self) {
        self.review.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_branch(name: &str) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            full_name: format!("refs/heads/{name}"),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        }
    }

    #[test]
    fn test_review_state_new() {
        let state = ReviewState::new();
        assert!(state.branches.is_empty());
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
        assert!(state.base_branch.is_none());
    }

    #[test]
    fn test_start() {
        let mut state = ReviewState::new();
        state.filter = "old-filter".to_string();
        state.selected = 5;

        let branches = vec![make_branch("main"), make_branch("develop")];
        state.start(branches);

        assert_eq!(state.branches.len(), 2);
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
        assert!(state.base_branch.is_none());
    }

    #[test]
    fn test_filtered_branches_no_filter() {
        let mut state = ReviewState::new();
        state.branches = vec![make_branch("main"), make_branch("develop")];

        let filtered = state.filtered_branches();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filtered_branches_with_filter() {
        let mut state = ReviewState::new();
        state.branches = vec![
            make_branch("main"),
            make_branch("develop"),
            make_branch("feature-auth"),
        ];
        state.filter = "dev".to_string();

        let filtered = state.filtered_branches();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "develop");
    }

    #[test]
    fn test_select_next() {
        let mut state = ReviewState::new();
        state.branches = vec![make_branch("a"), make_branch("b"), make_branch("c")];
        state.selected = 0;

        state.select_next();
        assert_eq!(state.selected, 1);

        state.select_next();
        assert_eq!(state.selected, 2);

        state.select_next();
        assert_eq!(state.selected, 0); // Wraps around
    }

    #[test]
    fn test_select_prev() {
        let mut state = ReviewState::new();
        state.branches = vec![make_branch("a"), make_branch("b"), make_branch("c")];
        state.selected = 0;

        state.select_prev();
        assert_eq!(state.selected, 2); // Wraps around to end

        state.select_prev();
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn test_selected_branch() {
        let mut state = ReviewState::new();
        state.branches = vec![make_branch("main"), make_branch("develop")];
        state.selected = 1;

        let selected = state.selected_branch();
        assert!(selected.is_some());
        if let Some(branch) = selected {
            assert_eq!(branch.name, "develop");
        }
    }

    #[test]
    fn test_handle_filter_char() {
        let mut state = ReviewState::new();
        state.selected = 5;

        state.handle_filter_char('a');
        state.handle_filter_char('b');

        assert_eq!(state.filter, "ab");
        assert_eq!(state.selected, 0); // Reset on filter change
    }

    #[test]
    fn test_handle_filter_backspace() {
        let mut state = ReviewState::new();
        state.filter = "abc".to_string();
        state.selected = 5;

        state.handle_filter_backspace();

        assert_eq!(state.filter, "ab");
        assert_eq!(state.selected, 0); // Reset on filter change
    }

    #[test]
    fn test_confirm_selection() {
        let mut state = ReviewState::new();
        state.branches = vec![make_branch("main"), make_branch("develop")];
        state.selected = 1;

        let result = state.confirm_selection();
        assert!(result);
        assert_eq!(state.base_branch, Some("develop".to_string()));
    }

    #[test]
    fn test_confirm_selection_empty() {
        let mut state = ReviewState::new();

        let result = state.confirm_selection();
        assert!(!result);
        assert!(state.base_branch.is_none());
    }

    #[test]
    fn test_clear() {
        let mut state = ReviewState::new();
        state.branches = vec![make_branch("main")];
        state.filter = "test".to_string();
        state.selected = 5;
        state.base_branch = Some("main".to_string());

        state.clear();

        assert!(state.branches.is_empty());
        assert!(state.filter.is_empty());
        assert_eq!(state.selected, 0);
        assert!(state.base_branch.is_none());
    }
}
