//! Helper functions for test setup and common operations

use std::path::{Path, PathBuf};

/// Check if tmux is available on the system
pub fn tmux_available() -> bool {
    tenex::tmux::is_available()
}

/// Skip a test if tmux is not available. Returns true if test should be skipped.
pub fn skip_if_no_tmux() -> bool {
    if !tmux_available() {
        eprintln!("Skipping test: tmux not available");
        return true;
    }
    false
}

/// Assert two paths are equal after canonicalization.
///
/// This is necessary on macOS where `/var` is a symlink to `/private/var`,
/// causing path comparisons to fail unexpectedly.
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
