//! Git operations module

mod branch;
mod diff;
mod worktree;

pub use branch::{BranchInfo, Manager as BranchManager};
pub use diff::{
    DiffDigest, DiffFile, DiffHunk, DiffHunkLine, DiffModel, FileChange, FileStatus,
    Generator as DiffGenerator, LineChange, Summary as DiffSummary,
};
pub use worktree::{
    CreateOptions as WorktreeCreateOptions, Info as WorktreeInfo, Manager as WorktreeManager,
    TargetPreparation as WorktreeTargetPreparation,
};

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

    // Only line-scan regular files. Special files can return data without EOF and grow the line
    // buffer without a bound. Non-regular paths fall through to the append open so the OS returns
    // the path error.
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

    writeln!(file, "{EXCLUDE_ENTRY}")
        .with_context(|| format!("Failed to write to {}", exclude_path.display()))?;

    Ok(())
}
