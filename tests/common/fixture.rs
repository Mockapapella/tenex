//! Test fixture for setting up temporary git repositories

use std::fs;
use std::path::{Path, PathBuf};

use git2::{BranchType, Repository, Signature};
use tempfile::TempDir;
use tenex::agent::Storage;
use tenex::config::Config;
use tenex::git::WorktreeManager;
use tenex::mux::SessionManager;

/// Test fixture that sets up a temporary git repository
pub struct TestFixture {
    /// Temporary directory containing the git repo
    _temp_dir: TempDir,
    /// Path to the git repository
    pub repo_path: PathBuf,
    /// Temporary directory for worktrees
    pub worktree_dir: TempDir,
    /// Temporary directory for state storage
    pub state_dir: TempDir,
    /// Test-specific session prefix to avoid conflicts
    pub session_prefix: String,
}

impl TestFixture {
    pub fn new(test_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        // Canonicalize to handle macOS symlink (/var -> /private/var)
        let repo_path = temp_dir
            .path()
            .canonicalize()
            .unwrap_or_else(|_| temp_dir.path().to_path_buf());

        // Initialize git repo with initial commit
        let repo = Repository::init(&repo_path)?;

        // Make git CLI operations deterministic across environments.
        // Many integration tests call `git commit` / `git rebase` / `git merge` via the `git`
        // binary; those commands require author/committer identity and may be affected by
        // global config (e.g. `commit.gpgsign=true`).
        {
            let mut config = repo.config()?;
            config.set_str("user.name", "Tenex Test")?;
            config.set_str("user.email", "tenex@test.invalid")?;
            config.set_bool("commit.gpgsign", false)?;
        }
        let sig = Signature::now("Test", "test@test.com")?;

        // Create a file and commit it
        let readme_path = repo_path.join("README.md");
        fs::write(&readme_path, "# Test Repository\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        // Ensure a consistent default branch name for tests.
        // Some environments default to "main", but many tests assume "master".
        if repo.find_branch("master", BranchType::Local).is_err() {
            let head = repo.head()?.peel_to_commit()?;
            repo.branch("master", &head, true)?;
        }
        repo.set_head("refs/heads/master")?;
        repo.checkout_head(None)?;

        let worktree_dir = TempDir::new()?;
        let state_dir = TempDir::new()?;

        // Generate unique session prefix for this test run
        let session_prefix = format!("tenex-test-{}-{}", test_name, std::process::id());

        Ok(Self {
            _temp_dir: temp_dir,
            repo_path,
            worktree_dir,
            state_dir,
            session_prefix,
        })
    }

    pub fn config(&self) -> Config {
        // Canonicalize worktree_dir to handle macOS symlink (/var -> /private/var)
        let worktree_dir = self
            .worktree_dir
            .path()
            .canonicalize()
            .unwrap_or_else(|_| self.worktree_dir.path().to_path_buf());

        Config {
            // Use a long-running command instead of `claude` for testing.
            // Important: many integration tests assume mux sessions/windows stay alive long enough
            // for follow-up operations (spawn children, synthesize, etc). A short-lived command like
            // `echo` can exit immediately and cause flakiness across platforms.
            default_program: "sh -c 'sleep 3600'".to_string(),
            branch_prefix: format!("{}/", self.session_prefix),
            worktree_dir,
            auto_yes: false,
            poll_interval_ms: 100,
        }
    }

    pub fn storage_path(&self) -> PathBuf {
        self.state_dir.path().join("agents.json")
    }

    /// Returns the canonicalized worktree directory path.
    /// This handles macOS symlink (/var -> /private/var).
    pub fn worktree_path(&self) -> PathBuf {
        self.worktree_dir
            .path()
            .canonicalize()
            .unwrap_or_else(|_| self.worktree_dir.path().to_path_buf())
    }

    pub const fn create_storage() -> Storage {
        Storage::new()
    }

    pub fn session_name(&self, suffix: &str) -> String {
        format!("{}-{}", self.session_prefix, suffix)
    }

    /// Clean up any mux sessions created by this test
    pub fn cleanup_sessions(&self) {
        let manager = SessionManager::new();
        if let Ok(sessions) = manager.list() {
            for session in sessions {
                if session.name.starts_with(&self.session_prefix) {
                    let _ = manager.kill(&session.name);
                }
            }
        }
    }

    /// Clean up branches and worktrees from the test's repo
    pub fn cleanup_branches(&self) {
        if let Ok(repo) = Repository::open(&self.repo_path) {
            // Clean up worktrees
            let worktree_mgr = WorktreeManager::new(&repo);
            if let Ok(worktrees) = worktree_mgr.list() {
                for wt in worktrees {
                    if wt.name.starts_with(&self.session_prefix) {
                        let _ = worktree_mgr.remove(&wt.name);
                    }
                }
            }

            // Clean up branches
            let branch_mgr = tenex::git::BranchManager::new(&repo);
            if let Ok(branches) = branch_mgr.list() {
                for branch in branches {
                    if branch.starts_with(&self.session_prefix) {
                        let _ = branch_mgr.delete(&branch);
                    }
                }
            }
        }
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        self.cleanup_sessions();
        self.cleanup_branches();
        // Note: We intentionally do NOT call cleanup_storage() here.
        // Tests use isolated state via state_dir TempDir, so there's
        // no need to touch the real state file. Doing so would cause
        // race conditions with any running tenex instance.
    }
}
