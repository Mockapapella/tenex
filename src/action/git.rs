use crate::action::ValidIn;
use crate::app::{Actions, AppData};
use crate::git;
use crate::state::{
    AppMode, ConfirmPushForPRMode, ConfirmPushMode, ErrorModalMode, MergeBranchSelectorMode,
    NormalMode, RebaseBranchSelectorMode, RenameBranchMode, ScrollingMode,
    SwitchBranchSelectorMode,
};
use anyhow::Result;

/// Normal-mode action: start the git push flow.
#[derive(Debug, Clone, Copy, Default)]
pub struct PushAction;

impl ValidIn<NormalMode> for PushAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Push requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        app_data.git_op.start_push(agent_id, branch_name);

        Ok(ConfirmPushMode.into())
    }
}

impl ValidIn<ScrollingMode> for PushAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Push requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        app_data.git_op.start_push(agent_id, branch_name);

        Ok(ConfirmPushMode.into())
    }
}

/// Normal-mode action: start the rename-branch flow.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenameBranchAction;

impl ValidIn<NormalMode> for RenameBranchAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let is_root = agent.is_root() && agent.is_git_workspace();
        let current_name = agent.title.clone();

        app_data
            .git_op
            .start_rename(agent_id, current_name.clone(), is_root);
        app_data.input.buffer = current_name;
        app_data.input.cursor = app_data.input.buffer.len();

        Ok(RenameBranchMode.into())
    }
}

impl ValidIn<ScrollingMode> for RenameBranchAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let is_root = agent.is_root() && agent.is_git_workspace();
        let current_name = agent.title.clone();

        app_data
            .git_op
            .start_rename(agent_id, current_name.clone(), is_root);
        app_data.input.buffer = current_name;
        app_data.input.cursor = app_data.input.buffer.len();

        Ok(RenameBranchMode.into())
    }
}

/// Normal-mode action: open a pull request (may prompt for push first).
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenPRAction;

impl ValidIn<NormalMode> for OpenPRAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Open PR requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        let base_branch = Actions::detect_base_branch(&worktree_path, &branch_name);
        let has_unpushed = Actions::has_unpushed_commits(&worktree_path, &branch_name)?;

        app_data
            .git_op
            .start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        if has_unpushed {
            return Ok(ConfirmPushForPRMode.into());
        }

        Actions::open_pr_in_browser(app_data)?;
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for OpenPRAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Open PR requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        let base_branch = Actions::detect_base_branch(&worktree_path, &branch_name);
        let has_unpushed = Actions::has_unpushed_commits(&worktree_path, &branch_name)?;

        app_data
            .git_op
            .start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        if has_unpushed {
            return Ok(ConfirmPushForPRMode.into());
        }

        Actions::open_pr_in_browser(app_data)?;
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: start the rebase flow (branch selector).
#[derive(Debug, Clone, Copy, Default)]
pub struct RebaseAction;

impl ValidIn<NormalMode> for RebaseAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to rebase.".to_string(),
            }
            .into());
        };
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Rebase requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo = git::open_repository(&agent.worktree_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_rebase(agent_id, current_branch);
        app_data.review.start(branches);

        Ok(RebaseBranchSelectorMode.into())
    }
}

impl ValidIn<ScrollingMode> for RebaseAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to rebase.".to_string(),
            }
            .into());
        };
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Rebase requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo = git::open_repository(&agent.worktree_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_rebase(agent_id, current_branch);
        app_data.review.start(branches);

        Ok(RebaseBranchSelectorMode.into())
    }
}

/// Normal-mode action: start the merge flow (branch selector).
#[derive(Debug, Clone, Copy, Default)]
pub struct MergeAction;

impl ValidIn<NormalMode> for MergeAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to merge.".to_string(),
            }
            .into());
        };
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Merge requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo = git::open_repository(&agent.worktree_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_merge(agent_id, current_branch);
        app_data.review.start(branches);

        Ok(MergeBranchSelectorMode.into())
    }
}

impl ValidIn<ScrollingMode> for MergeAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to merge.".to_string(),
            }
            .into());
        };
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message:
                    "Merge requires a git repository. Start Tenex in a git repo to use worktrees."
                        .to_string(),
            }
            .into());
        }

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo = git::open_repository(&agent.worktree_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_merge(agent_id, current_branch);
        app_data.review.start(branches);

        Ok(MergeBranchSelectorMode.into())
    }
}

/// Normal-mode action: switch the selected root agent's branch (branch selector).
#[derive(Debug, Clone, Copy, Default)]
pub struct SwitchBranchAction;

impl ValidIn<NormalMode> for SwitchBranchAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to switch branches.".to_string(),
            }
            .into());
        };
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message: "Switch branch requires a git repository. Start Tenex in a git repo to use worktrees."
                    .to_string(),
            }
            .into());
        }

        let root = app_data
            .storage
            .root_ancestor(agent.id)
            .ok_or_else(|| anyhow::anyhow!("Could not find root agent"))?;
        let root_id = root.id;
        let root_branch = root.branch.clone();

        // Fetch branches for selector.
        let repo = git::open_repository(&root.worktree_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_switch_branch(root_id, root_branch);
        app_data.review.start(branches);

        Ok(SwitchBranchSelectorMode.into())
    }
}

