//! Git operations module

mod branch;
mod diff;
mod worktree;

pub use branch::Manager as BranchManager;
pub use diff::{FileChange, Generator as DiffGenerator, LineChange, Summary as DiffSummary};
pub use worktree::{Info as WorktreeInfo, Manager as WorktreeManager};

use anyhow::{Context, Result};
use git2::Repository;
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
}
