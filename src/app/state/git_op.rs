//! Git operation state: Push, Rename Branch, Open PR, Rebase, Merge

/// Type of git operation being performed (for rebase/merge)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitOperationType {
    /// Rebase current branch onto target branch
    Rebase,
    /// Merge target branch into current branch
    Merge,
    /// Switch the root agent's branch
    SwitchBranch,
}

/// State for git operations (push, rename, open PR, rebase, merge)
#[derive(Debug, Default)]
pub struct GitOpState {
    /// Agent ID for git operations (push, rename, PR)
    pub agent_id: Option<uuid::Uuid>,

    /// Branch name for operations (current or new name when renaming)
    pub branch_name: String,

    /// Original branch name (for rename operations)
    pub original_branch: String,

    /// Base branch for PR (detected from git history)
    pub base_branch: String,

    /// Whether there are unpushed commits (for PR flow)
    pub has_unpushed: bool,

    /// Whether this rename is for a root agent (includes branch rename) or sub-agent (title only)
    pub is_root_rename: bool,

    /// Target branch for rebase/merge operations
    pub target_branch: String,

    /// Type of git operation (rebase or merge)
    pub operation_type: Option<GitOperationType>,
}

impl GitOpState {
    /// Create a new git operation state with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            agent_id: None,
            branch_name: String::new(),
            original_branch: String::new(),
            base_branch: String::new(),
            has_unpushed: false,
            is_root_rename: false,
            target_branch: String::new(),
            operation_type: None,
        }
    }

    /// Start the push flow
    pub fn start_push(&mut self, agent_id: uuid::Uuid, branch_name: String) {
        self.agent_id = Some(agent_id);
        self.branch_name = branch_name;
    }

    /// Start the rename flow
    ///
    /// For root agents (`is_root=true`): Renames branch + agent title + session
    /// For sub-agents (`is_root=false`): Renames agent title + window only
    pub fn start_rename(&mut self, agent_id: uuid::Uuid, current_name: String, is_root: bool) {
        self.agent_id = Some(agent_id);
        self.original_branch.clone_from(&current_name);
        self.branch_name = current_name;
        self.is_root_rename = is_root;
    }

    /// Update the branch name (used when user edits in rename mode)
    pub fn set_branch_name(&mut self, name: String) {
        self.branch_name = name;
    }

    /// Start the open PR flow
    pub fn start_open_pr(
        &mut self,
        agent_id: uuid::Uuid,
        branch_name: String,
        base_branch: String,
        has_unpushed: bool,
    ) {
        self.agent_id = Some(agent_id);
        self.branch_name = branch_name;
        self.base_branch = base_branch;
        self.has_unpushed = has_unpushed;
    }

    /// Clear all git operation state
    pub fn clear(&mut self) {
        self.agent_id = None;
        self.branch_name.clear();
        self.original_branch.clear();
        self.base_branch.clear();
        self.has_unpushed = false;
        self.is_root_rename = false;
        self.target_branch.clear();
        self.operation_type = None;
    }

    /// Start the rebase flow
    pub fn start_rebase(&mut self, agent_id: uuid::Uuid, current_branch: String) {
        self.agent_id = Some(agent_id);
        self.branch_name = current_branch;
        self.operation_type = Some(GitOperationType::Rebase);
    }

    /// Start the merge flow
    pub fn start_merge(&mut self, agent_id: uuid::Uuid, current_branch: String) {
        self.agent_id = Some(agent_id);
        self.branch_name = current_branch;
        self.operation_type = Some(GitOperationType::Merge);
    }

    /// Start the switch-branch flow
    pub fn start_switch_branch(&mut self, agent_id: uuid::Uuid, current_branch: String) {
        self.agent_id = Some(agent_id);
        self.branch_name = current_branch;
        self.operation_type = Some(GitOperationType::SwitchBranch);
    }

    /// Set the target branch for rebase/merge
    pub fn set_target_branch(&mut self, target: String) {
        self.target_branch = target;
    }
}

use super::{App, BranchInfo};
use crate::state::{
    ConfirmPushForPRMode, ConfirmPushMode, MergeBranchSelectorMode, RebaseBranchSelectorMode,
    RenameBranchMode,
};

impl App {
    /// Start the push flow - show confirmation dialog
    pub fn start_push(&mut self, agent_id: uuid::Uuid, branch_name: String) {
        self.data.git_op.start_push(agent_id, branch_name);
        self.apply_mode(ConfirmPushMode.into());
    }

