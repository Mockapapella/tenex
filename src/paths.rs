//! Platform-specific filesystem path helpers.

use std::ffi::OsString;
use std::path::PathBuf;

/// Path to Tenex's debug log file.
///
/// This is located in the OS temp directory.
#[must_use]
pub fn log_path() -> PathBuf {
    std::env::temp_dir().join("tenex.log")
}

#[must_use]
fn home_dir_from(var_os: &mut impl FnMut(&'static str) -> Option<OsString>) -> Option<PathBuf> {
    var_os("HOME").map(PathBuf::from)
}

/// Locate the user's home directory without pulling in external crates.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    let mut var_os = |key: &'static str| std::env::var_os(key);
    home_dir_from(&mut var_os)
}

#[must_use]
fn data_local_dir_from(
    var_os: &mut impl FnMut(&'static str) -> Option<OsString>,
) -> Option<PathBuf> {
    var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir_from(var_os).map(|home| home.join(".local").join("share")))
}

/// Resolve the local application data directory for the current platform.
#[must_use]
pub fn data_local_dir() -> Option<PathBuf> {
    let mut var_os = |key: &'static str| std::env::var_os(key);
    data_local_dir_from(&mut var_os)
}

#[cfg(any(test, feature = "test-support"))]
/// Integration-test helpers for otherwise private path resolution logic.
pub mod test_support {
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// Resolve a home directory from an injected environment reader.
    #[must_use]
    pub fn home_dir_from(
        var_os: &mut impl FnMut(&'static str) -> Option<OsString>,
    ) -> Option<PathBuf> {
        super::home_dir_from(var_os)
    }

    /// Resolve a local data directory from an injected environment reader.
    #[must_use]
    pub fn data_local_dir_from(
        var_os: &mut impl FnMut(&'static str) -> Option<OsString>,
    ) -> Option<PathBuf> {
        super::data_local_dir_from(var_os)
    }
}
