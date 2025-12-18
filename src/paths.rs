//! Platform-specific filesystem path helpers.

use std::path::PathBuf;

/// Locate the user's home directory without pulling in external crates.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE");
                let path = std::env::var_os("HOMEPATH");
                match (drive, path) {
                    (Some(drive), Some(path)) => {
                        let mut combined = PathBuf::from(drive);
                        combined.push(path);
                        Some(combined)
                    }
                    _ => None,
                }
            })
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
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
        std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                home_dir().map(|home| {
                    if cfg!(target_os = "macos") {
                        home.join("Library").join("Application Support")
                    } else {
                        home.join(".local").join("share")
                    }
                })
            })
    }
}