    /// Start the rename flow
    ///
    /// For root agents (`is_root=true`): Renames branch + agent title + session
    /// For sub-agents (`is_root=false`): Renames agent title + window only
    pub fn start_rename(&mut self, agent_id: uuid::Uuid, current_name: String, is_root: bool) {
        self.data
            .git_op
            .start_rename(agent_id, current_name.clone(), is_root);
        self.data.input.buffer = current_name;
        self.data.input.cursor = self.data.input.buffer.len(); // Cursor at end
        self.apply_mode(RenameBranchMode.into());
    }

    /// Confirm the branch rename (update `branch_name` from `input_buffer`)
    pub fn confirm_rename_branch(&mut self) -> bool {
        let new_name = self.data.input.buffer.trim().to_string();
        if new_name.is_empty() {
            return false;
        }
        self.data.git_op.set_branch_name(new_name);
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
        self.data
            .git_op
            .start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        if has_unpushed {
            self.apply_mode(ConfirmPushForPRMode.into());
        } else {
            // No unpushed commits, will open PR directly (handled in handler)
        }
    }

    /// Clear all git operation state
    pub fn clear_git_op_state(&mut self) {
        self.data.git_op.clear();
    }

    /// Start the rebase flow - show branch selector to choose target branch
    pub fn start_rebase(
        &mut self,
        agent_id: uuid::Uuid,
        current_branch: String,
        branches: Vec<BranchInfo>,
    ) {
        self.data.git_op.start_rebase(agent_id, current_branch);
        self.data.review.start(branches);
        self.apply_mode(RebaseBranchSelectorMode.into());
    }

    /// Start the merge flow - show branch selector to choose source branch
    pub fn start_merge(
        &mut self,
        agent_id: uuid::Uuid,
        current_branch: String,
        branches: Vec<BranchInfo>,
    ) {
        self.data.git_op.start_merge(agent_id, current_branch);
        self.data.review.start(branches);
        self.apply_mode(MergeBranchSelectorMode.into());
    }

    /// Confirm branch selection for rebase/merge and set target branch
    pub fn confirm_rebase_merge_branch(&mut self) -> bool {
        if let Some(branch) = self.data.review.selected_branch() {
            let target = if branch.is_remote {
                branch.remote.as_deref().map_or_else(
                    || branch.name.clone(),
                    |remote| format!("{remote}/{}", branch.name),
                )
            } else {
                branch.name.clone()
            };
            self.data.git_op.set_target_branch(target);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::git::BranchInfo;

    fn branch(name: &str, is_remote: bool, remote: Option<&str>) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            full_name: if is_remote {
                format!("refs/remotes/{}/{}", remote.unwrap_or(""), name)
            } else {
                format!("refs/heads/{name}")
            },
            is_remote,
            remote: remote.map(str::to_string),
            last_commit_time: None,
        }
    }

    #[test]
    fn test_git_op_state_new() {
        let state = GitOpState::new();
        assert!(state.agent_id.is_none());
        assert!(state.branch_name.is_empty());
        assert!(state.original_branch.is_empty());
        assert!(state.base_branch.is_empty());
        assert!(!state.has_unpushed);
        assert!(!state.is_root_rename);
        assert!(state.target_branch.is_empty());
        assert!(state.operation_type.is_none());
    }

