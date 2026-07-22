//! Platform-specific filesystem path helpers.

use std::path::PathBuf;

/// Path to Tenex's debug log file.
///
/// This is located in the OS temp directory.
#[must_use]
pub fn log_path() -> PathBuf {
    std::env::temp_dir().join("tenex.log")
}

/// Locate the user's home directory without pulling in external crates.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Resolve the local application data directory for the current platform.
#[must_use]
pub fn data_local_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local").join("share")))
}
