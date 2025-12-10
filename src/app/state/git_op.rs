//! Git operation state: Push, Rename Branch, Open PR

/// State for git operations (push, rename, open PR)
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
        }
    }

    /// Start the push flow
    pub fn start_push(&mut self, agent_id: uuid::Uuid, branch_name: String) {
        self.agent_id = Some(agent_id);
        self.branch_name = branch_name;
    }

    /// Start the rename flow
    ///
    /// For root agents (`is_root=true`): Renames branch + agent title + tmux session
    /// For sub-agents (`is_root=false`): Renames agent title + tmux window only
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_op_state_new() {
        let state = GitOpState::new();
        assert!(state.agent_id.is_none());
        assert!(state.branch_name.is_empty());
        assert!(state.original_branch.is_empty());
        assert!(state.base_branch.is_empty());
        assert!(!state.has_unpushed);
        assert!(!state.is_root_rename);
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
    }
}
