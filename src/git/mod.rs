//! Git operations module

mod branch;
mod diff;
mod worktree;
#[cfg(test)]
pub(crate) use branch::with_list_for_selector_override_for_tests;
pub use branch::{BranchInfo, Manager as BranchManager};
pub use diff::{
    DiffDigest, DiffFile, DiffHunk, DiffHunkLine, DiffModel, FileChange, FileStatus,
    Generator as DiffGenerator, LineChange, Summary as DiffSummary,
};
pub use worktree::{
    CreateOptions as WorktreeCreateOptions, Info as WorktreeInfo, Manager as WorktreeManager,
};

use anyhow::{Context, Result};
use std::borrow::Cow;
#[cfg(test)]
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub use git2::Repository;

#[cfg(any(test, coverage))]
thread_local! {
    static FORCE_EXCLUDE_WRITE_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
pub(crate) fn with_forced_repo_worktrees_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    worktree::with_forced_repo_worktrees_error_for_tests(f)
}

#[cfg(test)]
thread_local! {
    static GIT_PROGRAM_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[cfg(test)]
struct GitProgramOverrideGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for GitProgramOverrideGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        GIT_PROGRAM_OVERRIDE.with(|cell| {
            *cell.borrow_mut() = previous;
        });
    }
}

#[cfg(test)]
pub(crate) fn with_git_program_override_for_tests<T>(program: PathBuf, f: impl FnOnce() -> T) -> T {
    let previous = GIT_PROGRAM_OVERRIDE.with(|cell| (*cell.borrow_mut()).replace(program));
    let _guard = GitProgramOverrideGuard { previous };
    f()
}

/// Create a `git` command for Tenex.
///
/// Git hooks can set variables like `GIT_DIR` which override repository discovery and ignore
/// `current_dir`. Clearing these for child processes ensures Tenex operates on the intended
/// worktree repositories.
#[must_use]
pub(crate) fn git_command() -> Command {
    #[cfg(test)]
    let program = GIT_PROGRAM_OVERRIDE
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| PathBuf::from("git"));

    #[cfg(not(test))]
    let program = "git";

    let mut cmd = Command::new(program);
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

/// Open a git repository at the given path
///
/// # Errors
///
/// Returns an error if the path is not a git repository
pub fn open_repository(path: &Path) -> Result<Repository> {
    Repository::discover(path)
        .with_context(|| format!("Failed to open git repository at {}", path.display()))
}

/// Check if a path is inside a git repository
#[must_use]
pub fn is_git_repository(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

/// Get the root of the git repository containing the given path
///
/// # Errors
///
/// Returns an error if the path is not inside a git repository
pub fn repository_root(path: &Path) -> Result<PathBuf> {
    let repo = open_repository(path)?;
    repo.workdir()
        .map(std::path::Path::to_path_buf)
        .context("Repository has no working directory")
}

fn resolve_repo_common_dir(git_dir: &Path) -> Result<Cow<'_, Path>> {
    let commondir_path = git_dir.join("commondir");
    if !commondir_path.exists() {
        return Ok(Cow::Borrowed(git_dir));
    }

    let raw = fs::read_to_string(&commondir_path)
        .with_context(|| format!("Failed to read {}", commondir_path.display()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Cow::Borrowed(git_dir));
    }

    let path = PathBuf::from(trimmed);
    let resolved = if path.is_absolute() {
        path
    } else {
        git_dir.join(path)
    };

    let canonical = resolved.canonicalize().unwrap_or(resolved);
    Ok(Cow::Owned(canonical))
}

/// Get the workspace root of the repository that owns the given path.
///
/// For a normal repository this returns the repository root. For git worktrees
/// this returns the main repository root (not the worktree directory).
///
/// # Errors
///
/// Returns an error if the path is not inside a git repository.
pub fn repository_workspace_root(path: &Path) -> Result<PathBuf> {
    let repo = open_repository(path)?;
    let git_dir = repo.path();
    let common_dir = resolve_repo_common_dir(git_dir)?;

    if common_dir.file_name().is_some_and(|name| name == ".git") {
        let default_parent = Path::new(".");
        return Ok(common_dir.parent().unwrap_or(default_parent).to_path_buf());
    }

    repo.workdir()
        .map(std::path::Path::to_path_buf)
        .or_else(|| Some(common_dir.as_ref().to_path_buf()))
        .context("Repository has no working directory")
}