impl ValidIn<ScrollingMode> for SwitchBranchAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to switch branches.".to_string(),
            }
            .into());
        };
        if !agent.is_git_workspace() {
            return Ok(ErrorModalMode {
                message: "Switch branch requires a git repository. Start Tenex in a git repo to use worktrees."
                    .to_string(),
            }
            .into());
        }

        let root = app_data
            .storage
            .root_ancestor(agent.id)
            .ok_or_else(|| anyhow::anyhow!("Could not find root agent"))?;
        let root_id = root.id;
        let root_branch = root.branch.clone();

        // Fetch branches for selector.
        let repo = git::open_repository(&root.worktree_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_switch_branch(root_id, root_branch);
        app_data.review.start(branches);

        Ok(SwitchBranchSelectorMode.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage, WorkspaceKind};
    use crate::config::Config;
    use git2::{Repository, RepositoryInitOptions, Signature};
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn make_agent(title: &str) -> Agent {
        let pid = std::process::id();
        Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("tenex-action-git-test-{pid}/{title}"),
            PathBuf::from(format!("/tmp/tenex-action-git-test-{pid}/{title}")),
        )
    }

    fn init_repo() -> Result<(TempDir, PathBuf)> {
        let dir = TempDir::new()?;
        let path = dir.path().to_path_buf();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");

        let repo = Repository::init_opts(&path, &init_opts)?;
        repo.set_head("refs/heads/master")?;
        {
            let mut config = repo.config()?;
            config.set_str("user.name", "Test")?;
            config.set_str("user.email", "test@test.com")?;
            config.set_str("commit.gpgsign", "false")?;
        }

        std::fs::write(path.join("README.md"), "# Test\n")?;
        let sig = Signature::now("Test", "test@test.com")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        Ok((dir, path))
    }

    fn new_data_with_agent(agent: Agent) -> AppData {
        let mut storage = Storage::new();
        storage.add(agent);
        AppData::new(
            Config::default(),
            storage,
            crate::app::Settings::default(),
            false,
        )
    }

    #[test]
    fn test_push_action_sets_git_op_state_in_normal_and_scrolling()
    -> Result<(), Box<dyn std::error::Error>> {
        let agent = make_agent("agent-1");
        let agent_id = agent.id;
        let branch = agent.branch.clone();
        let mut data = new_data_with_agent(agent);

        let next = PushAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ConfirmPush(_)));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, branch);

        let next = PushAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ConfirmPush(_)));
        Ok(())
    }

    #[test]
    fn test_push_action_returns_error_modal_when_not_git_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = PushAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));

        let next = PushAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_rename_branch_action_enters_rename_mode_and_populates_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let agent = make_agent("my-branch");
        let agent_id = agent.id;
        let title = agent.title.clone();
        let mut data = new_data_with_agent(agent);

        let next = RenameBranchAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::RenameBranch(_)));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, title);
        assert_eq!(data.input.cursor, data.input.buffer.len());
        assert!(!data.input.buffer.is_empty());

        let next = RenameBranchAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::RenameBranch(_)));
        Ok(())
    }

    #[test]
    fn test_open_pr_action_errors_without_selected_agent() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        assert!(OpenPRAction.execute(NormalMode, &mut data).is_err());
        assert!(OpenPRAction.execute(ScrollingMode, &mut data).is_err());
    }

    #[test]
    fn test_open_pr_action_returns_error_modal_when_not_git_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = OpenPRAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = OpenPRAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_open_pr_action_enters_confirm_push_for_pr_when_unpushed()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, repo_path) = init_repo()?;

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let agent_id = agent.id;
        let mut data = new_data_with_agent(agent);

        let next = OpenPRAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ConfirmPushForPR(_)));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, "master");
        assert!(data.git_op.has_unpushed);

        let next = OpenPRAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ConfirmPushForPR(_)));
        Ok(())
    }

    #[test]
    fn test_rebase_action_returns_error_modal_without_selected_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = RebaseAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = RebaseAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_rebase_action_returns_error_modal_when_not_git_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = RebaseAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = RebaseAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_merge_action_returns_error_modal_without_selected_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = MergeAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = MergeAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_merge_action_returns_error_modal_when_not_git_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = MergeAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = MergeAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_switch_branch_action_returns_error_modal_without_selected_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = SwitchBranchAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = SwitchBranchAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_switch_branch_action_returns_error_modal_when_not_git_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = SwitchBranchAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        let next = SwitchBranchAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_switch_branch_action_enters_branch_selector_and_sets_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, repo_path) = init_repo()?;

        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr.create("feature")?;

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let agent_id = agent.id;
        let mut data = new_data_with_agent(agent);

        let next = SwitchBranchAction.execute(NormalMode, &mut data)?;
        assert!(matches!(next, AppMode::SwitchBranchSelector(_)));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, "master");
        assert!(data.review.filter.is_empty());
        assert!(!data.review.branches.is_empty());

        let next = SwitchBranchAction.execute(ScrollingMode, &mut data)?;
        assert!(matches!(next, AppMode::SwitchBranchSelector(_)));

        Ok(())
    }
}
