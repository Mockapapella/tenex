//! Tmux integration module

mod capture;
mod session;

pub use capture::Capture as OutputCapture;
pub use session::{Manager as SessionManager, Session};

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::process::Command;

fn default_tmux_bin() -> OsString {
    #[cfg(windows)]
    {
        use std::path::PathBuf;

        let msys2_tmux = PathBuf::from(r"C:\msys64\usr\bin\tmux.exe");
        if msys2_tmux.exists() {
            return msys2_tmux.into_os_string();
        }
    }

    OsString::from("tmux")
}

fn tmux_bin() -> OsString {
    std::env::var_os("TENEX_MUX_BIN")
        .or_else(|| std::env::var_os("TENEX_TMUX_BIN"))
        .unwrap_or_else(default_tmux_bin)
}

#[cfg(not(windows))]
fn tmux_command() -> Command {
    Command::new(tmux_bin())
}

#[cfg(windows)]
fn tmux_command() -> Command {
    use std::path::Path;

    let tmux = tmux_bin();
    let mut cmd = Command::new(&tmux);

    // Users commonly install tmux via MSYS2. If Tenex is invoking tmux via an
    // absolute path, prepend tmux's directory to PATH so tmux can spawn sibling
    // binaries like `bash` and `sleep`.
    if let Some(tmux_dir) = Path::new(&tmux).parent().filter(|p| p.is_absolute()) {
        let mut paths = vec![tmux_dir.to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }

    cmd
}

/// Check if tmux is available on the system
#[must_use]
pub fn is_available() -> bool {
    tmux_command()
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if tmux server is running and accepting commands
#[must_use]
pub fn is_server_running() -> bool {
    tmux_command()
        .arg("list-sessions")
        .output()
        .map(|o| {
            let stderr = String::from_utf8_lossy(&o.stderr);
            !stderr.contains("no server running")
        })
        .unwrap_or(false)
}

/// Get the tmux version
///
/// # Errors
///
/// Returns an error if tmux is not available or version cannot be parsed
pub fn version() -> Result<String> {
    let output = tmux_command()
        .arg("-V")
        .output()
        .context("Failed to execute tmux")?;

    if !output.status.success() {
        anyhow::bail!("tmux -V failed");
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_available() {
        let _available = is_available();
    }

    #[test]
    fn test_version_when_available() -> Result<(), Box<dyn std::error::Error>> {
        if is_available() {
            let version = version()?;
            assert!(version.starts_with("tmux"));
        }
        Ok(())
    }
}
