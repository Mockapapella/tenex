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
#[cfg(windows)]
fn home_dir_from(var_os: &mut impl FnMut(&'static str) -> Option<OsString>) -> Option<PathBuf> {
    if let Some(home) = var_os("USERPROFILE") {
        return Some(PathBuf::from(home));
    }

    let drive = var_os("HOMEDRIVE");
    let path = var_os("HOMEPATH");
    if let (Some(drive), Some(path)) = (drive, path) {
        let mut combined = PathBuf::from(drive);
        combined.push(path);
        return Some(combined);
    }

    var_os("HOME").map(PathBuf::from)
}

#[must_use]
#[cfg(not(windows))]
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
#[cfg(windows)]
fn data_local_dir_from(
    var_os: &mut impl FnMut(&'static str) -> Option<OsString>,
) -> Option<PathBuf> {
    var_os("LOCALAPPDATA")
        .or_else(|| var_os("APPDATA"))
        .map(PathBuf::from)
}

#[must_use]
#[cfg(not(windows))]
fn data_local_dir_from(
    var_os: &mut impl FnMut(&'static str) -> Option<OsString>,
) -> Option<PathBuf> {
    var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| {
        home_dir_from(var_os).map(|home| {
            #[cfg(target_os = "macos")]
            {
                home.join("Library").join("Application Support")
            }

            #[cfg(not(target_os = "macos"))]
            {
                home.join(".local").join("share")
            }
        })
    })
}

/// Resolve the local application data directory for the current platform.
#[must_use]
pub fn data_local_dir() -> Option<PathBuf> {
    let mut var_os = |key: &'static str| std::env::var_os(key);
    data_local_dir_from(&mut var_os)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_path_suffix() {
        let path = log_path();
        assert!(path.ends_with("tenex.log"));
    }

    #[test]
    fn test_data_local_dir_returns_when_env_present() {
        let has_xdg_data_home = std::env::var_os("XDG_DATA_HOME").is_some();
        let has_home = std::env::var_os("HOME").is_some();
        let has_local_appdata = std::env::var_os("LOCALAPPDATA").is_some();
        let has_appdata = std::env::var_os("APPDATA").is_some();
        let has_env = has_xdg_data_home || has_home || has_local_appdata || has_appdata;
        let resolved = data_local_dir();
        assert_eq!(resolved.is_some(), has_env);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_home_dir_matches_home_env() {
        let expected = std::env::var_os("HOME").map(std::path::PathBuf::from);
        assert_eq!(home_dir(), expected);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_home_dir_from_reads_home() {
        let mut env = |key: &'static str| {
            (key == "HOME").then(|| std::ffi::OsString::from("/tmp/tenex-home"))
        };
        assert_eq!(
            home_dir_from(&mut env),
            Some(std::path::PathBuf::from("/tmp/tenex-home"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_data_local_dir_from_prefers_xdg_data_home() {
        let mut env = |key: &'static str| {
            (key == "XDG_DATA_HOME").then(|| std::ffi::OsString::from("/tmp/tenex-xdg"))
        };

        assert_eq!(
            data_local_dir_from(&mut env),
            Some(std::path::PathBuf::from("/tmp/tenex-xdg"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_data_local_dir_from_falls_back_to_home() {
        let mut env = |key: &'static str| {
            (key == "HOME").then(|| std::ffi::OsString::from("/tmp/tenex-home"))
        };

        #[cfg(target_os = "macos")]
        let expected = std::path::PathBuf::from("/tmp/tenex-home")
            .join("Library")
            .join("Application Support");

        #[cfg(not(target_os = "macos"))]
        let expected = std::path::PathBuf::from("/tmp/tenex-home")
            .join(".local")
            .join("share");

        assert_eq!(data_local_dir_from(&mut env), Some(expected));
    }

    #[cfg(not(windows))]
    #[test]
    fn test_data_local_dir_from_none_when_no_env() {
        let mut env = |_: &'static str| None::<std::ffi::OsString>;
        assert_eq!(data_local_dir_from(&mut env), None);
    }
}
