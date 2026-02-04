//! Git operations module

mod branch;
mod diff;
mod worktree;
pub use branch::{BranchInfo, Manager as BranchManager};
pub use diff::{
    DiffDigest, DiffFile, DiffHunk, DiffHunkLine, DiffModel, FileChange, FileStatus,
    Generator as DiffGenerator, LineChange, Summary as DiffSummary,
};
pub use worktree::{Info as WorktreeInfo, Manager as WorktreeManager};

use anyhow::{Context, Result};
use std::borrow::Cow;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub use git2::Repository;

/// Create a `git` command for Tenex.
///
/// Git hooks can set variables like `GIT_DIR` which override repository discovery and ignore
/// `current_dir`. Clearing these for child processes ensures Tenex operates on the intended
/// worktree repositories.
#[must_use]
pub(crate) fn git_command() -> Command {
    let mut cmd = Command::new("git");
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
        return Ok(common_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf());
    }

    repo.workdir()
        .map(std::path::Path::to_path_buf)
        .or_else(|| common_dir.parent().map(Path::to_path_buf))
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

    // Check if .tenex/ is already in exclude
    if exclude_path.exists() {
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

    // Append .tenex/ to exclude file
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)
        .with_context(|| format!("Failed to open {} for writing", exclude_path.display()))?;

    writeln!(file, "{EXCLUDE_ENTRY}")
        .with_context(|| format!("Failed to write to {}", exclude_path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_test_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let output = git_command()
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()?;
        if !output.status.success() {
            return Err("Failed to initialize test repo".into());
        }
        Ok(temp_dir)
    }

    fn init_test_repo_with_commit() -> Result<TempDir, Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;

        let output = git_command()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()?;
        if !output.status.success() {
            return Err("Failed to configure test git user email".into());
        }

        let output = git_command()
            .args(["config", "user.name", "Tenex Test"])
            .current_dir(temp_dir.path())
            .output()?;
        if !output.status.success() {
            return Err("Failed to configure test git user name".into());
        }

        let output = git_command()
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(temp_dir.path())
            .output()?;
        if !output.status.success() {
            return Err("Failed to create initial test commit".into());
        }

        Ok(temp_dir)
    }

    #[test]
    fn test_is_git_repository() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;
        assert!(is_git_repository(temp_dir.path()));

        let non_repo = TempDir::new()?;
        assert!(!is_git_repository(non_repo.path()));
        Ok(())
    }

    #[test]
    fn test_open_repository() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;
        assert!(open_repository(temp_dir.path()).is_ok());

        let non_repo = TempDir::new()?;
        assert!(open_repository(non_repo.path()).is_err());
        Ok(())
    }

    #[test]
    fn test_repository_root() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;
        let root = repository_root(temp_dir.path())?;
        // Canonicalize to handle symlinked temp dirs.
        let expected = temp_dir.path().canonicalize()?;
        let actual = root.canonicalize()?;
        assert_eq!(actual, expected);
        Ok(())
    }

    #[test]
    fn test_repository_workspace_root_regular_repo() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;
        let root = repository_workspace_root(temp_dir.path())?;
        assert_eq!(root.canonicalize()?, temp_dir.path().canonicalize()?);
        Ok(())
    }

    #[test]
    fn test_resolve_repo_common_dir_empty_commondir_file_returns_git_dir()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new()?;
        let git_dir = dir.path();
        std::fs::write(git_dir.join("commondir"), "  \n")?;

        let common_dir = resolve_repo_common_dir(git_dir)?;
        assert_eq!(common_dir.as_ref(), git_dir);
        Ok(())
    }

    #[test]
    fn test_resolve_repo_common_dir_absolute_path() -> Result<(), Box<dyn std::error::Error>> {
        let git_dir = TempDir::new()?;
        let common_dir = TempDir::new()?;

        std::fs::write(
            git_dir.path().join("commondir"),
            common_dir.path().to_string_lossy().as_ref(),
        )?;

        assert_eq!(
            resolve_repo_common_dir(git_dir.path())?
                .as_ref()
                .canonicalize()?,
            common_dir.path().canonicalize()?
        );
        Ok(())
    }

    #[test]
    fn test_repository_workspace_root_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo_with_commit()?;

        let worktree_path = temp_dir.path().join("worktree");
        let output = git_command()
            .args([
                "worktree",
                "add",
                "-b",
                "feature/test-worktree",
                worktree_path.to_str().ok_or("invalid worktree path")?,
            ])
            .current_dir(temp_dir.path())
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "Failed to create test worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }

        let root = repository_workspace_root(&worktree_path)?;
        assert_eq!(root.canonicalize()?, temp_dir.path().canonicalize()?);
        Ok(())
    }

    #[test]
    fn test_repository_workspace_root_errors_for_non_repo() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = TempDir::new()?;
        assert!(repository_workspace_root(dir.path()).is_err());
        Ok(())
    }

    #[test]
    fn test_ensure_tenex_excluded() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;

        // First call should add .tenex/ to exclude
        ensure_tenex_excluded(temp_dir.path())?;

        let exclude_path = exclude_path_for_test(temp_dir.path())?;
        assert!(exclude_path.exists());

        let contents = std::fs::read_to_string(&exclude_path)?;
        assert!(contents.contains(".tenex/"));

        // Second call should be idempotent
        ensure_tenex_excluded(temp_dir.path())?;

        let contents = std::fs::read_to_string(&exclude_path)?;
        let count = contents.matches(".tenex/").count();
        assert_eq!(count, 1, "Should only have one .tenex/ entry");

        Ok(())
    }

    #[test]
    fn test_ensure_tenex_excluded_creates_info_dir() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = init_test_repo()?;

        // Remove the info directory if it exists
        let info_dir = temp_dir.path().join(".git/info");
        if info_dir.exists() {
            std::fs::remove_dir_all(&info_dir)?;
        }

        // Should create the directory and file
        ensure_tenex_excluded(temp_dir.path())?;

        assert!(info_dir.exists());
        let exclude_path = info_dir.join("exclude");
        assert!(exclude_path.exists());

        Ok(())
    }

    fn exclude_path_for_test(repo_path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let output = git_command()
            .args(["rev-parse", "--git-path", "info/exclude"])
            .current_dir(repo_path)
            .output()?;
        if !output.status.success() {
            return Err("Failed to resolve .git info/exclude".into());
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let path = PathBuf::from(raw);
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(repo_path.join(path))
        }
    }
}
