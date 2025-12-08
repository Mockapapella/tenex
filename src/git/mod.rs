//! Git operations module

mod branch;
mod diff;
mod worktree;

pub use branch::{BranchInfo, Manager as BranchManager};
pub use diff::{FileChange, Generator as DiffGenerator, LineChange, Summary as DiffSummary};
pub use worktree::{Info as WorktreeInfo, Manager as WorktreeManager};

use anyhow::{Context, Result};
use git2::Repository;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

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
pub fn repository_root(path: &Path) -> Result<std::path::PathBuf> {
    let repo = open_repository(path)?;
    repo.workdir()
        .map(std::path::Path::to_path_buf)
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

    fn init_test_repo() -> Result<(TempDir, Repository), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let repo = Repository::init(temp_dir.path())?;
        Ok((temp_dir, repo))
    }

    #[test]
    fn test_is_git_repository() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, _repo) = init_test_repo()?;
        assert!(is_git_repository(temp_dir.path()));

        let non_repo = TempDir::new()?;
        assert!(!is_git_repository(non_repo.path()));
        Ok(())
    }

    #[test]
    fn test_open_repository() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, _repo) = init_test_repo()?;
        assert!(open_repository(temp_dir.path()).is_ok());

        let non_repo = TempDir::new()?;
        assert!(open_repository(non_repo.path()).is_err());
        Ok(())
    }

    #[test]
    fn test_repository_root() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, _repo) = init_test_repo()?;
        let root = repository_root(temp_dir.path())?;
        assert_eq!(root, temp_dir.path());
        Ok(())
    }

    #[test]
    fn test_ensure_tenex_excluded() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, _repo) = init_test_repo()?;

        // First call should add .tenex/ to exclude
        ensure_tenex_excluded(temp_dir.path())?;

        let exclude_path = temp_dir.path().join(".git/info/exclude");
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
        let (temp_dir, _repo) = init_test_repo()?;

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
}
