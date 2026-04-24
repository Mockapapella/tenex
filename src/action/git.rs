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

        let root = app_data.storage.root_ancestor(agent.id).unwrap_or(agent);
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

        let root = app_data.storage.root_ancestor(agent.id).unwrap_or(agent);
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
    use std::sync::OnceLock;
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

    fn init_repo() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temp repo dir should be created");
        let path = dir.path().to_path_buf();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");

        let repo = Repository::init_opts(&path, &init_opts).expect("repo should init");
        repo.set_head("refs/heads/master")
            .expect("repo head should be set");
        {
            let mut config = repo.config().expect("repo config should open");
            config
                .set_str("user.name", "Test")
                .expect("user.name should be set");
            config
                .set_str("user.email", "test@test.com")
                .expect("user.email should be set");
            config
                .set_str("commit.gpgsign", "false")
                .expect("commit.gpgsign should be set");
        }

        std::fs::write(path.join("README.md"), "# Test\n").expect("fixture file should write");
        let sig = Signature::now("Test", "test@test.com").expect("signature should be created");
        let mut index = repo.index().expect("repo index should open");
        index
            .add_path(Path::new("README.md"))
            .expect("index should stage fixture file");
        index.write().expect("index should write");
        let tree_id = index.write_tree().expect("tree should write");
        let tree = repo.find_tree(tree_id).expect("tree should be readable");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("initial commit should succeed");

        (dir, path)
    }

    fn ensure_stub_gh_installed() {
        static GH_STUB: OnceLock<PathBuf> = OnceLock::new();

        let gh_path = GH_STUB
            .get_or_init(|| {
                let dir = std::env::temp_dir().join(format!(
                    "tenex-gh-stub-action-git-{}-{}",
                    std::process::id(),
                    uuid::Uuid::new_v4()
                ));
                std::fs::create_dir_all(&dir).expect("gh stub dir should be created");

                #[cfg(windows)]
                let gh_path = dir.join("gh.cmd");
                #[cfg(not(windows))]
                let gh_path = dir.join("gh");

                #[cfg(windows)]
                std::fs::write(
                    &gh_path,
                    r#"@echo off
if "%5"=="main" (
  exit /b 0
)

echo boom 1>&2
exit /b 1
"#,
                )
                .expect("gh stub script should be written");

                #[cfg(not(windows))]
                std::fs::write(
                    &gh_path,
                    r#"#!/usr/bin/env bash

if [[ "$5" == "main" ]]; then
  exit 0
fi

echo "boom" 1>&2
exit 1
"#,
                )
                .expect("gh stub script should be written");

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = std::fs::metadata(&gh_path)
                        .expect("gh stub metadata should be readable")
                        .permissions();
                    perms.set_mode(0o755);
                    std::fs::set_permissions(&gh_path, perms)
                        .expect("gh stub permissions should be set");
                }

                gh_path
            })
            .clone();

        crate::app::set_gh_binary_override_for_tests(gh_path);
    }

    fn git_ok(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let output = crate::git::git_command()
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        if output.status.success() {
            return Ok(());
        }
        Err(format!(
            "git {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
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

    fn is_confirm_push_mode(mode: &AppMode) -> bool {
        matches!(mode, AppMode::ConfirmPush(_))
    }

    fn is_confirm_push_for_pr_mode(mode: &AppMode) -> bool {
        matches!(mode, AppMode::ConfirmPushForPR(_))
    }

    fn is_error_modal(mode: &AppMode) -> bool {
        matches!(mode, AppMode::ErrorModal(_))
    }

    fn is_merge_branch_selector_mode(mode: &AppMode) -> bool {
        matches!(mode, AppMode::MergeBranchSelector(_))
    }

    fn is_rebase_branch_selector_mode(mode: &AppMode) -> bool {
        matches!(mode, AppMode::RebaseBranchSelector(_))
    }

    fn is_rename_branch_mode(mode: &AppMode) -> bool {
        matches!(mode, AppMode::RenameBranch(_))
    }

    fn is_switch_branch_selector_mode(mode: &AppMode) -> bool {
        matches!(mode, AppMode::SwitchBranchSelector(_))
    }

    #[cfg(unix)]
    struct PermissionsGuard {
        path: PathBuf,
        original: std::fs::Permissions,
    }

    #[cfg(unix)]
    impl Drop for PermissionsGuard {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.path, self.original.clone());
        }
    }

    #[cfg(unix)]
    fn deny_all_permissions(path: &Path) -> PermissionsGuard {
        use std::os::unix::fs::PermissionsExt;

        let original = std::fs::metadata(path)
            .expect("permissions target should exist")
            .permissions();
        let mut perms = original.clone();
        perms.set_mode(0o000);
        std::fs::set_permissions(path, perms).expect("permissions should be updated");
        PermissionsGuard {
            path: path.to_path_buf(),
            original,
        }
    }

    #[test]
    fn test_push_action_sets_git_op_state_in_normal_and_scrolling() {
        let agent = make_agent("agent-1");
        let agent_id = agent.id;
        let branch = agent.branch.clone();
        let mut data = new_data_with_agent(agent);

        let next = PushAction
            .execute(NormalMode, &mut data)
            .expect("push action should succeed");
        assert!(is_confirm_push_mode(&next));
        assert!(!is_confirm_push_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, branch);

        let next = PushAction
            .execute(ScrollingMode, &mut data)
            .expect("push action should succeed");
        assert!(is_confirm_push_mode(&next));
        assert!(!is_confirm_push_mode(&AppMode::normal()));
    }

    #[test]
    fn test_push_action_errors_without_selected_agent() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        assert!(PushAction.execute(NormalMode, &mut data).is_err());
        assert!(PushAction.execute(ScrollingMode, &mut data).is_err());
    }

    #[test]
    fn test_push_action_returns_error_modal_when_not_git_workspace() {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = PushAction
            .execute(NormalMode, &mut data)
            .expect("push action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));

        let next = PushAction
            .execute(ScrollingMode, &mut data)
            .expect("push action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_rename_branch_action_enters_rename_mode_and_populates_input() {
        let agent = make_agent("my-branch");
        let agent_id = agent.id;
        let title = agent.title.clone();
        let mut data = new_data_with_agent(agent);

        let next = RenameBranchAction
            .execute(NormalMode, &mut data)
            .expect("rename action should succeed");
        assert!(is_rename_branch_mode(&next));
        assert!(!is_rename_branch_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, title);
        assert!(data.git_op.is_root_rename);
        assert_eq!(data.input.cursor, data.input.buffer.len());
        assert!(!data.input.buffer.is_empty());

        let next = RenameBranchAction
            .execute(ScrollingMode, &mut data)
            .expect("rename action should succeed");
        assert!(is_rename_branch_mode(&next));
        assert!(!is_rename_branch_mode(&AppMode::normal()));
    }

    #[test]
    fn test_rename_branch_action_errors_without_selected_agent() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        assert!(RenameBranchAction.execute(NormalMode, &mut data).is_err());
        assert!(
            RenameBranchAction
                .execute(ScrollingMode, &mut data)
                .is_err()
        );
    }

    #[test]
    fn test_rename_branch_action_sets_is_root_flag_for_child_agent() {
        use crate::app::SidebarItem;

        let mut root = make_agent("root-agent");
        root.collapsed = false;
        let root_id = root.id;

        let mut child = make_agent("child-agent");
        child.parent_id = Some(root_id);
        let child_id = child.id;

        let mut storage = Storage::new();
        storage.add(root);
        storage.add(child);
        let mut data = AppData::new(
            Config::default(),
            storage,
            crate::app::Settings::default(),
            false,
        );

        let selected = data
            .sidebar_items()
            .iter()
            .position(|item| match item {
                SidebarItem::Agent(agent) => agent.info.agent.id == child_id,
                SidebarItem::Project(_) => false,
            })
            .expect("child agent should be visible in sidebar");
        data.selected = selected;

        let next = RenameBranchAction
            .execute(NormalMode, &mut data)
            .expect("rename action should succeed");
        assert!(is_rename_branch_mode(&next));
        assert!(!is_rename_branch_mode(&AppMode::normal()));
        assert!(!data.git_op.is_root_rename);

        let next = RenameBranchAction
            .execute(ScrollingMode, &mut data)
            .expect("rename action should succeed");
        assert!(is_rename_branch_mode(&next));
        assert!(!is_rename_branch_mode(&AppMode::normal()));
        assert!(!data.git_op.is_root_rename);
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
    fn test_open_pr_action_returns_error_modal_when_not_git_workspace() {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = OpenPRAction
            .execute(NormalMode, &mut data)
            .expect("open pr action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = OpenPRAction
            .execute(ScrollingMode, &mut data)
            .expect("open pr action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_open_pr_action_enters_confirm_push_for_pr_when_unpushed() {
        let (_dir, repo_path) = init_repo();

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let agent_id = agent.id;
        let mut data = new_data_with_agent(agent);

        let next = OpenPRAction
            .execute(NormalMode, &mut data)
            .expect("open pr action should succeed");
        assert!(is_confirm_push_for_pr_mode(&next));
        assert!(!is_confirm_push_for_pr_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, "master");
        assert!(data.git_op.has_unpushed);

        let next = OpenPRAction
            .execute(ScrollingMode, &mut data)
            .expect("open pr action should succeed");
        assert!(is_confirm_push_for_pr_mode(&next));
        assert!(!is_confirm_push_for_pr_mode(&AppMode::normal()));
    }

    #[test]
    fn test_open_pr_action_propagates_has_unpushed_errors_in_normal_and_scrolling() {
        let temp = TempDir::new().expect("temp dir should be created");
        let missing = temp.path().join("missing-worktree-dir");

        let mut agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            missing,
        );
        agent.workspace_kind = WorkspaceKind::GitWorktree;

        let mut data = new_data_with_agent(agent);

        let err = OpenPRAction
            .execute(NormalMode, &mut data)
            .expect_err("open pr should report has_unpushed errors");
        assert!(err.to_string().contains("Failed to check remote branch"));
        assert!(data.git_op.agent_id.is_none());

        let err = OpenPRAction
            .execute(ScrollingMode, &mut data)
            .expect_err("open pr should report has_unpushed errors");
        assert!(err.to_string().contains("Failed to check remote branch"));
        assert!(data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_open_pr_action_propagates_open_pr_errors_and_clears_state() {
        ensure_stub_gh_installed();

        let (_dir, repo_path) = init_repo();
        let origin_dir = TempDir::new().expect("origin dir should be created");

        git_ok(origin_dir.path(), &["init", "--bare"]).expect("origin repo should init");
        let origin_path = origin_dir
            .path()
            .to_str()
            .expect("origin path should be utf-8");
        git_ok(
            repo_path.as_path(),
            &["remote", "add", "origin", origin_path],
        )
        .expect("remote should be added");

        git_ok(
            repo_path.as_path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "push",
                "-u",
                "origin",
                "master",
            ],
        )
        .expect("git push should succeed");
        git_ok(
            repo_path.as_path(),
            &["-c", "protocol.file.allow=always", "fetch", "origin"],
        )
        .expect("git fetch should succeed");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = OpenPRAction
            .execute(NormalMode, &mut data)
            .expect_err("open pr should surface gh failures");
        assert!(err.to_string().contains("boom"));
        assert!(data.git_op.agent_id.is_none());
        assert!(data.git_op.branch_name.is_empty());
        assert!(data.git_op.base_branch.is_empty());

        let err = OpenPRAction
            .execute(ScrollingMode, &mut data)
            .expect_err("open pr should surface gh failures");
        assert!(err.to_string().contains("boom"));
        assert!(data.git_op.agent_id.is_none());
        assert!(data.git_op.branch_name.is_empty());
        assert!(data.git_op.base_branch.is_empty());
    }

    #[test]
    fn test_open_pr_action_opens_pr_immediately_when_no_unpushed_commits() {
        ensure_stub_gh_installed();
        ensure_stub_gh_installed();

        let (_dir, repo_path) = init_repo();
        let origin_dir = TempDir::new().expect("origin dir should be created");

        git_ok(origin_dir.path(), &["init", "--bare"]).expect("origin repo should init");
        let origin_path = origin_dir
            .path()
            .to_str()
            .expect("origin path should be utf-8");
        git_ok(
            repo_path.as_path(),
            &["remote", "add", "origin", origin_path],
        )
        .expect("remote should be added");

        git_ok(repo_path.as_path(), &["branch", "main"]).expect("main branch should be created");

        git_ok(
            repo_path.as_path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "push",
                "-u",
                "origin",
                "master",
            ],
        )
        .expect("git push should succeed");
        git_ok(
            repo_path.as_path(),
            &["-c", "protocol.file.allow=always", "fetch", "origin"],
        )
        .expect("git fetch should succeed");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let mut data = new_data_with_agent(agent);

        let next = OpenPRAction
            .execute(NormalMode, &mut data)
            .expect("open pr action should succeed");
        assert_eq!(next, AppMode::normal());
        assert!(data.git_op.agent_id.is_none());

        let next = OpenPRAction
            .execute(ScrollingMode, &mut data)
            .expect("open pr action should succeed");
        assert_eq!(next, ScrollingMode.into());
        assert!(data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_git_ok_formats_error_message_on_failure() {
        let dir = TempDir::new().expect("temp dir should be created");
        let err = git_ok(dir.path(), &["rev-parse", "--is-inside-work-tree"]).unwrap_err();
        assert!(err.to_string().contains("git [\"rev-parse\""));
    }

    #[test]
    fn test_rebase_action_returns_error_modal_without_selected_agent() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = RebaseAction
            .execute(NormalMode, &mut data)
            .expect("rebase action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = RebaseAction
            .execute(ScrollingMode, &mut data)
            .expect("rebase action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_rebase_action_returns_error_modal_when_not_git_workspace() {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = RebaseAction
            .execute(NormalMode, &mut data)
            .expect("rebase action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = RebaseAction
            .execute(ScrollingMode, &mut data)
            .expect("rebase action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_rebase_action_enters_branch_selector_and_sets_state() {
        let (_dir, repo_path) = init_repo();

        let repo = git::open_repository(&repo_path).expect("repo should open");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr
            .create("feature")
            .expect("feature branch should be created");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let agent_id = agent.id;
        let mut data = new_data_with_agent(agent);

        let next = RebaseAction
            .execute(NormalMode, &mut data)
            .expect("rebase action should succeed");
        assert!(is_rebase_branch_selector_mode(&next));
        assert!(!is_rebase_branch_selector_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, "master");
        assert!(data.review.filter.is_empty());
        assert!(!data.review.branches.is_empty());

        let next = RebaseAction
            .execute(ScrollingMode, &mut data)
            .expect("rebase action should succeed");
        assert!(is_rebase_branch_selector_mode(&next));
        assert!(!is_rebase_branch_selector_mode(&AppMode::normal()));
    }

    #[test]
    fn test_rebase_action_propagates_repo_open_errors_in_normal_and_scrolling() {
        let temp = TempDir::new().expect("temp dir should be created");
        let worktree_path = temp.path().join("not-a-repo");
        std::fs::create_dir_all(&worktree_path).expect("worktree should be created");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            worktree_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = RebaseAction
            .execute(NormalMode, &mut data)
            .expect_err("rebase should surface repository open failures");
        assert!(err.to_string().contains("Failed to open git repository"));

        let err = RebaseAction
            .execute(ScrollingMode, &mut data)
            .expect_err("rebase should surface repository open failures");
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn test_rebase_action_propagates_branch_selector_errors_in_normal_and_scrolling() {
        let (_dir, repo_path) = init_repo();
        let refs_dir = repo_path.join(".git").join("refs");
        let _guard = deny_all_permissions(&refs_dir);

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = RebaseAction
            .execute(NormalMode, &mut data)
            .expect_err("rebase should surface branch selector failures");
        assert!(err.to_string().contains("Failed to list local branches"));

        let err = RebaseAction
            .execute(ScrollingMode, &mut data)
            .expect_err("rebase should surface branch selector failures");
        assert!(err.to_string().contains("Failed to list local branches"));
    }

    #[test]
    fn test_merge_action_returns_error_modal_without_selected_agent() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = MergeAction
            .execute(NormalMode, &mut data)
            .expect("merge action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = MergeAction
            .execute(ScrollingMode, &mut data)
            .expect("merge action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_merge_action_returns_error_modal_when_not_git_workspace() {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = MergeAction
            .execute(NormalMode, &mut data)
            .expect("merge action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = MergeAction
            .execute(ScrollingMode, &mut data)
            .expect("merge action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_merge_action_enters_branch_selector_and_sets_state() {
        let (_dir, repo_path) = init_repo();

        let repo = git::open_repository(&repo_path).expect("repo should open");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr
            .create("feature")
            .expect("feature branch should be created");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let agent_id = agent.id;
        let mut data = new_data_with_agent(agent);

        let next = MergeAction
            .execute(NormalMode, &mut data)
            .expect("merge action should succeed");
        assert!(is_merge_branch_selector_mode(&next));
        assert!(!is_merge_branch_selector_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, "master");
        assert!(data.review.filter.is_empty());
        assert!(!data.review.branches.is_empty());

        let next = MergeAction
            .execute(ScrollingMode, &mut data)
            .expect("merge action should succeed");
        assert!(is_merge_branch_selector_mode(&next));
        assert!(!is_merge_branch_selector_mode(&AppMode::normal()));
    }

    #[test]
    fn test_merge_action_propagates_repo_open_errors_in_normal_and_scrolling() {
        let temp = TempDir::new().expect("temp dir should be created");
        let worktree_path = temp.path().join("not-a-repo");
        std::fs::create_dir_all(&worktree_path).expect("worktree should be created");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            worktree_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = MergeAction
            .execute(NormalMode, &mut data)
            .expect_err("merge should surface repository open failures");
        assert!(err.to_string().contains("Failed to open git repository"));

        let err = MergeAction
            .execute(ScrollingMode, &mut data)
            .expect_err("merge should surface repository open failures");
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_action_propagates_branch_selector_errors_in_normal_and_scrolling() {
        let (_dir, repo_path) = init_repo();
        let refs_dir = repo_path.join(".git").join("refs");
        let _guard = deny_all_permissions(&refs_dir);

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = MergeAction
            .execute(NormalMode, &mut data)
            .expect_err("merge should surface branch selector failures");
        assert!(err.to_string().contains("Failed to list local branches"));

        let err = MergeAction
            .execute(ScrollingMode, &mut data)
            .expect_err("merge should surface branch selector failures");
        assert!(err.to_string().contains("Failed to list local branches"));
    }

    #[test]
    fn test_switch_branch_action_returns_error_modal_without_selected_agent() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        );

        let next = SwitchBranchAction
            .execute(NormalMode, &mut data)
            .expect("switch branch action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = SwitchBranchAction
            .execute(ScrollingMode, &mut data)
            .expect("switch branch action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_switch_branch_action_returns_error_modal_when_not_git_workspace() {
        let mut agent = make_agent("agent-1");
        agent.workspace_kind = WorkspaceKind::PlainDir;
        let mut data = new_data_with_agent(agent);

        let next = SwitchBranchAction
            .execute(NormalMode, &mut data)
            .expect("switch branch action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
        let next = SwitchBranchAction
            .execute(ScrollingMode, &mut data)
            .expect("switch branch action should return a mode");
        assert!(is_error_modal(&next));
        assert!(!is_error_modal(&AppMode::normal()));
    }

    #[test]
    fn test_switch_branch_action_propagates_repo_open_errors_in_normal_and_scrolling() {
        let temp = TempDir::new().expect("temp dir should be created");
        let worktree_path = temp.path().join("not-a-repo");
        std::fs::create_dir_all(&worktree_path).expect("worktree should be created");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            worktree_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = SwitchBranchAction
            .execute(NormalMode, &mut data)
            .expect_err("switch branch should surface repository open failures");
        assert!(err.to_string().contains("Failed to open git repository"));

        let err = SwitchBranchAction
            .execute(ScrollingMode, &mut data)
            .expect_err("switch branch should surface repository open failures");
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[cfg(unix)]
    #[test]
    fn test_switch_branch_action_propagates_branch_selector_errors_in_normal_and_scrolling() {
        let (_dir, repo_path) = init_repo();
        let refs_dir = repo_path.join(".git").join("refs");
        let _guard = deny_all_permissions(&refs_dir);

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let mut data = new_data_with_agent(agent);

        let err = SwitchBranchAction
            .execute(NormalMode, &mut data)
            .expect_err("switch branch should surface branch selector failures");
        assert!(err.to_string().contains("Failed to list local branches"));

        let err = SwitchBranchAction
            .execute(ScrollingMode, &mut data)
            .expect_err("switch branch should surface branch selector failures");
        assert!(err.to_string().contains("Failed to list local branches"));
    }

    #[test]
    fn test_switch_branch_action_uses_root_ancestor_when_child_selected() {
        use crate::app::SidebarItem;

        let (_dir, repo_path) = init_repo();
        let repo = git::open_repository(&repo_path).expect("repo should open");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr
            .create("feature")
            .expect("feature branch should be created");

        let mut root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        root.collapsed = false;
        let root_id = root.id;

        let mut child = make_agent("child");
        child.parent_id = Some(root_id);
        let child_id = child.id;

        let mut storage = Storage::new();
        storage.add(root);
        storage.add(child);
        let mut data = AppData::new(
            Config::default(),
            storage,
            crate::app::Settings::default(),
            false,
        );

        let selected = data
            .sidebar_items()
            .iter()
            .position(|item| match item {
                SidebarItem::Agent(agent) => agent.info.agent.id == child_id,
                SidebarItem::Project(_) => false,
            })
            .expect("child agent should be visible in sidebar");
        data.selected = selected;

        let next = SwitchBranchAction
            .execute(NormalMode, &mut data)
            .expect("switch branch action should succeed");
        assert!(is_switch_branch_selector_mode(&next));
        assert!(!is_switch_branch_selector_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(root_id));
        assert_eq!(data.git_op.branch_name, "master");
    }

    #[test]
    fn test_switch_branch_action_enters_branch_selector_and_sets_state() {
        let (_dir, repo_path) = init_repo();

        let repo = git::open_repository(&repo_path).expect("repo should open");
        let branch_mgr = git::BranchManager::new(&repo);
        branch_mgr
            .create("feature")
            .expect("feature branch should be created");

        let agent = Agent::new(
            "agent-1".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo_path,
        );
        let agent_id = agent.id;
        let mut data = new_data_with_agent(agent);

        let next = SwitchBranchAction
            .execute(NormalMode, &mut data)
            .expect("switch branch action should succeed");
        assert!(is_switch_branch_selector_mode(&next));
        assert!(!is_switch_branch_selector_mode(&AppMode::normal()));
        assert_eq!(data.git_op.agent_id, Some(agent_id));
        assert_eq!(data.git_op.branch_name, "master");
        assert!(data.review.filter.is_empty());
        assert!(!data.review.branches.is_empty());

        let next = SwitchBranchAction
            .execute(ScrollingMode, &mut data)
            .expect("switch branch action should succeed");
        assert!(is_switch_branch_selector_mode(&next));
        assert!(!is_switch_branch_selector_mode(&AppMode::normal()));
    }
}
