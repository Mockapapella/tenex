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
    #[cfg(windows)]
    {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            return Some(PathBuf::from(home));
        }

        let drive = std::env::var_os("HOMEDRIVE");
        let path = std::env::var_os("HOMEPATH");
        if let (Some(drive), Some(path)) = (drive, path) {
            let mut combined = PathBuf::from(drive);
            combined.push(path);
            return Some(combined);
        }

        std::env::var_os("HOME").map(PathBuf::from)
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Resolve the local application data directory for the current platform.
#[must_use]
pub fn data_local_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        data_local_dir_from_env(
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            std::env::var_os("APPDATA").map(PathBuf::from),
        )
    }

    #[cfg(not(windows))]
    {
        data_local_dir_from_env(
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            home_dir(),
        )
    }
}

#[cfg(windows)]
pub(crate) fn data_local_dir_from_env(
    local_app_data: Option<PathBuf>,
    roaming_app_data: Option<PathBuf>,
) -> Option<PathBuf> {
    local_app_data.or(roaming_app_data)
}

#[cfg(not(windows))]
pub(crate) fn data_local_dir_from_env(
    xdg_data_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    xdg_data_home.or_else(|| {
        home.map(|home| {
            if cfg!(target_os = "macos") {
                home.join("Library").join("Application Support")
            } else {
                home.join(".local").join("share")
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_path_suffix() {
        let path = log_path();
        assert!(path.ends_with("tenex.log"));
    }

    #[cfg(not(windows))]
    #[test]
    fn test_data_local_dir_from_env_prefers_xdg_data_home() {
        assert_eq!(
            data_local_dir_from_env(
                Some(PathBuf::from("/tmp/tenex-xdg-data")),
                Some(PathBuf::from("/tmp/tenex-home")),
            ),
            Some(PathBuf::from("/tmp/tenex-xdg-data"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_data_local_dir_from_env_falls_back_to_home() {
        let expected = if cfg!(target_os = "macos") {
            PathBuf::from("/tmp/tenex-home")
                .join("Library")
                .join("Application Support")
        } else {
            PathBuf::from("/tmp/tenex-home")
                .join(".local")
                .join("share")
        };

        assert_eq!(
            data_local_dir_from_env(None, Some(PathBuf::from("/tmp/tenex-home"))),
            Some(expected)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_data_local_dir_from_env_returns_none_when_missing() {
        assert!(data_local_dir_from_env(None, None).is_none());
    }

    #[test]
    fn test_data_local_dir_returns_when_env_present() {
        let has_env = std::env::var_os("XDG_DATA_HOME").is_some()
            || std::env::var_os("HOME").is_some()
            || std::env::var_os("LOCALAPPDATA").is_some()
            || std::env::var_os("APPDATA").is_some();
        let resolved = data_local_dir();
        if has_env {
            assert!(resolved.is_some());
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn test_home_dir_matches_home_env() {
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(home_dir(), Some(std::path::PathBuf::from(home)));
        }
    }
}
