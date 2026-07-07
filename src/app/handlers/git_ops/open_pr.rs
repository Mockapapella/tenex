//! Open PR flow (base branch detection, unpushed check, gh integration).
#![cfg_attr(coverage_nightly, coverage(off))]

use anyhow::{Context, Result, bail};
use tracing::{debug, info, warn};

use crate::app::AppData;
use crate::state::{AppMode, ConfirmPushForPRMode, ErrorModalMode};

use super::super::Actions;

use std::cell::RefCell;

thread_local! {
    static GH_BINARY_OVERRIDE: RefCell<std::ffi::OsString> = RefCell::new(std::ffi::OsString::from("gh"));
}

#[cfg(test)]
pub(super) fn set_gh_binary_override(path: std::path::PathBuf) {
    GH_BINARY_OVERRIDE.with(|value| {
        let _ = value.replace(path.into_os_string());
    });
}

#[cfg(test)]
pub(super) fn with_gh_binary_override<T>(
    program: impl Into<std::ffi::OsString>,
    f: impl FnOnce() -> T,
) -> T {
    struct GhBinaryOverrideGuard {
        previous: std::ffi::OsString,
    }

    impl Drop for GhBinaryOverrideGuard {
        fn drop(&mut self) {
            GH_BINARY_OVERRIDE.with(|value| {
                let _ = value.replace(self.previous.clone());
            });
        }
    }

    GH_BINARY_OVERRIDE.with(|value| {
        let previous = value.replace(program.into());
        let _guard = GhBinaryOverrideGuard { previous };
        f()
    })
}