    #[test]
    fn test_start_push() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_push(agent_id, "feature-branch".to_string());

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "feature-branch");
    }

    #[test]
    fn test_start_rename_root() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_rename(agent_id, "old-name".to_string(), true);

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "old-name");
        assert_eq!(state.original_branch, "old-name");
        assert!(state.is_root_rename);
    }

    #[test]
    fn test_start_rename_sub_agent() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_rename(agent_id, "old-name".to_string(), false);

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "old-name");
        assert_eq!(state.original_branch, "old-name");
        assert!(!state.is_root_rename);
    }

    #[test]
    fn test_set_branch_name() {
        let mut state = GitOpState::new();
        state.set_branch_name("new-name".to_string());
        assert_eq!(state.branch_name, "new-name");
    }

    #[test]
    fn test_start_open_pr() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_open_pr(agent_id, "feature".to_string(), "main".to_string(), true);

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "feature");
        assert_eq!(state.base_branch, "main");
        assert!(state.has_unpushed);
    }

    #[test]
    fn test_clear() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();
        state.start_open_pr(agent_id, "feature".to_string(), "main".to_string(), true);
        state.is_root_rename = true;

        state.clear();

        assert!(state.agent_id.is_none());
        assert!(state.branch_name.is_empty());
        assert!(state.original_branch.is_empty());
        assert!(state.base_branch.is_empty());
        assert!(!state.has_unpushed);
        assert!(!state.is_root_rename);
        assert!(state.target_branch.is_empty());
        assert!(state.operation_type.is_none());
    }

    #[test]
    fn test_start_rebase() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_rebase(agent_id, "feature-branch".to_string());

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "feature-branch");
        assert_eq!(state.operation_type, Some(GitOperationType::Rebase));
    }

    #[test]
    fn test_start_merge() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_merge(agent_id, "feature-branch".to_string());

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "feature-branch");
        assert_eq!(state.operation_type, Some(GitOperationType::Merge));
    }

    #[test]
    fn test_start_switch_branch() {
        let mut state = GitOpState::new();
        let agent_id = uuid::Uuid::new_v4();

        state.start_switch_branch(agent_id, "feature-branch".to_string());

        assert_eq!(state.agent_id, Some(agent_id));
        assert_eq!(state.branch_name, "feature-branch");
        assert_eq!(state.operation_type, Some(GitOperationType::SwitchBranch));
    }

    #[test]
    fn test_set_target_branch() {
        let mut state = GitOpState::new();
        state.set_target_branch("main".to_string());
        assert_eq!(state.target_branch, "main");
    }

    #[test]
    fn test_app_start_rebase_sets_state_and_mode() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();
        let branches = vec![branch("main", false, None)];

        app.start_rebase(agent_id, "feature".to_string(), branches);

        let normal_app = App::default();
        for (candidate, expected) in [(&app, true), (&normal_app, false)] {
            assert_eq!(
                matches!(
                    &candidate.mode,
                    crate::state::AppMode::RebaseBranchSelector(_)
                ),
                expected
            );
        }
        assert_eq!(app.data.git_op.agent_id, Some(agent_id));
        assert_eq!(app.data.git_op.branch_name, "feature");
        assert_eq!(
            app.data.git_op.operation_type,
            Some(GitOperationType::Rebase)
        );
        assert_eq!(app.data.review.branches.len(), 1);
    }

    #[test]
    fn test_app_start_merge_sets_state_and_mode() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();
        let branches = vec![branch("main", false, None)];

        app.start_merge(agent_id, "feature".to_string(), branches);

        let normal_app = App::default();
        for (candidate, expected) in [(&app, true), (&normal_app, false)] {
            assert_eq!(
                matches!(
                    &candidate.mode,
                    crate::state::AppMode::MergeBranchSelector(_)
                ),
                expected
            );
        }
        assert_eq!(app.data.git_op.agent_id, Some(agent_id));
        assert_eq!(app.data.git_op.branch_name, "feature");
        assert_eq!(
            app.data.git_op.operation_type,
            Some(GitOperationType::Merge)
        );
        assert_eq!(app.data.review.branches.len(), 1);
    }

    #[test]
    fn test_confirm_rebase_merge_branch_sets_local_target() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();
        let branches = vec![branch("main", false, None)];

        app.start_merge(agent_id, "feature".to_string(), branches);
        assert!(app.confirm_rebase_merge_branch());
        assert_eq!(app.data.git_op.target_branch, "main");
    }

    #[test]
    fn test_confirm_rebase_merge_branch_formats_remote_target_with_remote_name() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();
        let branches = vec![branch("feature", true, Some("origin"))];

        app.start_rebase(agent_id, "current".to_string(), branches);
        assert!(app.confirm_rebase_merge_branch());
        assert_eq!(app.data.git_op.target_branch, "origin/feature");
    }

    #[test]
    fn test_confirm_rebase_merge_branch_uses_remote_name_fallback_when_missing() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();
        let branches = vec![branch("feature", true, None)];

        app.start_rebase(agent_id, "current".to_string(), branches);
        assert!(app.confirm_rebase_merge_branch());
        assert_eq!(app.data.git_op.target_branch, "feature");
    }

    #[test]
    fn test_confirm_rebase_merge_branch_returns_false_without_selection() {
        let mut app = App::default();
        let agent_id = uuid::Uuid::new_v4();
        let branches = vec![branch("main", false, None)];

        app.start_merge(agent_id, "feature".to_string(), branches);
        app.data.review.filter = "nope".to_string();

        assert!(!app.confirm_rebase_merge_branch());
        assert!(app.data.git_op.target_branch.is_empty());
    }
}
