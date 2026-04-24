//! Merge flow (branch selector + merge execution).

use crate::agent::{Agent, ChildConfig};
use crate::git;
use crate::mux::SessionManager;
use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode, MergeBranchSelectorMode, SuccessModalMode};

use super::super::Actions;

/// Result of a git merge operation
enum MergeResult {
    Success,
    Conflict,
    Failed(String),
}

pub(super) fn git_failure_message(stdout: &str, stderr: &str) -> String {
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    "Unknown error".to_string()
}

fn output_indicates_merge_conflict(combined_output: &str) -> bool {
    let has_conflict_marker = combined_output.contains("CONFLICT");
    let has_automatic_failure = combined_output.contains("Automatic merge failed");
    has_conflict_marker || has_automatic_failure
}

impl Actions {
    /// Start the merge flow - show branch selector (Ctrl+m or Ctrl+n)
    ///
    /// # Errors
    ///
    /// Returns an error if the git repository cannot be opened or branches cannot be listed.
    pub fn merge_branch(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to merge.".to_string(),
            }
            .into());
        };

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        debug!(branch = %current_branch, "Starting merge flow");

        // Fetch branches for selector from the selected agent's repository.
        let repo_path = agent
            .repo_root
            .clone()
            .or_else(|| git::repository_workspace_root(&agent.worktree_path).ok())
            .unwrap_or_else(|| agent.worktree_path.clone());
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_merge(agent_id, current_branch);
        app_data.review.start(branches);
        Ok(MergeBranchSelectorMode.into())
    }

    /// Execute the merge operation
    ///
    /// Merges the agent's branch INTO the target branch (e.g., feature -> master).
    /// If the target branch has a worktree, merges directly there.
    /// Otherwise, merges from the main repo.
    ///
    /// # Errors
    ///
    /// Returns an error if the merge operation fails
    pub fn execute_merge(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for merge".to_string(),
            }
            .into());
        };

        let Some(agent) = app_data.storage.get(agent_id) else {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "Agent not found".to_string(),
            }
            .into());
        };
        let repo_path = agent
            .repo_root
            .clone()
            .or_else(|| git::repository_workspace_root(&agent.worktree_path).ok())
            .unwrap_or_else(|| agent.worktree_path.clone());

        let source_branch = app_data.git_op.branch_name.clone(); // Agent's branch (e.g., tenex/feature)
        let target_branch = app_data.git_op.target_branch.clone(); // Branch to merge into (e.g., master)

        debug!(
            source = %source_branch,
            target = %target_branch,
            "Executing merge: {source_branch} -> {target_branch}"
        );

        // Check if target branch has a worktree
        if let Some(worktree_path) = Self::find_worktree_for_branch(&repo_path, &target_branch)? {
            Self::execute_merge_in_worktree(
                app_data,
                &source_branch,
                &target_branch,
                &worktree_path,
            )
        } else {
            Self::execute_merge_in_main_repo(app_data, &repo_path, &source_branch, &target_branch)
        }
    }

    /// Find the worktree path for a branch, if one exists
    pub(super) fn find_worktree_for_branch(
        repo_path: &std::path::Path,
        branch: &str,
    ) -> Result<Option<std::path::PathBuf>> {
        let output = crate::git::git_command()
            .args(["worktree", "list", "--porcelain"])
            .current_dir(repo_path)
            .output()
            .context("Failed to list worktrees")?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut current_worktree: Option<std::path::PathBuf> = None;

        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                current_worktree = Some(std::path::PathBuf::from(path));
            } else if let Some(worktree_branch) = line.strip_prefix("branch refs/heads/")
                && worktree_branch == branch
            {
                return Ok(current_worktree);
            }
        }

        Ok(None)
    }

    /// Execute merge directly in a worktree (when target branch is checked out there)
    fn execute_merge_in_worktree(
        app_data: &mut AppData,
        source_branch: &str,
        target_branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<AppMode> {
        debug!(source = %source_branch, target = %target_branch, worktree = %worktree_path.display(), "Merging in worktree");

        // Merge directly in the worktree
        let merge_output = crate::git::git_command()
            .args([
                "merge",
                source_branch,
                "-m",
                &format!("Merge {source_branch} into {target_branch}"),
            ])
            .current_dir(worktree_path)
            .output()
            .context("Failed to execute merge")?;

        if !merge_output.status.success() {
            let stdout = String::from_utf8_lossy(&merge_output.stdout);
            let stderr = String::from_utf8_lossy(&merge_output.stderr);
            let combined = format!("{stdout}{stderr}");

            // Check if there are merge conflicts (git outputs to stdout)
            if output_indicates_merge_conflict(&combined) {
                info!(source = %source_branch, target = %target_branch, "Merge has conflicts - spawning terminal");

                return Self::spawn_merge_conflict_terminal_in_worktree(
                    app_data,
                    source_branch,
                    target_branch,
                    worktree_path,
                );
            }

            // Show error with both stdout and stderr for context
            let error_msg = git_failure_message(&stdout, &stderr);
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: format!("Merge failed: {error_msg}"),
            }
            .into());
        }

        info!(
            source = %source_branch,
            target = %target_branch,
            "Merge successful in worktree"
        );
        app_data.git_op.clear();
        app_data.review.clear();
        Ok(SuccessModalMode {
            message: format!("Merged {source_branch} into {target_branch}"),
        }
        .into())
    }

    /// Spawn a terminal for merge conflict resolution in a worktree
    fn spawn_merge_conflict_terminal_in_worktree(
        app_data: &mut AppData,
        source_branch: &str,
        target_branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<AppMode> {
        let worktree_path = worktree_path
            .canonicalize()
            .unwrap_or_else(|_| worktree_path.to_path_buf());
        let worktree_path_display = worktree_path.display().to_string();

        // Find the agent that owns this worktree
        let root_snapshot = app_data.storage.agents.iter().find_map(|agent| {
            let agent_worktree = agent
                .worktree_path
                .canonicalize()
                .unwrap_or_else(|_| agent.worktree_path.clone());
            if agent_worktree != worktree_path || !agent.is_root() {
                return None;
            }

            Some((
                agent.id,
                agent.mux_session.clone(),
                agent.branch.clone(),
                agent.repo_root.clone(),
                agent.runtime,
                agent.effective_runtime_scope().to_string(),
            ))
        });

        if let Some((root_id, root_session, branch, repo_root, runtime, runtime_scope)) =
            root_snapshot
        {
            let title = format!("Merge Conflict: {source_branch} -> {target_branch}");

            // Reserve a window index
            let window_index = app_data.storage.reserve_window_indices(root_id);

            // Create child agent marked as terminal
            let mut terminal = Agent::new_child(
                title.clone(),
                "terminal".to_string(),
                branch,
                worktree_path,
                ChildConfig {
                    parent_id: root_id,
                    mux_session: root_session.clone(),
                    window_index,
                    repo_root,
                },
            );
            terminal.is_terminal = true;
            terminal.runtime = runtime;
            terminal.runtime_scope = runtime_scope;

            // Create session manager and window
            let session_manager = SessionManager::new();
            crate::runtime::ensure_runtime_ready(&terminal, &app_data.settings)?;
            let terminal_command = crate::runtime::build_terminal_command(
                &terminal,
                Some("git status"),
                &app_data.settings,
            );
            let actual_index = session_manager.create_window(
                &root_session,
                &title,
                terminal.worktree_path.as_path(),
                terminal_command.as_deref(),
            )?;

            // Resize the new window to match preview dimensions
            if let Some((width, height)) = app_data.ui.preview_dimensions {
                let window_target = SessionManager::window_target(&root_session, actual_index);
                let _ = session_manager.resize_window(&window_target, width, height);
            }

            // Update window index if it differs
            terminal.window_index = Some(actual_index);

            let window_target = SessionManager::window_target(&root_session, actual_index);
            if terminal_command.is_none() {
                let _ = session_manager.send_keys_and_submit(&window_target, "git status");
            }

            app_data.storage.add(terminal);

            // Expand the parent to show the new terminal
            let _ = app_data.storage.set_collapsed(root_id, false);

            app_data.storage.save()?;

            app_data.set_status(format!("Opened terminal for conflict resolution: {title}"));
        } else {
            // No agent owns this worktree, just show a message
            app_data.set_status(format!(
                "Merge conflict in {target_branch}. Resolve in: {worktree_path_display}"
            ));
        }

        app_data.git_op.clear();
        app_data.review.clear();

        Ok(AppMode::normal())
    }

    /// Execute merge from main repo (when target branch has no worktree)
    fn execute_merge_in_main_repo(
        app_data: &mut AppData,
        repo_path: &std::path::Path,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<AppMode> {
        debug!(source = %source_branch, target = %target_branch, "Merging in main repo");

        // Prepare: stash changes and get current branch
        let did_stash = Self::git_stash_push(repo_path)?;
        let original_branch = Self::git_get_current_branch(repo_path)?;

        // Checkout target branch
        if !Self::git_checkout(repo_path, target_branch)? {
            Self::restore_git_state(repo_path, did_stash);
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: format!("Failed to checkout {target_branch}"),
            }
            .into());
        }

        // Attempt merge
        let merge_result = Self::git_merge(
            repo_path,
            source_branch,
            target_branch,
            &original_branch,
            did_stash,
        );

        let next = match merge_result {
            MergeResult::Success => {
                Self::restore_git_state(repo_path, did_stash);
                info!(source = %source_branch, target = %target_branch, "Merge successful");
                SuccessModalMode {
                    message: format!("Merged {source_branch} into {target_branch}"),
                }
                .into()
            }
            MergeResult::Conflict => {
                // Stay on target branch, don't restore stash - user needs to resolve.
                return Self::spawn_conflict_terminal(
                    app_data,
                    &format!("Merge Conflict: {source_branch} -> {target_branch}"),
                    "git status",
                );
            }
            MergeResult::Failed(error_msg) => {
                Self::git_checkout(repo_path, &original_branch)?;
                Self::restore_git_state(repo_path, did_stash);
                ErrorModalMode {
                    message: format!("Merge failed: {error_msg}"),
                }
                .into()
            }
        };

        app_data.git_op.clear();
        app_data.review.clear();
        Ok(next)
    }

    /// Stash any uncommitted changes
    fn git_stash_push(repo_path: &std::path::Path) -> Result<bool> {
        let stash_output = crate::git::git_command()
            .args(["stash", "push", "-m", "tenex-merge-temp"])
            .current_dir(repo_path)
            .output()
            .context("Failed to stash changes")?;

        Ok(String::from_utf8_lossy(&stash_output.stdout).contains("Saved working"))
    }

    /// Get current branch name
    fn git_get_current_branch(repo_path: &std::path::Path) -> Result<String> {
        let output = crate::git::git_command()
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo_path)
            .output()
            .context("Failed to get current branch")?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Checkout a branch, returns success status
    fn git_checkout(repo_path: &std::path::Path, branch: &str) -> Result<bool> {
        let output = crate::git::git_command()
            .args(["checkout", branch])
            .current_dir(repo_path)
            .output()
            .context("Failed to checkout branch")?;

        Ok(output.status.success())
    }

    /// Restore git state (checkout original branch and pop stash)
    fn restore_git_state(repo_path: &std::path::Path, did_stash: bool) {
        if did_stash {
            let _ = crate::git::git_command()
                .args(["stash", "pop"])
                .current_dir(repo_path)
                .output();
        }
    }

    /// Perform git merge and return result
    fn git_merge(
        repo_path: &std::path::Path,
        source_branch: &str,
        target_branch: &str,
        original_branch: &str,
        did_stash: bool,
    ) -> MergeResult {
        let merge_output = match crate::git::git_command()
            .args([
                "merge",
                source_branch,
                "-m",
                &format!("Merge {source_branch} into {target_branch}"),
            ])
            .current_dir(repo_path)
            .output()
        {
            Ok(output) => output,
            Err(e) => return MergeResult::Failed(e.to_string()),
        };

        if merge_output.status.success() {
            // Go back to original branch after successful merge
            let _ = crate::git::git_command()
                .args(["checkout", original_branch])
                .current_dir(repo_path)
                .output();
            return MergeResult::Success;
        }

        let stdout = String::from_utf8_lossy(&merge_output.stdout);
        let stderr = String::from_utf8_lossy(&merge_output.stderr);
        let combined = format!("{stdout}{stderr}");

        if output_indicates_merge_conflict(&combined) {
            MergeResult::Conflict
        } else {
            // Restore state before returning error
            let _ = crate::git::git_command()
                .args(["checkout", original_branch])
                .current_dir(repo_path)
                .output();
            if did_stash {
                let _ = crate::git::git_command()
                    .args(["stash", "pop"])
                    .current_dir(repo_path)
                    .output();
            }

            let error_msg = git_failure_message(&stdout, &stderr);
            MergeResult::Failed(error_msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn assert_app_mode_variant(actual: &AppMode, expected: &AppMode) {
        assert_eq!(
            std::mem::discriminant(actual),
            std::mem::discriminant(expected)
        );
    }

    fn assert_merge_result_variant(actual: &MergeResult, expected: &MergeResult) {
        assert_eq!(
            std::mem::discriminant(actual),
            std::mem::discriminant(expected)
        );
    }

    fn error_modal_message(mode: &AppMode) -> Option<&str> {
        match mode {
            AppMode::ErrorModal(state) => Some(state.message.as_str()),
            _ => None,
        }
    }

    fn sleep_command(seconds: u32) -> Vec<String> {
        #[cfg(windows)]
        {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!("Start-Sleep -Seconds {seconds}"),
            ]
        }
        #[cfg(not(windows))]
        {
            vec!["sleep".to_string(), seconds.to_string()]
        }
    }

    fn git(repo: &Path, args: &[&str]) -> std::process::Output {
        crate::git::git_command()
            .args(args)
            .current_dir(repo)
            .output()
            .expect("failed to execute git command")
    }

    fn git_ok(repo: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let output = git(repo, args);
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
        Err(format!(
            "git {args:?} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }

    fn init_repo() -> TempDir {
        let dir = TempDir::new().expect("create repo temp dir");
        git_ok(dir.path(), &["init", "-q"]).expect("git init");
        git_ok(dir.path(), &["checkout", "-B", "master"]).expect("git checkout master");
        git_ok(dir.path(), &["config", "user.email", "test@test.com"]).expect("git config email");
        git_ok(dir.path(), &["config", "user.name", "Test"]).expect("git config name");
        git_ok(dir.path(), &["config", "commit.gpgsign", "false"]).expect("git config gpgsign");

        let hooks = dir.path().join(".git").join("hooks-tenex-tests");
        std::fs::create_dir_all(&hooks).expect("create hooks dir");
        git_ok(
            dir.path(),
            &[
                "config",
                "core.hooksPath",
                hooks.to_str().expect("hooks path not utf-8"),
            ],
        )
        .expect("failed to set core.hooksPath");

        std::fs::write(dir.path().join("README.md"), "test\n").expect("write README");
        git_ok(dir.path(), &["add", "."]).expect("git add");
        git_ok(dir.path(), &["commit", "-q", "--no-verify", "-m", "init"]).expect("git commit");
        dir
    }

    fn init_repo_in_subdir() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("create repo temp dir");
        let repo_root = dir.path().join("repo");
        fs::create_dir_all(&repo_root).expect("create repo root dir");

        git_ok(&repo_root, &["init", "-q"]).expect("git init");
        git_ok(&repo_root, &["checkout", "-B", "master"]).expect("git checkout master");
        git_ok(&repo_root, &["config", "user.email", "test@test.com"]).expect("git config email");
        git_ok(&repo_root, &["config", "user.name", "Test"]).expect("git config name");
        git_ok(&repo_root, &["config", "commit.gpgsign", "false"]).expect("git config gpgsign");

        let hooks = repo_root.join(".git").join("hooks-tenex-tests");
        fs::create_dir_all(&hooks).expect("create hooks dir");
        git_ok(
            &repo_root,
            &[
                "config",
                "core.hooksPath",
                hooks.to_str().expect("hooks path not utf-8"),
            ],
        )
        .expect("failed to set core.hooksPath");

        fs::write(repo_root.join("README.md"), "test\n").expect("write README");
        git_ok(&repo_root, &["add", "."]).expect("git add");
        git_ok(&repo_root, &["commit", "-q", "--no-verify", "-m", "init"]).expect("git commit");
        (dir, repo_root)
    }

    fn write_executable_script(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, contents).expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path).expect("script metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).expect("chmod script");
        }
        path
    }

    fn create_test_app(repo_root: &Path, state_path: PathBuf) -> crate::App {
        let config = Config {
            worktree_dir: repo_root.join(".tenex-test-worktrees"),
            branch_prefix: "tenex-test-merge/".to_string(),
            ..Config::default()
        };
        let storage = Storage::with_path(state_path);
        crate::App::new(config, storage, Settings::default(), false)
    }

    fn select_agent(app: &mut crate::App, agent_id: uuid::Uuid) {
        let index = app
            .data
            .sidebar_items()
            .iter()
            .position(|item| match item {
                crate::app::SidebarItem::Agent(agent) => agent.info.agent.id == agent_id,
                crate::app::SidebarItem::Project(_) => false,
            })
            .expect("expected agent to be present in sidebar");
        app.data.selected = index;
    }

    #[test]
    fn test_git_failure_message_prefers_stderr_then_stdout() {
        assert_eq!(git_failure_message("ignored", " boom "), "boom");
        assert_eq!(git_failure_message(" boom ", "   "), "boom");
        assert_eq!(git_failure_message("   ", "   "), "Unknown error");
    }

    #[test]
    fn test_output_indicates_merge_conflict_recognizes_markers() {
        assert!(output_indicates_merge_conflict(
            "CONFLICT (content): Merge conflict"
        ));
        assert!(output_indicates_merge_conflict(
            "Automatic merge failed; fix conflicts"
        ));
        assert!(!output_indicates_merge_conflict(
            "merge failed for other reason"
        ));
    }

    #[test]
    fn test_git_ok_formats_error_output() {
        let repo = init_repo();
        let err = git_ok(repo.path(), &["show", "does-not-exist"]).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("git"));
        assert!(message.contains("failed"));
    }

    #[test]
    fn test_merge_branch_requires_selected_agent() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let next = Actions::merge_branch(&mut app.data).expect("merge_branch");
        assert_app_mode_variant(
            &next,
            &AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }),
        );
        assert!(
            error_modal_message(&next)
                .unwrap_or_default()
                .contains("No agent selected")
        );
        assert!(error_modal_message(&AppMode::normal()).is_none());
    }

    #[test]
    fn test_merge_branch_starts_selector_state() {
        let repo = init_repo();
        git_ok(repo.path(), &["branch", "feature"]).expect("create feature branch");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "merge-agent".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);
        select_agent(&mut app, agent_id);

        let next = Actions::merge_branch(&mut app.data).expect("merge_branch");
        assert_app_mode_variant(
            &next,
            &AppMode::MergeBranchSelector(MergeBranchSelectorMode),
        );
        assert_eq!(app.data.git_op.agent_id, Some(agent_id));
        assert_eq!(app.data.git_op.branch_name, "master");
        assert!(app.data.git_op.operation_type.is_some());
        assert_eq!(
            format!("{:?}", app.data.git_op.operation_type),
            "Some(Merge)"
        );
        assert!(!app.data.review.branches.is_empty());
    }

    #[test]
    fn test_merge_branch_uses_workspace_root_when_repo_root_missing() {
        let repo = init_repo();
        git_ok(repo.path(), &["branch", "feature"]).expect("create feature branch");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let agent_id = {
            let agent = Agent::new(
                "merge-agent".to_string(),
                "echo".to_string(),
                "master".to_string(),
                repo.path().to_path_buf(),
            );
            let id = agent.id;
            app.data.storage.add(agent);
            id
        };
        select_agent(&mut app, agent_id);

        let next = Actions::merge_branch(&mut app.data).expect("merge_branch");
        assert_app_mode_variant(
            &next,
            &AppMode::MergeBranchSelector(MergeBranchSelectorMode),
        );
    }

    #[test]
    fn test_merge_branch_falls_back_to_worktree_path_and_reports_repo_open_error() {
        let repo = TempDir::new().expect("create temp dir");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let agent_id = {
            let agent = Agent::new(
                "merge-agent".to_string(),
                "echo".to_string(),
                "master".to_string(),
                repo.path().to_path_buf(),
            );
            let id = agent.id;
            app.data.storage.add(agent);
            id
        };
        select_agent(&mut app, agent_id);

        let err = Actions::merge_branch(&mut app.data).unwrap_err();
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[test]
    fn test_merge_branch_propagates_branch_list_error() {
        let repo = init_repo();
        git_ok(repo.path(), &["branch", "feature"]).expect("create feature branch");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let agent_id = {
            let mut agent = Agent::new(
                "merge-agent".to_string(),
                "echo".to_string(),
                "master".to_string(),
                repo.path().to_path_buf(),
            );
            agent.repo_root = Some(repo.path().to_path_buf());
            let id = agent.id;
            app.data.storage.add(agent);
            id
        };
        select_agent(&mut app, agent_id);

        let result = crate::git::with_list_for_selector_override_for_tests(
            |_repo| Err(anyhow::anyhow!("boom")),
            || Actions::merge_branch(&mut app.data),
        );
        let err = result.unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_find_worktree_for_branch_returns_none_when_missing() {
        let repo = init_repo();
        assert!(
            Actions::find_worktree_for_branch(repo.path(), "nope")
                .expect("find_worktree_for_branch")
                .is_none()
        );
    }

    #[test]
    fn test_execute_merge_requires_agent_id() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        app.data.git_op.agent_id = None;
        app.data.review.start(Vec::new());

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }),
        );
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.review.branches.is_empty());
    }

    #[test]
    fn test_execute_merge_errors_when_agent_missing() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.review.start(Vec::new());

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }),
        );
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.review.branches.is_empty());
    }

    #[test]
    fn test_execute_merge_in_worktree_success() {
        let repo = init_repo();
        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");

        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        std::fs::write(feature_wt.join("feature.txt"), "feature\n").expect("write feature file");
        git_ok(&feature_wt, &["add", "feature.txt"]).expect("git add feature file");
        git_ok(
            &feature_wt,
            &["commit", "-q", "--no-verify", "-m", "feature"],
        )
        .expect("failed to commit feature changes");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            feature_wt,
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "master".to_string();

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::SuccessModal(SuccessModalMode {
                message: String::new(),
            }),
        );
        assert!(app.data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_execute_merge_in_worktree_conflict_sets_status_when_no_owner() {
        let repo = init_repo();
        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");

        std::fs::write(repo.path().join("shared.txt"), "base\n").expect("write base file");
        git_ok(repo.path(), &["add", "shared.txt"]).expect("git add shared");
        git_ok(repo.path(), &["commit", "-q", "--no-verify", "-m", "base"])
            .expect("git commit base");

        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        std::fs::write(feature_wt.join("shared.txt"), "feature\n").expect("write feature shared");
        git_ok(&feature_wt, &["add", "shared.txt"]).expect("git add feature shared");
        git_ok(
            &feature_wt,
            &["commit", "-q", "--no-verify", "-m", "feature"],
        )
        .expect("failed to commit feature changes");

        std::fs::write(repo.path().join("shared.txt"), "master\n").expect("write master shared");
        git_ok(repo.path(), &["add", "shared.txt"]).expect("git add master shared");
        git_ok(
            repo.path(),
            &["commit", "-q", "--no-verify", "-m", "master"],
        )
        .expect("failed to commit master changes");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut feature_agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            feature_wt,
        );
        feature_agent.repo_root = Some(repo.path().to_path_buf());
        let feature_id = feature_agent.id;
        app.data.storage.add(feature_agent);

        app.data.git_op.agent_id = Some(feature_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "master".to_string();

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_eq!(next, AppMode::normal());
        assert!(
            app.data
                .ui
                .status_message
                .as_deref()
                .is_some_and(|status| status.contains("Merge conflict"))
        );
    }

    #[test]
    fn test_execute_merge_in_worktree_conflict_spawns_terminal_for_owner() {
        let _guard = crate::test_support::lock_mux_test_environment();

        let repo = init_repo();
        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");

        std::fs::write(repo.path().join("shared.txt"), "base\n").expect("write base file");
        git_ok(repo.path(), &["add", "shared.txt"]).expect("git add shared");
        git_ok(repo.path(), &["commit", "-q", "--no-verify", "-m", "base"])
            .expect("git commit base");

        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        std::fs::write(feature_wt.join("shared.txt"), "feature\n").expect("write feature shared");
        git_ok(&feature_wt, &["add", "shared.txt"]).expect("git add feature shared");
        git_ok(
            &feature_wt,
            &["commit", "-q", "--no-verify", "-m", "feature"],
        )
        .expect("failed to commit feature changes");

        std::fs::write(repo.path().join("shared.txt"), "master\n").expect("write master shared");
        git_ok(repo.path(), &["add", "shared.txt"]).expect("git add master shared");
        git_ok(
            repo.path(),
            &["commit", "-q", "--no-verify", "-m", "master"],
        )
        .expect("failed to commit master changes");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);
        app.data.ui.preview_dimensions = Some((80, 24));

        let mut master_owner = Agent::new(
            "master-owner".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        master_owner.repo_root = Some(repo.path().to_path_buf());
        let root_id = master_owner.id;
        let root_session = master_owner.mux_session.clone();
        app.data.storage.add(master_owner);

        let manager = SessionManager::new();
        let command = sleep_command(30);
        manager
            .create(&root_session, repo.path(), Some(&command))
            .expect("create root mux session");

        let mut feature_agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            feature_wt,
        );
        feature_agent.repo_root = Some(repo.path().to_path_buf());
        let feature_id = feature_agent.id;
        app.data.storage.add(feature_agent);

        app.data.git_op.agent_id = Some(feature_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "master".to_string();

        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        let next = tracing::dispatcher::with_default(&dispatch, || {
            Actions::execute_merge(&mut app.data).expect("execute_merge")
        });
        assert_eq!(next, AppMode::normal());

        let spawned = app
            .data
            .storage
            .iter()
            .any(|agent| agent.parent_id == Some(root_id) && agent.is_terminal);
        assert!(spawned);

        let _ = manager.kill(&root_session);
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_skips_resize_when_preview_dimensions_missing() {
        let _guard = crate::test_support::lock_mux_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);
        app.data.ui.preview_dimensions = None;

        let mut owner = Agent::new(
            "owner".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        owner.repo_root = Some(repo.path().to_path_buf());
        let root_id = owner.id;
        let session = owner.mux_session.clone();
        app.data.storage.add(owner);

        let manager = SessionManager::new();
        manager
            .create(&session, repo.path(), Some(&sleep_command(30)))
            .expect("create mux session");

        let next = Actions::spawn_merge_conflict_terminal_in_worktree(
            &mut app.data,
            "feature",
            "master",
            repo.path(),
        )
        .expect("spawn conflict terminal");
        assert_eq!(next, AppMode::normal());

        let spawned = app
            .data
            .storage
            .iter()
            .any(|agent| agent.parent_id == Some(root_id) && agent.is_terminal);
        assert!(spawned);

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_uses_terminal_command_when_runtime_docker() {
        let _guard_mux = crate::test_support::lock_mux_test_environment();
        let _guard_env = crate::test_support::lock_env_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut owner = Agent::new(
            "owner".to_string(),
            "terminal".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        owner.repo_root = Some(repo.path().to_path_buf());
        owner.runtime = crate::agent::AgentRuntime::Docker;
        let root_id = owner.id;
        let session = owner.mux_session.clone();
        app.data.storage.add(owner);

        let manager = SessionManager::new();
        manager
            .create(&session, repo.path(), Some(&sleep_command(30)))
            .expect("create mux session");

        #[expect(
            clippy::literal_string_with_formatting_args,
            reason = "Shell scripts use braces for parameter expansion."
        )]
        let fake_docker_script = r#"#!/usr/bin/env bash
	set -euo pipefail
	cmd="${1:-}"
	shift || true
	case "$cmd" in
	  version)
    exit 0
    ;;
  image)
    if [ "${1:-}" = "inspect" ]; then
      echo "No such image" >&2
      exit 1
    fi
    ;;
  build)
    # Consume stdin (Dockerfile) then report success.
    cat >/dev/null || true
    exit 0
    ;;
  inspect)
    echo "No such object" >&2
    exit 1
    ;;
  run)
    echo "fake-container-id"
    exit 0
    ;;
  exec)
    # Keep the pane alive until the mux session is killed.
    sleep 30
    exit 0
    ;;
  start|rm)
    exit 0
    ;;
	esac
	exit 0
