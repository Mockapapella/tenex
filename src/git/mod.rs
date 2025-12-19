//! Git operations module

#[cfg(not(windows))]
mod branch;
#[cfg(windows)]
mod branch_cli;

#[cfg(not(windows))]
mod diff;
#[cfg(windows)]
mod diff_cli;

#[cfg(not(windows))]
mod worktree;
#[cfg(windows)]
mod worktree_cli;

#[cfg(not(windows))]
pub use branch::{BranchInfo, Manager as BranchManager};
#[cfg(windows)]
pub use branch_cli::{BranchInfo, Manager as BranchManager};

#[cfg(not(windows))]
pub use diff::{FileChange, Generator as DiffGenerator, LineChange, Summary as DiffSummary};
#[cfg(windows)]
pub use diff_cli::{FileChange, Generator as DiffGenerator, LineChange, Summary as DiffSummary};

#[cfg(not(windows))]
pub use worktree::{Info as WorktreeInfo, Manager as WorktreeManager};
#[cfg(windows)]
pub use worktree_cli::{Info as WorktreeInfo, Manager as WorktreeManager};

#[cfg(windows)]
use anyhow::bail;
use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;
use std::process::Command;

#[cfg(not(windows))]
pub use git2::Repository;

#[cfg(windows)]
/// Lightweight repository metadata for CLI-based operations.
#[derive(Debug, Clone)]
pub struct Repository {
    /// Root working directory of the repository.
    pub root: PathBuf,
}

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

#[cfg(windows)]
pub(crate) fn git_output(repo_root: &Path, args: &[&str]) -> Result<String> {
    let output = git_command()
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("Failed to execute git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!(
                "git {} failed with status {}",
                args.join(" "),
                output.status
            );
        }
        bail!("git {} failed: {stderr}", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(windows)]
pub(crate) fn git_run(repo_root: &Path, args: &[&str]) -> Result<()> {
    git_output(repo_root, args)?;
    Ok(())
}

/// Open a git repository at the given path
///
/// # Errors
///
/// Returns an error if the path is not a git repository
#[cfg(not(windows))]
pub fn open_repository(path: &Path) -> Result<Repository> {
    Repository::discover(path)
        .with_context(|| format!("Failed to open git repository at {}", path.display()))
}

#[cfg(windows)]
/// Open a git repository at the given path
///
/// # Errors
///
/// Returns an error if the path is not a git repository
pub fn open_repository(path: &Path) -> Result<Repository> {
    let root = git_output(path, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("Failed to open git repository at {}", path.display()))?;
    let root_path = PathBuf::from(root);
    let root = if root_path.is_absolute() {
        root_path
    } else {
        path.join(root_path)
    };
    Ok(Repository { root })
}

/// Check if a path is inside a git repository
#[cfg(not(windows))]
#[must_use]
pub fn is_git_repository(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

#[cfg(windows)]
/// Check if a path is inside a git repository
#[must_use]
pub fn is_git_repository(path: &Path) -> bool {
    git_command()
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// Get the root of the git repository containing the given path
///
/// # Errors
///
/// Returns an error if the path is not inside a git repository
#[cfg(not(windows))]
pub fn repository_root(path: &Path) -> Result<std::path::PathBuf> {
    let repo = open_repository(path)?;
    repo.workdir()
        .map(std::path::Path::to_path_buf)
        .context("Repository has no working directory")
}

#[cfg(windows)]
/// Get the root of the git repository containing the given path
///
/// # Errors
///
/// Returns an error if the path is not inside a git repository
pub fn repository_root(path: &Path) -> Result<std::path::PathBuf> {
    let repo = open_repository(path)?;
    Ok(repo.root)
}

/// Ensure `.tenex/` is in `.git/info/exclude`
///
/// This prevents synthesis files from being tracked by git.
/// Creates the exclude file if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the exclude file cannot be read or written
#[cfg(not(windows))]
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

#[cfg(windows)]
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
    let exclude_path = git_path(&repo.root, "info/exclude")?;

    if let Some(parent) = exclude_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }

    if exclude_path.exists() {
        let file = fs::File::open(&exclude_path)
            .with_context(|| format!("Failed to open {}", exclude_path.display()))?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line.context("Failed to read exclude file")?;
            if line.trim() == EXCLUDE_ENTRY {
                return Ok(());
            }
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)
        .with_context(|| format!("Failed to open {} for writing", exclude_path.display()))?;

    writeln!(file, "{EXCLUDE_ENTRY}")
        .with_context(|| format!("Failed to write to {}", exclude_path.display()))?;

    Ok(())
}

#[cfg(windows)]
fn git_path(repo_root: &Path, git_path: &str) -> Result<PathBuf> {
    let path = git_output(repo_root, &["rev-parse", "--git-path", git_path])?;
    let path_buf = PathBuf::from(path);
    if path_buf.is_absolute() {
        Ok(path_buf)
    } else {
        Ok(repo_root.join(path_buf))
    }
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
        // Canonicalize both paths to handle macOS /var -> /private/var symlink
        let expected = temp_dir.path().canonicalize()?;
        let actual = root.canonicalize()?;
        assert_eq!(actual, expected);
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