impl Actions {
    /// Open a PR for the selected agent's branch (Ctrl+o)
    ///
    /// Detects the base branch, checks for unpushed commits, and opens a PR.
    ///
    /// # Errors
    ///
    /// Returns an error if no agent is selected or PR creation fails.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn open_pr_flow(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            bail!("No agent selected");
        };

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        // Detect base branch from git history (best-effort)
        let base_branch = Self::detect_base_branch(&worktree_path, &branch_name);

        // Check if there are unpushed commits
        let has_unpushed = Self::has_unpushed_commits(&worktree_path, &branch_name)?;

        debug!(
            branch = %branch_name,
            base_branch = %base_branch,
            has_unpushed,
            "Starting open PR flow"
        );

        app_data
            .git_op
            .start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        // If no unpushed commits, open PR immediately
        if has_unpushed {
            return Ok(ConfirmPushForPRMode.into());
        }

        if let Err(err) = Self::open_pr_in_browser(app_data) {
            return Ok(ErrorModalMode {
                message: format!("Failed to open PR: {err:#}"),
            }
            .into());
        }

        Ok(AppMode::normal())
    }

    /// Detect the base branch that this branch was created from
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn detect_base_branch(worktree_path: &std::path::Path, branch_name: &str) -> String {
        // Prefer explicit "Created from <branch>" data in reflog when available.
        if let Ok(output) = crate::git::git_command()
            .args(["reflog", "show", "--no-abbrev", branch_name])
            .current_dir(worktree_path)
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(from_idx) = line.find("Created from ") {
                    let rest = &line[from_idx + 13..];
                    let base = rest.split_whitespace().next().unwrap_or("main");
                    return base.to_string();
                }
            }
        }

        // Fall back to the remote's default branch when available (e.g. origin/main).
        if let Ok(output) = crate::git::git_command()
            .args([
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ])
            .current_dir(worktree_path)
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(remote_ref) = stdout.lines().next()
                && let Some((_, base)) = remote_ref.trim().rsplit_once('/')
                && !base.is_empty()
            {
                return base.to_string();
            }
        }

        // Otherwise, use the first common default branch that exists locally or on origin.
        let candidates = ["main", "master", "develop"];
        for candidate in &candidates {
            let local_ref = format!("refs/heads/{candidate}");
            if crate::git::git_command()
                .args(["show-ref", "--verify", "--quiet", &local_ref])
                .current_dir(worktree_path)
                .status()
                .is_ok_and(|s| s.success())
            {
                return (*candidate).to_string();
            }

            let remote_ref = format!("refs/remotes/origin/{candidate}");
            if crate::git::git_command()
                .args(["show-ref", "--verify", "--quiet", &remote_ref])
                .current_dir(worktree_path)
                .status()
                .is_ok_and(|s| s.success())
            {
                return (*candidate).to_string();
            }
        }

        "main".to_string()
    }

    /// Check if there are unpushed commits on the branch
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn has_unpushed_commits(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<bool> {
        let remote_branch = if super::push::configured_upstream(worktree_path, branch_name)
            .context("Failed to check remote branch")?
            .is_some()
        {
            format!("{branch_name}@{{upstream}}")
        } else {
            format!("origin/{branch_name}")
        };
        let output = crate::git::git_command()
            .args(["rev-parse", "--verify", &remote_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check remote branch")?;

        if !output.status.success() {
            // No remote branch means all commits are unpushed
            return Ok(true);
        }

        // Compare local and remote
        let output = crate::git::git_command()
            .args([
                "rev-list",
                "--count",
                &format!("{remote_branch}..{branch_name}"),
            ])
            .current_dir(worktree_path)
            .output()
            .context("Failed to count unpushed commits")?;

        let count: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(count > 0)
    }

    /// Execute push and then open PR (for Ctrl+o flow)
    ///
    /// # Errors
    ///
    /// Returns an error if the push or PR open fails
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn execute_push_and_open_pr(app_data: &mut AppData) -> Result<AppMode> {
        let Some(agent_id) = app_data.git_op.agent_id else {
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: "No agent ID for push".to_string(),
            }
            .into());
        };

        let Some(agent) = app_data.storage.get(agent_id) else {
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: "Agent not found".to_string(),
            }
            .into());
        };

        let worktree_path = agent.worktree_path.clone();
        let branch_name = app_data.git_op.branch_name.clone();

        debug!(branch = %branch_name, "Executing push before opening PR");

        let push_output = super::push::run_push(&worktree_path, &branch_name)?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            app_data.git_op.clear();
            return Ok(ErrorModalMode {
                message: format!("Push failed: {}", stderr.trim()),
            }
            .into());
        }

        info!(branch = %branch_name, "Push successful, opening PR");

        // Now open the PR
        if let Err(err) = Self::open_pr_in_browser(app_data) {
            return Ok(ErrorModalMode {
                message: format!("Failed to open PR: {err:#}"),
            }
            .into());
        }

        Ok(AppMode::normal())
    }

    /// Open PR in browser using gh CLI
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn open_pr_in_browser(app_data: &mut AppData) -> Result<()> {
        let agent_id = app_data
            .git_op
            .agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for PR"))?;

        let agent = app_data
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch = app_data.git_op.branch_name.clone();
        let base_branch = app_data.git_op.base_branch.clone();

        debug!(
            branch = %branch,
            base_branch = %base_branch,
            "Opening PR with gh CLI"
        );

        // Use gh pr create with --web flag to open in browser
        let gh = GH_BINARY_OVERRIDE.with(|value| value.borrow().clone());

        let output = std::process::Command::new(&gh)
            .args(["pr", "create", "--web", "--base", &base_branch])
            .current_dir(&worktree_path)
            .output();

        match output {
            Ok(result) if result.status.success() => {
                info!(branch = %branch, base = %base_branch, "Opened PR creation page in browser");
                app_data.set_status(format!("Opening PR: {branch} → {base_branch}"));
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                warn!(error = %stderr, "gh pr create failed");
                app_data.git_op.clear();
                anyhow::bail!("{}", stderr.trim());
            }
            Err(e) => {
                warn!(error = %e, "gh CLI not found");
                app_data.git_op.clear();
                anyhow::bail!("gh CLI not found. Install it with: brew install gh");
            }
        }

        app_data.git_op.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    #[test]
    fn test_open_pr_flow_propagates_has_unpushed_errors() {
        let (mut app, _temp) = create_test_app();
        let missing_worktree_path = std::env::temp_dir().join(format!(
            "tenex-open-pr-missing-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        assert!(!missing_worktree_path.exists());

        let agent = Agent::new(
            "agent".to_string(),
            "codex".to_string(),
            "feature".to_string(),
            missing_worktree_path,
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.select_agent_by_id(agent_id);

        let err = Actions::open_pr_flow(&mut app.data).expect_err("expected open_pr_flow to fail");
        assert!(err.to_string().contains("Failed to check remote branch"));
    }

    #[cfg(unix)]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "Matches the hook signature used by write_fake_script_with_hooks."
    )]
    fn ok_hook(_: &Path) -> Result<()> {
        Ok(())
    }

    #[cfg(unix)]
    fn write_fake_script(dir: &Path, name: &str, body: &str) -> Result<PathBuf> {
        write_fake_script_with_hooks(dir, name, body, ok_hook, ok_hook)
    }

    #[cfg(unix)]
    fn write_fake_script_with_hooks<F, G>(
        dir: &Path,
        name: &str,
        body: &str,
        after_write: F,
        after_metadata: G,
    ) -> Result<PathBuf>
    where
        F: FnOnce(&Path) -> Result<()>,
        G: FnOnce(&Path) -> Result<()>,
    {
        let script = dir.join(name);
        fs::write(&script, body)?;
        after_write(&script)?;

        let mut perms = fs::metadata(&script)?.permissions();
        after_metadata(&script)?;

        perms.set_mode(0o755);
        fs::set_permissions(&script, perms)?;
        Ok(script)
    }

    #[cfg(unix)]
    #[test]
    fn test_has_unpushed_commits_reports_error_when_rev_list_spawn_fails() -> Result<()> {
        let worktree = TempDir::new()?;
        let temp = TempDir::new()?;
        let script = write_fake_script(
            temp.path(),
            "git",
            r#"#!/bin/sh
if [ "$1" = "config" ]; then
  exit 1
fi
if [ "$1" = "rev-parse" ]; then
  rm -- "$0"
  exit 0
fi
exit 0
"#,
        )?;

        let err = crate::git::with_git_program_override_for_tests(script, || {
            Actions::has_unpushed_commits(worktree.path(), "feature")
        })
        .err()
        .ok_or_else(|| anyhow::anyhow!("expected rev-list spawn to fail"))?;

        assert!(err.to_string().contains("Failed to count unpushed commits"));
        Ok(())
    }

    #[test]
    fn test_execute_push_and_open_pr_reports_error_when_push_spawn_fails() {
        let (mut app, _temp) = create_test_app();
        let worktree = TempDir::new().unwrap();

        let agent = Agent::new(
            "agent".to_string(),
            "codex".to_string(),
            "feature".to_string(),
            worktree.path().to_path_buf(),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data
            .git_op
            .start_open_pr(agent_id, "feature".to_string(), "main".to_string(), true);

        let missing_program = worktree.path().join("git-missing");
        let err = crate::git::with_git_program_override_for_tests(missing_program, || {
            Actions::execute_push_and_open_pr(&mut app.data)
        })
        .expect_err("expected push spawn to fail");

        assert!(err.to_string().contains("Failed to push to remote"));
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_base_branch_falls_back_when_remote_head_has_no_slash() {
        let worktree = TempDir::new().unwrap();
        let temp = TempDir::new().unwrap();
        let script = write_fake_script(
            temp.path(),
            "git",
            r#"#!/bin/sh
if [ "$1" = "reflog" ]; then
  exit 1
fi
if [ "$1" = "symbolic-ref" ]; then
  printf 'main\n'
  exit 0
fi
exit 1
"#,
        )
        .unwrap();

        let base = crate::git::with_git_program_override_for_tests(script, || {
            Actions::detect_base_branch(worktree.path(), "feature")
        });

        assert_eq!(base, "main");
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_base_branch_falls_back_when_remote_head_base_is_empty() {
        let worktree = TempDir::new().unwrap();
        let temp = TempDir::new().unwrap();
        let script = write_fake_script(
            temp.path(),
            "git",
            r#"#!/bin/sh
if [ "$1" = "reflog" ]; then
  exit 1
fi
if [ "$1" = "symbolic-ref" ]; then
  printf 'origin/\n'
  exit 0
fi
exit 1
"#,
        )
        .unwrap();

        let base = crate::git::with_git_program_override_for_tests(script, || {
            Actions::detect_base_branch(worktree.path(), "feature")
        });

        assert_eq!(base, "main");
    }

    #[cfg(unix)]
    #[test]
    fn test_open_pr_in_browser_success_sets_status_and_clears_state() {
        let (mut app, _temp) = create_test_app();
        let worktree = TempDir::new().unwrap();
        let temp = TempDir::new().unwrap();
        let script = write_fake_script(temp.path(), "gh", "#!/bin/sh\nexit 0\n").unwrap();

        let agent = Agent::new(
            "agent".to_string(),
            "codex".to_string(),
            "feature".to_string(),
            worktree.path().to_path_buf(),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data
            .git_op
            .start_open_pr(agent_id, "feature".to_string(), "main".to_string(), false);

        with_gh_binary_override(script, || Actions::open_pr_in_browser(&mut app.data)).unwrap();

        let message = app.data.ui.status_message.unwrap_or_default();
        assert!(message.contains("Opening PR:"));
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.base_branch.is_empty());
    }

    #[test]
    fn test_open_pr_in_browser_reports_error_when_gh_missing_and_clears_state() {
        let (mut app, _temp) = create_test_app();
        let worktree = TempDir::new().unwrap();

        let agent = Agent::new(
            "agent".to_string(),
            "codex".to_string(),
            "feature".to_string(),
            worktree.path().to_path_buf(),
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        app.data
            .git_op
            .start_open_pr(agent_id, "feature".to_string(), "main".to_string(), false);

        let missing = worktree.path().join("gh-missing");
        let err = with_gh_binary_override(missing, || Actions::open_pr_in_browser(&mut app.data))
            .expect_err("expected missing gh binary to fail");

        assert!(
            err.to_string()
                .contains("gh CLI not found. Install it with: brew install gh")
        );
        assert!(app.data.git_op.agent_id.is_none());
        assert!(app.data.git_op.branch_name.is_empty());
        assert!(app.data.git_op.base_branch.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_fake_script_succeeds_and_sets_permissions() {
        let temp = TempDir::new().unwrap();
        let script = write_fake_script(temp.path(), "git", "#!/bin/sh\nexit 0\n").unwrap();

        let meta = fs::metadata(&script).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_fake_script_reports_write_errors() {
        let temp = TempDir::new().unwrap();
        let missing_dir = temp.path().join("missing");

        let err = write_fake_script(&missing_dir, "git", "#!/bin/sh\nexit 0\n")
            .expect_err("expected write to fail");
        let io_err = err
            .downcast_ref::<std::io::Error>()
            .expect("expected io error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_fake_script_reports_metadata_errors() {
        let temp = TempDir::new().unwrap();

        let err = write_fake_script_with_hooks(
            temp.path(),
            "git",
            "#!/bin/sh\nexit 0\n",
            |script| {
                fs::remove_file(script).unwrap();
                Ok(())
            },
            ok_hook,
        )
        .expect_err("expected metadata to fail");
        let io_err = err
            .downcast_ref::<std::io::Error>()
            .expect("expected io error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_fake_script_reports_set_permissions_errors() {
        let temp = TempDir::new().unwrap();

        let err = write_fake_script_with_hooks(
            temp.path(),
            "git",
            "#!/bin/sh\nexit 0\n",
            |_| Ok(()),
            |script| {
                fs::remove_file(script).unwrap();
                Ok(())
            },
        )
        .expect_err("expected set_permissions to fail");
        let io_err = err
            .downcast_ref::<std::io::Error>()
            .expect("expected io error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_fake_script_reports_after_write_hook_errors() {
        let temp = TempDir::new().unwrap();

        let err = write_fake_script_with_hooks(
            temp.path(),
            "git",
            "#!/bin/sh\nexit 0\n",
            |_| anyhow::bail!("after_write failed"),
            ok_hook,
        )
        .expect_err("expected after_write hook to fail");
        assert!(err.to_string().contains("after_write failed"));
    }

    #[cfg(unix)]
    #[test]
    fn test_write_fake_script_reports_after_metadata_hook_errors() {
        let temp = TempDir::new().unwrap();

        let err = write_fake_script_with_hooks(
            temp.path(),
            "git",
            "#!/bin/sh\nexit 0\n",
            ok_hook,
            |_| anyhow::bail!("after_metadata failed"),
        )
        .expect_err("expected after_metadata hook to fail");
        assert!(err.to_string().contains("after_metadata failed"));
    }
}
