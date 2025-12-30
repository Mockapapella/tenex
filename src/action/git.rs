use crate::action::ValidIn;
use crate::app::{Actions, AppData};
use crate::git;
use crate::state::{
    AppMode, ConfirmPushForPRMode, ConfirmPushMode, ErrorModalMode, MergeBranchSelectorMode,
    NormalMode, RebaseBranchSelectorMode, RenameBranchMode, ScrollingMode,
};
use anyhow::{Context, Result};

/// Normal-mode action: start the git push flow.
#[derive(Debug, Clone, Copy, Default)]
pub struct PushAction;

impl ValidIn<NormalMode> for PushAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let agent = app_data
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

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
        let is_root = agent.is_root();
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
        let is_root = agent.is_root();
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

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        let base_branch = Actions::detect_base_branch(&worktree_path, &branch_name)?;
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

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        let base_branch = Actions::detect_base_branch(&worktree_path, &branch_name)?;
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

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path)?;
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

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path)?;
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

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
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

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        // Fetch branches for selector.
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_merge(agent_id, current_branch);
        app_data.review.start(branches);

        Ok(MergeBranchSelectorMode.into())
    }
}
