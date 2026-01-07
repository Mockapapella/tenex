//! Helper functions for test setup and common operations

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Create a `git` command with hook-related environment variables removed.
///
/// Git hooks can set variables like `GIT_DIR` which override repository discovery and ignore
/// `current_dir`. This ensures `git` operations in tests use the temporary repositories created
/// by the fixtures.
#[must_use]
pub fn git_command() -> Command {
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

/// Check if the mux is available on the system
pub const fn mux_available() -> bool {
    tenex::mux::is_available()
}

fn ensure_isolated_test_mux_socket() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let name = format!("tenex-mux-test-{}", std::process::id());
        if let Err(err) = tenex::mux::set_socket_override(&name) {
            eprintln!("Warning: failed to set mux socket override: {err}");
        }
    });
}

/// Skip a test if the mux is not available. Returns true if test should be skipped.
pub fn skip_if_no_mux() -> bool {
    ensure_isolated_test_mux_socket();
    if !mux_available() {
        eprintln!("Skipping test: mux not available");
        return true;
    }
    false
}

/// Assert two paths are equal after canonicalization.
///
/// This avoids failures when temp dirs are symlinked (for example `/var` -> `/private/var`).
pub fn assert_paths_eq(left: &Path, right: &Path, msg: &str) {
    let left_canonical = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right_canonical = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    assert_eq!(left_canonical, right_canonical, "{msg}");
}

/// Guard that restores the current directory when dropped (even on panic).
///
/// This is useful in tests that need to change directory but must ensure
/// the original directory is restored even if the test fails or panics.
pub struct DirGuard {
    original_dir: PathBuf,
}

impl DirGuard {
    /// Create a new guard that will restore to the current directory on drop.
    ///
    /// # Errors
    ///
    /// Returns an error if the current directory cannot be determined.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            original_dir: std::env::current_dir()?,
        })
    }
}

impl Drop for DirGuard {
    fn drop(&mut self) {
        if let Err(e) = std::env::set_current_dir(&self.original_dir) {
            eprintln!(
                "Warning: failed to restore working directory to {}: {e}",
                self.original_dir.display()
            );
        }
    }
}