"#;
        let fake_docker = write_executable_script(&repo, "fake-docker", fake_docker_script);

        let next = crate::runtime::with_docker_program_override_for_tests(fake_docker, || {
            Actions::spawn_merge_conflict_terminal_in_worktree(
                &mut app.data,
                "feature",
                "master",
                repo.path(),
            )
            .expect("spawn conflict terminal")
        });
        assert_eq!(next, AppMode::normal());

        let spawned = app
            .data
            .storage
            .iter()
            .any(|agent| agent.parent_id == Some(root_id) && agent.is_terminal);
        assert!(spawned);

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_ignores_non_root_agents_in_worktree() {
        let _guard = crate::test_support::lock_mux_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);
        app.data.ui.preview_dimensions = None;

        let mut owner = Agent::new(
            "owner".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        owner.repo_root = Some(repo.path().to_path_buf());
        let root_id = owner.id;
        let session = owner.mux_session.clone();
        let repo_root = owner.repo_root.clone();

        let child = Agent::new_child(
            "child".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
            ChildConfig {
                parent_id: root_id,
                mux_session: session.clone(),
                window_index: 0,
                repo_root,
            },
        );

        app.data.storage.add(child);
        app.data.storage.add(owner);

        let manager = SessionManager::new();
        manager
            .create(&session, repo.path(), Some(&sleep_command(30)))
            .expect("create mux session");

        let next = Actions::spawn_merge_conflict_terminal_in_worktree(
            &mut app.data,
            "feature",
            "master",
            repo.path(),
        )
        .expect("spawn conflict terminal");
        assert_eq!(next, AppMode::normal());

        let spawned = app
            .data
            .storage
            .iter()
            .any(|agent| agent.parent_id == Some(root_id) && agent.is_terminal);
        assert!(spawned);

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_falls_back_when_canonicalize_fails() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let missing_agent_worktree = repo.path().join("missing-agent-worktree");
        let missing_conflict_worktree = repo.path().join("missing-conflict-worktree");

        let agent = Agent::new(
            "owner".to_string(),
            "echo".to_string(),
            "master".to_string(),
            missing_agent_worktree,
        );
        app.data.storage.add(agent);

        let next = Actions::spawn_merge_conflict_terminal_in_worktree(
            &mut app.data,
            "feature",
            "master",
            &missing_conflict_worktree,
        )
        .expect("spawn conflict terminal");
        assert_eq!(next, AppMode::normal());
        assert!(
            app.data
                .ui
                .status_message
                .as_deref()
                .unwrap_or_default()
                .contains("Resolve in")
        );
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_propagates_runtime_ready_errors() {
        let _guard_env = crate::test_support::lock_env_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut owner = Agent::new(
            "owner".to_string(),
            "terminal".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        owner.repo_root = Some(repo.path().to_path_buf());
        owner.runtime = crate::agent::AgentRuntime::Docker;
        app.data.storage.add(owner);

        let missing_docker = repo.path().join("missing-docker");
        let err = crate::runtime::with_docker_program_override_for_tests(missing_docker, || {
            Actions::spawn_merge_conflict_terminal_in_worktree(
                &mut app.data,
                "feature",
                "master",
                repo.path(),
            )
            .unwrap_err()
        });
        assert!(!err.to_string().trim().is_empty());
    }

    #[test]
    fn test_execute_merge_in_main_repo_propagates_stash_spawn_errors() {
        let _guard_env = crate::test_support::lock_env_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let missing_git = repo.path().join("missing-git");
        let err = crate::git::with_git_program_override_for_tests(missing_git, || {
            Actions::execute_merge_in_main_repo(&mut app.data, repo.path(), "feature", "master")
                .unwrap_err()
        });
        assert!(err.to_string().contains("Failed to stash changes"));
    }

    #[test]
    fn test_execute_merge_in_main_repo_propagates_rev_parse_spawn_errors() {
        let _guard_env = crate::test_support::lock_env_test_environment();

        let (dir, repo_root) = init_repo_in_subdir();
        let state_path = dir.path().join("state.json");
        let mut app = create_test_app(&repo_root, state_path);

        let fake_git = write_executable_script(
            &dir,
            "fake-git",
            &format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${{1:-}}" = "stash" ] && [ "${{2:-}}" = "push" ]; then
  echo "Saved working directory and index state WIP on master"
  cd /
  rm -rf "{repo_root}"
  exit 0
fi
exit 0
"#,
                repo_root = repo_root.display()
            ),
        );

        let err = crate::git::with_git_program_override_for_tests(fake_git, || {
            Actions::execute_merge_in_main_repo(&mut app.data, &repo_root, "feature", "master")
                .unwrap_err()
        });
        assert!(err.to_string().contains("Failed to get current branch"));
    }

    #[test]
    fn test_execute_merge_in_main_repo_propagates_checkout_spawn_errors() {
        let _guard_env = crate::test_support::lock_env_test_environment();

        let (dir, repo_root) = init_repo_in_subdir();
        let state_path = dir.path().join("state.json");
        let mut app = create_test_app(&repo_root, state_path);

        let fake_git = write_executable_script(
            &dir,
            "fake-git",
            &format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${{1:-}}" = "stash" ] && [ "${{2:-}}" = "push" ]; then
  echo "Saved working directory and index state WIP on master"
  exit 0
fi
if [ "${{1:-}}" = "rev-parse" ]; then
  echo "master"
  cd /
  rm -rf "{repo_root}"
  exit 0
fi
exit 0
"#,
                repo_root = repo_root.display()
            ),
        );

        let err = crate::git::with_git_program_override_for_tests(fake_git, || {
            Actions::execute_merge_in_main_repo(&mut app.data, &repo_root, "feature", "master")
                .unwrap_err()
        });
        assert!(err.to_string().contains("Failed to checkout branch"));
    }

    #[test]
    fn test_execute_merge_in_main_repo_merge_failed_checkout_spawn_errors() {
        let _guard_env = crate::test_support::lock_env_test_environment();

        let (dir, repo_root) = init_repo_in_subdir();
        let state_path = dir.path().join("state.json");
        let mut app = create_test_app(&repo_root, state_path);

        let fake_git = write_executable_script(
            &dir,
            "fake-git",
            &format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
if [ "${{1:-}}" = "stash" ] && [ "${{2:-}}" = "push" ]; then
  echo "Saved working directory and index state WIP on master"
  exit 0
fi
if [ "${{1:-}}" = "rev-parse" ]; then
  echo "master"
  exit 0
fi
if [ "${{1:-}}" = "checkout" ]; then
  cd /
  rm -rf "{repo_root}"
  exit 0
fi
exit 0
"#,
                repo_root = repo_root.display()
            ),
        );

        let err = crate::git::with_git_program_override_for_tests(fake_git, || {
            Actions::execute_merge_in_main_repo(&mut app.data, &repo_root, "feature", "master")
                .unwrap_err()
        });
        assert!(!err.to_string().trim().is_empty());
    }

    #[test]
    fn test_merge_branch_and_execute_merge_evaluate_tracing_fields_when_enabled() {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);

        let repo = init_repo();

        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");
        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "merge-agent".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);
        select_agent(&mut app, agent_id);

        tracing::dispatcher::with_default(&dispatch, || {
            let next = Actions::merge_branch(&mut app.data).expect("merge_branch");
            assert_app_mode_variant(
                &next,
                &AppMode::MergeBranchSelector(MergeBranchSelectorMode),
            );
        });

        let mut feature_agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            feature_wt,
        );
        feature_agent.repo_root = Some(repo.path().to_path_buf());
        let feature_id = feature_agent.id;
        app.data.storage.add(feature_agent);

        app.data.git_op.agent_id = Some(feature_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "master".to_string();

        tracing::dispatcher::with_default(&dispatch, || {
            let _ = Actions::execute_merge(&mut app.data).expect("execute_merge");
        });
    }

    #[test]
    fn test_git_merge_returns_failed_when_command_cannot_run() {
        let result = Actions::git_merge(
            Path::new("/path/that/does/not/exist"),
            "source",
            "target",
            "master",
            false,
        );

        assert_merge_result_variant(&result, &MergeResult::Failed(String::new()));
    }

    #[test]
    fn test_restore_git_state_runs_stash_pop_when_did_stash_true() {
        let temp = TempDir::new().expect("create temp dir");
        Actions::restore_git_state(temp.path(), true);
    }

    #[test]
    fn test_git_merge_pops_stash_on_failure_when_did_stash_true() {
        let temp = TempDir::new().expect("create temp dir");
        let result = Actions::git_merge(temp.path(), "source", "target", "master", true);
        assert_merge_result_variant(&result, &MergeResult::Failed(String::new()));
    }

    #[test]
    fn test_find_worktree_for_branch_finds_expected_path() {
        let repo = init_repo();
        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");

        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        let found = Actions::find_worktree_for_branch(repo.path(), "feature")
            .expect("find_worktree_for_branch")
            .expect("expected to find worktree for feature");
        let found = found.canonicalize().unwrap_or(found);
        let feature_wt = feature_wt.canonicalize().unwrap_or(feature_wt);
        assert_eq!(found, feature_wt);
    }

    #[test]
    fn test_execute_merge_in_worktree_reports_merge_failure() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            repo.path().to_path_buf(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "does-not-exist".to_string();
        app.data.git_op.target_branch = "master".to_string();

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_execute_merge_in_worktree_errors_when_worktree_missing() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let err = Actions::execute_merge_in_worktree(
            &mut app.data,
            "source",
            "target",
            Path::new("/path/that/does/not/exist"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Failed to execute merge"));
    }

    #[test]
    fn test_execute_merge_in_main_repo_checkout_failure_returns_error_modal() {
        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            repo.path().to_path_buf(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "master".to_string();
        app.data.git_op.target_branch = "definitely-not-a-branch".to_string();

        // Ensure there is no worktree for the non-existent target branch.
        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_execute_merge_in_main_repo_success_for_unchecked_out_target() {
        let repo = init_repo();
        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");

        git_ok(repo.path(), &["branch", "develop"]).expect("create develop branch");

        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        std::fs::write(feature_wt.join("feature.txt"), "feature\n").expect("write feature file");
        git_ok(&feature_wt, &["add", "feature.txt"]).expect("git add feature file");
        git_ok(
            &feature_wt,
            &["commit", "-q", "--no-verify", "-m", "feature"],
        )
        .expect("failed to commit feature changes");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            feature_wt,
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "develop".to_string();

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::SuccessModal(SuccessModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_execute_merge_in_main_repo_conflict_spawns_terminal() {
        let _guard = crate::test_support::lock_mux_test_environment();

        let repo = init_repo();
        let worktrees = repo.path().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("create worktrees dir");

        std::fs::write(repo.path().join("shared.txt"), "base\n").expect("write base file");
        git_ok(repo.path(), &["add", "shared.txt"]).expect("git add shared");
        git_ok(repo.path(), &["commit", "-q", "--no-verify", "-m", "base"])
            .expect("git commit base");

        git_ok(repo.path(), &["checkout", "-B", "develop"]).expect("git checkout develop");
        std::fs::write(repo.path().join("shared.txt"), "develop\n").expect("write develop file");
        git_ok(repo.path(), &["add", "shared.txt"]).expect("git add develop shared");
        git_ok(
            repo.path(),
            &["commit", "-q", "--no-verify", "-m", "develop"],
        )
        .expect("failed to commit develop changes");
        git_ok(repo.path(), &["checkout", "master"]).expect("git checkout master");

        let feature_wt = worktrees.join("feature");
        git_ok(
            repo.path(),
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                feature_wt
                    .to_str()
                    .expect("feature worktree path not utf-8"),
            ],
        )
        .expect("failed to add feature worktree");

        std::fs::write(feature_wt.join("shared.txt"), "feature\n").expect("write feature file");
        git_ok(&feature_wt, &["add", "shared.txt"]).expect("git add feature shared");
        git_ok(
            &feature_wt,
            &["commit", "-q", "--no-verify", "-m", "feature"],
        )
        .expect("failed to commit feature changes");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);
        app.data.ui.preview_dimensions = Some((80, 24));

        let mut agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            feature_wt.clone(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        let session = agent.mux_session.clone();
        app.data.storage.add(agent);

        let manager = SessionManager::new();
        manager
            .create(&session, &feature_wt, Some(&sleep_command(30)))
            .expect("create mux session");

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "develop".to_string();

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_eq!(next, AppMode::normal());
        assert!(app.data.storage.iter().any(|agent| agent.is_terminal));

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_execute_merge_in_main_repo_failure_returns_error_modal() {
        let repo = init_repo();
        git_ok(repo.path(), &["branch", "develop"]).expect("create develop branch");

        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "feature-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            repo.path().to_path_buf(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "does-not-exist".to_string();
        app.data.git_op.target_branch = "develop".to_string();

        let next = Actions::execute_merge(&mut app.data).expect("execute_merge");
        assert_app_mode_variant(
            &next,
            &AppMode::ErrorModal(ErrorModalMode {
                message: String::new(),
            }),
        );
    }

    #[test]
    fn test_git_helpers_error_on_missing_repo() {
        let missing = Path::new("/path/that/does/not/exist");
        assert!(Actions::git_stash_push(missing).is_err());
        assert!(Actions::git_get_current_branch(missing).is_err());
        assert!(Actions::git_checkout(missing, "master").is_err());
    }

    #[test]
    fn test_find_worktree_for_branch_errors_when_workdir_missing() {
        let missing = Path::new("/path/that/does/not/exist");
        assert!(Actions::find_worktree_for_branch(missing, "master").is_err());
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_errors_when_session_missing() {
        let _guard = crate::test_support::lock_mux_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut owner = Agent::new(
            "owner".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        owner.repo_root = Some(repo.path().to_path_buf());
        app.data.storage.add(owner);

        let err = Actions::spawn_merge_conflict_terminal_in_worktree(
            &mut app.data,
            "feature",
            "master",
            repo.path(),
        )
        .unwrap_err();
        assert!(!err.to_string().trim().is_empty());
    }

    #[test]
    fn test_spawn_merge_conflict_terminal_errors_when_storage_save_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();

        let repo = init_repo();
        let state_path = repo.path().join("state-dir");
        std::fs::create_dir_all(&state_path).expect("create state dir");

        let mut app = create_test_app(repo.path(), state_path);
        app.data.ui.preview_dimensions = Some((80, 24));

        let mut owner = Agent::new(
            "owner".to_string(),
            "echo".to_string(),
            "master".to_string(),
            repo.path().to_path_buf(),
        );
        owner.repo_root = Some(repo.path().to_path_buf());
        let session = owner.mux_session.clone();
        app.data.storage.add(owner);

        let manager = SessionManager::new();
        manager
            .create(&session, repo.path(), Some(&sleep_command(30)))
            .expect("create mux session");

        let err = Actions::spawn_merge_conflict_terminal_in_worktree(
            &mut app.data,
            "feature",
            "master",
            repo.path(),
        )
        .unwrap_err();
        assert!(!err.to_string().trim().is_empty());

        let _ = manager.kill(&session);
    }
}
