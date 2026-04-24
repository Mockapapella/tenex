//! Rebase flow (branch selector + rebase execution).

use crate::git;
use anyhow::{Context, Result};
use tracing::{debug, info};

use crate::app::AppData;
use crate::state::{AppMode, ErrorModalMode, RebaseBranchSelectorMode, SuccessModalMode};

use super::super::Actions;

fn output_indicates_rebase_conflict(combined_output: &str) -> bool {
    combined_output.contains("CONFLICT") || combined_output.contains("could not apply")
}

impl Actions {
    /// Start the rebase flow - show branch selector (Ctrl+r)
    ///
    /// # Errors
    ///
    /// Returns an error if the git repository cannot be opened or branches cannot be listed.
    pub fn rebase_branch(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected. Select an agent first to rebase.".to_string(),
            }
            .into());
        };

        let agent_id = agent.id;
        let current_branch = agent.branch.clone();

        debug!(branch = %current_branch, "Starting rebase flow");

        // Fetch branches for selector from the selected agent's repository.
        let repo_path = agent
            .repo_root
            .clone()
            .or_else(|| git::repository_workspace_root(&agent.worktree_path).ok())
            .unwrap_or_else(|| agent.worktree_path.clone());
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.git_op.start_rebase(agent_id, current_branch);
        app_data.review.start(branches);
        Ok(RebaseBranchSelectorMode.into())
    }

    /// Execute the rebase operation
    ///
    /// # Errors
    ///
    /// Returns an error if the rebase operation fails
    pub fn execute_rebase(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for rebase".to_string(),
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

        let worktree_path = agent.worktree_path.clone();
        let current_branch = app_data.git_op.branch_name.clone();
        let target_branch = app_data.git_op.target_branch.clone();

        debug!(
            current = %current_branch,
            target = %target_branch,
            "Executing rebase"
        );

        // Execute git rebase
        let output = crate::git::git_command()
            .args(["rebase", &target_branch])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to execute rebase")?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{stdout}{stderr}");

            // Check if there are merge conflicts (git may output to stdout or stderr)
            if output_indicates_rebase_conflict(&combined) {
                info!(
                    current = %current_branch,
                    target = %target_branch,
                    "Rebase has conflicts - spawning terminal"
                );
                // Spawn terminal for conflict resolution
                return Self::spawn_conflict_terminal(app_data, "Rebase Conflict", "git status");
            }

            // Show error with both stdout and stderr for context
            let error_msg = super::merge::git_failure_message(stdout.as_ref(), stderr.as_ref());
            app_data.git_op.clear();
            app_data.review.clear();
            return Ok(ErrorModalMode {
                message: format!("Rebase failed: {error_msg}"),
            }
            .into());
        }

        info!(
            current = %current_branch,
            target = %target_branch,
            "Rebase successful"
        );
        app_data.git_op.clear();
        app_data.review.clear();
        Ok(SuccessModalMode {
            message: format!("Rebased {current_branch} onto {target_branch}"),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[test]
    fn test_output_indicates_rebase_conflict_recognizes_markers() {
        assert!(output_indicates_rebase_conflict(
            "error: could not apply abc123"
        ));
        assert!(output_indicates_rebase_conflict(
            "CONFLICT (content): Merge conflict"
        ));
        assert!(!output_indicates_rebase_conflict(
            "fatal: not a git repository"
        ));
    }

    fn create_test_app(repo_root: &Path, state_path: PathBuf) -> crate::App {
        let config = Config {
            worktree_dir: repo_root.join(".tenex-test-worktrees"),
            branch_prefix: "tenex-test-rebase/".to_string(),
            ..Config::default()
        };
        let storage = Storage::with_path(state_path);
        crate::App::new(config, storage, Settings::default(), false)
    }

    #[test]
    fn test_execute_rebase_propagates_rebase_spawn_errors() {
        let _guard_env = crate::test_support::lock_env_test_environment();

        let repo = TempDir::new().expect("temp repo");
        let state_path = repo.path().join("state.json");
        let mut app = create_test_app(repo.path(), state_path);

        let mut agent = Agent::new(
            "rebase-agent".to_string(),
            "echo".to_string(),
            "feature".to_string(),
            repo.path().to_path_buf(),
        );
        agent.repo_root = Some(repo.path().to_path_buf());
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data.git_op.agent_id = Some(agent_id);
        app.data.git_op.branch_name = "feature".to_string();
        app.data.git_op.target_branch = "master".to_string();

        let missing_git = repo.path().join("missing-git");
        let err = crate::git::with_git_program_override_for_tests(missing_git, || {
            Actions::execute_rebase(&mut app.data).unwrap_err()
        });
        assert!(err.to_string().contains("Failed to execute rebase"));
    }
}