/// Ensure `.tenex/` is in `.git/info/exclude`
///
/// This prevents synthesis files from being tracked by git.
/// Creates the exclude file if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the exclude file cannot be read or written
pub fn ensure_tenex_excluded(repo_path: &Path) -> Result<()> {
    const EXCLUDE_ENTRY: &str = ".tenex/";

    let repo = open_repository(repo_path)?;
    let git_dir = repo.path();
    let info_dir = git_dir.join("info");
    let exclude_path = info_dir.join("exclude");

    // Create info directory if it doesn't exist
    if !info_dir.exists() {
        fs::create_dir_all(&info_dir)
            .with_context(|| format!("Failed to create {}", info_dir.display()))?;
    }

    // Only line-scan regular files here.
    //
    // We hit an OOM bug while running coverage because one test intentionally points
    // `.git/info/exclude` at `/dev/full` to force a write failure. That device does fail writes
    // with `ENOSPC`, but reads keep returning zero bytes without a newline or EOF. Passing that
    // path into `BufRead::lines()` causes the reader to keep extending one line buffer until the
    // process is killed for memory pressure.
    //
    // The fix is to validate the path shape at the filesystem boundary and keep the happy path
    // clean. Regular files still get the idempotent read pass so we do not append duplicate
    // `.tenex/` entries. Non-regular files skip the read pass and fall through to the append open,
    // where the OS returns a deterministic error instead of letting a special file consume
    // unbounded memory.
    if exclude_path.exists() {
        let should_scan_existing = fs::metadata(&exclude_path)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false);

        if should_scan_existing {
            let file = fs::File::open(&exclude_path)
                .with_context(|| format!("Failed to open {}", exclude_path.display()))?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                let line = line.context("Failed to read exclude file")?;
                if line.trim() == EXCLUDE_ENTRY {
                    // Already excluded
                    return Ok(());
                }
            }
        }
    }

    // Append .tenex/ to exclude file
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)
        .with_context(|| format!("Failed to open {} for writing", exclude_path.display()))?;

    #[cfg(any(test, coverage))]
    if FORCE_EXCLUDE_WRITE_ERROR.with(std::cell::Cell::get) {
        return Err(std::io::Error::other("forced exclude write failure"))
            .with_context(|| format!("Failed to write to {}", exclude_path.display()));
    }

    writeln!(file, "{EXCLUDE_ENTRY}")
        .with_context(|| format!("Failed to write to {}", exclude_path.display()))?;

    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn with_forced_exclude_write_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
        FORCE_EXCLUDE_WRITE_ERROR.with(|slot| {
            let previous = slot.replace(true);
            let result = f();
            slot.set(previous);
            result
        })
    }

    fn init_test_repo() -> TempDir {
        let temp_dir = TempDir::new().expect("create temp dir");
        let output = git_command()
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .expect("run git init");
        assert!(output.status.success());
        temp_dir
    }

    fn init_test_repo_with_commit() -> TempDir {
        let temp_dir = init_test_repo();

        let output = git_command()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()
            .expect("configure user.email");
        assert!(output.status.success());

        let output = git_command()
            .args(["config", "user.name", "Tenex Test"])
            .current_dir(temp_dir.path())
            .output()
            .expect("configure user.name");
        assert!(output.status.success());

        let output = git_command()
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(temp_dir.path())
            .output()
            .expect("create init commit");
        assert!(output.status.success());

        temp_dir
    }

    #[test]
    fn git_command_uses_default_program_when_unset() {
        let cmd = git_command();
        assert_eq!(cmd.get_program(), "git");
    }

    #[test]
    fn git_command_uses_test_override_and_resets_afterwards() {
        let fake_git = PathBuf::from("/tmp/tenex-fake-git");

        with_git_program_override_for_tests(fake_git.clone(), || {
            let cmd = git_command();
            assert_eq!(cmd.get_program(), fake_git.as_os_str());
        });

        let cmd = git_command();
        assert_eq!(cmd.get_program(), "git");
    }

    #[test]
    fn test_is_git_repository() {
        let temp_dir = init_test_repo();
        assert!(is_git_repository(temp_dir.path()));

        let non_repo = TempDir::new().expect("create temp dir");
        assert!(!is_git_repository(non_repo.path()));
    }

    #[test]
    fn test_open_repository() {
        let temp_dir = init_test_repo();
        assert!(open_repository(temp_dir.path()).is_ok());

        let non_repo = TempDir::new().expect("create temp dir");
        assert!(open_repository(non_repo.path()).is_err());
    }

    #[test]
    fn test_repository_root() {
        let temp_dir = init_test_repo();
        let root = repository_root(temp_dir.path()).expect("resolve repository root");

        let expected = temp_dir
            .path()
            .canonicalize()
            .expect("canonicalize expected path");
        let actual = root.canonicalize().expect("canonicalize actual path");
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_repository_root_errors_for_non_repo() {
        let non_repo = TempDir::new().expect("create temp dir");
        let err = repository_root(non_repo.path()).expect_err("repository_root should fail");
        assert!(err.to_string().contains("Failed to open git repository at"));
    }

    #[test]
    fn test_repository_workspace_root_regular_repo() {
        let temp_dir = init_test_repo();
        let root = repository_workspace_root(temp_dir.path()).expect("workspace root");
        assert_eq!(
            root.canonicalize().expect("canonicalize root"),
            temp_dir
                .path()
                .canonicalize()
                .expect("canonicalize temp dir"),
        );
    }

    #[test]
    fn test_repository_workspace_root_bare_repo() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let output = git_command()
            .args(["init", "--bare"])
            .current_dir(temp_dir.path())
            .output()
            .expect("run git init --bare");
        assert!(output.status.success());

        let root = repository_workspace_root(temp_dir.path()).expect("workspace root");
        assert_eq!(
            root.canonicalize().expect("canonicalize root"),
            temp_dir
                .path()
                .canonicalize()
                .expect("canonicalize temp dir"),
        );
    }

    #[test]
    fn test_repository_workspace_root_relative_dot_exercises_parent_fallback() {
        let temp_dir = init_test_repo();
        let original_dir = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(temp_dir.path()).expect("set current dir to repo");
        let result = repository_workspace_root(Path::new(".")).expect("workspace root");
        std::env::set_current_dir(&original_dir).expect("restore current dir");

        assert_eq!(
            result.canonicalize().expect("canonicalize root"),
            temp_dir
                .path()
                .canonicalize()
                .expect("canonicalize temp dir"),
        );
    }

    #[test]
    fn test_repository_workspace_root_reports_common_dir_read_failures() {
        let temp_dir = init_test_repo();
        let git_dir = temp_dir.path().join(".git");
        std::fs::create_dir_all(git_dir.join("commondir")).expect("create commondir directory");

        let err =
            repository_workspace_root(temp_dir.path()).expect_err("expected commondir read error");
        assert!(err.to_string().contains("Failed to read"));
    }

    #[test]
    fn test_resolve_repo_common_dir_empty_commondir_file_returns_git_dir() {
        let dir = TempDir::new().expect("create temp dir");
        let git_dir = dir.path();
        std::fs::write(git_dir.join("commondir"), "  \n").expect("write commondir");

        let common_dir = resolve_repo_common_dir(git_dir).expect("resolve common dir");
        assert_eq!(common_dir.as_ref(), git_dir);
    }

    #[test]
    fn test_resolve_repo_common_dir_absolute_path() {
        let git_dir = TempDir::new().expect("create temp dir");
        let common_dir = TempDir::new().expect("create temp dir");

        std::fs::write(
            git_dir.path().join("commondir"),
            common_dir.path().to_string_lossy().as_ref(),
        )
        .expect("write commondir");

        let resolved = resolve_repo_common_dir(git_dir.path()).expect("resolve common dir");
        assert_eq!(
            resolved
                .as_ref()
                .canonicalize()
                .expect("canonicalize resolved"),
            common_dir
                .path()
                .canonicalize()
                .expect("canonicalize expected"),
        );
    }

    #[test]
    fn test_resolve_repo_common_dir_reports_read_failure_when_commondir_is_directory() {
        let dir = TempDir::new().expect("create temp dir");
        let commondir_path = dir.path().join("commondir");
        std::fs::create_dir_all(&commondir_path).expect("create commondir directory");

        let err = resolve_repo_common_dir(dir.path()).expect_err("expected read failure");
        assert!(err.to_string().contains("Failed to read"));
    }

    #[test]
    fn test_repository_workspace_root_worktree() {
        let temp_dir = init_test_repo_with_commit();

        let worktree_path = temp_dir.path().join("worktree");
        let output = git_command()
            .args([
                "worktree",
                "add",
                "-b",
                "feature/test-worktree",
                worktree_path
                    .to_str()
                    .expect("worktree path should be valid utf8"),
            ])
            .current_dir(temp_dir.path())
            .output()
            .expect("add git worktree");
        assert!(output.status.success());

        let root = repository_workspace_root(&worktree_path).expect("workspace root");
        assert_eq!(
            root.canonicalize().expect("canonicalize root"),
            temp_dir
                .path()
                .canonicalize()
                .expect("canonicalize temp dir"),
        );
    }

    #[test]
    fn test_repository_workspace_root_errors_for_non_repo() {
        let dir = TempDir::new().expect("create temp dir");
        assert!(repository_workspace_root(dir.path()).is_err());
    }

    #[test]
    fn test_ensure_tenex_excluded() {
        let temp_dir = init_test_repo();

        ensure_tenex_excluded(temp_dir.path()).expect("ensure exclude");

        let exclude_path = exclude_path_for_test(temp_dir.path()).expect("resolve exclude path");
        assert!(exclude_path.exists());

        let contents = std::fs::read_to_string(&exclude_path).expect("read exclude file");
        assert!(contents.contains(".tenex/"));

        ensure_tenex_excluded(temp_dir.path()).expect("ensure exclude again");

        let contents = std::fs::read_to_string(&exclude_path).expect("read exclude file");
        let count = contents.matches(".tenex/").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_ensure_tenex_excluded_creates_info_dir() {
        let temp_dir = init_test_repo();

        let info_dir = temp_dir.path().join(".git/info");
        let _ = std::fs::remove_dir_all(&info_dir);

        ensure_tenex_excluded(temp_dir.path()).expect("ensure exclude");

        assert!(info_dir.exists());
        let exclude_path = info_dir.join("exclude");
        assert!(exclude_path.exists());
    }

    #[test]
    fn test_ensure_tenex_excluded_errors_for_non_repo() {
        let dir = TempDir::new().expect("create temp dir");
        assert!(ensure_tenex_excluded(dir.path()).is_err());
    }

    #[cfg(unix)]
    fn chmod(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(path).expect("stat path").permissions();
        perms.set_mode(mode);
        std::fs::set_permissions(path, perms).expect("set permissions");
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_tenex_excluded_reports_open_error_when_exclude_unreadable() {
        let temp_dir = init_test_repo();
        let exclude_path = exclude_path_for_test(temp_dir.path()).expect("resolve exclude path");
        std::fs::write(&exclude_path, "").expect("ensure exclude exists");
        chmod(&exclude_path, 0o000);

        let err = ensure_tenex_excluded(temp_dir.path()).expect_err("expected open failure");
        assert!(err.to_string().contains("Failed to open"));

        chmod(&exclude_path, 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_tenex_excluded_reports_open_for_write_error_when_exclude_is_directory() {
        let temp_dir = init_test_repo();
        let exclude_path = exclude_path_for_test(temp_dir.path()).expect("resolve exclude path");
        let _ = std::fs::remove_file(&exclude_path);
        std::fs::create_dir_all(&exclude_path).expect("create exclude directory");

        let err = ensure_tenex_excluded(temp_dir.path()).expect_err("expected open failure");
        assert!(err.to_string().contains("for writing"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_tenex_excluded_reports_open_for_write_error_when_exclude_is_read_only() {
        let temp_dir = init_test_repo();
        let exclude_path = exclude_path_for_test(temp_dir.path()).expect("resolve exclude path");
        std::fs::write(&exclude_path, "").expect("write exclude");
        chmod(&exclude_path, 0o444);

        let err = ensure_tenex_excluded(temp_dir.path()).expect_err("expected open failure");
        assert!(err.to_string().contains("for writing"));

        chmod(&exclude_path, 0o644);
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_tenex_excluded_reports_write_error_when_forced() {
        let temp_dir = init_test_repo();
        let err =
            with_forced_exclude_write_error_for_tests(|| ensure_tenex_excluded(temp_dir.path()))
                .expect_err("expected write failure");
        assert!(err.to_string().contains("Failed to write to"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_ensure_tenex_excluded_reports_write_error_when_exclude_points_to_dev_full() {
        use std::os::unix::fs::symlink;

        let temp_dir = init_test_repo();
        let exclude_path = exclude_path_for_test(temp_dir.path()).expect("resolve exclude path");
        std::fs::create_dir_all(exclude_path.parent().expect("exclude parent"))
            .expect("create info dir");
        let _ = std::fs::remove_file(&exclude_path);

        symlink("/dev/full", &exclude_path).expect("create exclude symlink");

        let err = ensure_tenex_excluded(temp_dir.path()).expect_err("expected write failure");
        assert!(err.to_string().contains("Failed to write to"));
    }

    #[test]
    fn test_ensure_tenex_excluded_reports_read_error_for_invalid_utf8() {
        let temp_dir = init_test_repo();
        let exclude_path = exclude_path_for_test(temp_dir.path()).expect("resolve exclude path");
        std::fs::create_dir_all(exclude_path.parent().expect("exclude parent"))
            .expect("create info dir");
        std::fs::write(&exclude_path, [0xff, 0xfe]).expect("write invalid utf8 exclude");

        let err = ensure_tenex_excluded(temp_dir.path()).expect_err("expected read failure");
        assert!(err.to_string().contains("Failed to read exclude file"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_tenex_excluded_reports_create_info_dir_error() {
        let temp_dir = init_test_repo();
        let git_dir = temp_dir.path().join(".git");
        let info_dir = git_dir.join("info");
        let _ = std::fs::remove_dir_all(&info_dir);

        chmod(&git_dir, 0o555);

        let err = ensure_tenex_excluded(temp_dir.path()).expect_err("expected create failure");
        assert!(err.to_string().contains("Failed to create"));

        chmod(&git_dir, 0o755);
    }

    #[test]
    fn test_exclude_path_for_test_returns_error_when_git_rev_parse_fails() {
        let dir = TempDir::new().expect("create temp dir");
        let err = exclude_path_for_test(dir.path()).expect_err("expected git rev-parse to fail");
        assert!(!err.to_string().is_empty());
    }

    #[cfg(unix)]
    fn write_fake_git_script(temp: &TempDir, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("git");
        std::fs::write(&script, body).expect("write fake git script");
        let mut perms = std::fs::metadata(&script)
            .expect("stat fake git script")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).expect("chmod fake git script");
        script
    }

    #[cfg(unix)]
    #[test]
    fn test_exclude_path_for_test_returns_absolute_path_when_git_outputs_absolute() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let abs = temp_dir.path().join("abs").join("exclude");
        let abs_str = abs.to_string_lossy();
        let fake_git = write_fake_git_script(&temp_dir, &format!("#!/bin/sh\necho '{abs_str}'\n"));

        let resolved = with_git_program_override_for_tests(fake_git, || {
            exclude_path_for_test(temp_dir.path())
        })
        .expect("resolve exclude path");

        assert!(resolved.is_absolute());
        assert_eq!(resolved, abs);
    }

    fn exclude_path_for_test(repo_path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let output = git_command()
            .args(["rev-parse", "--git-path", "info/exclude"])
            .current_dir(repo_path)
            .output()
            .expect("run git rev-parse");
        if !output.status.success() {
            return Err("Failed to resolve .git info/exclude".into());
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let path = PathBuf::from(raw);
        Ok(if path.is_absolute() {
            path
        } else {
            repo_path.join(path)
        })
    }
}
