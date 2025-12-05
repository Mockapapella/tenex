//! Tmux integration module

mod capture;
mod session;

pub use capture::Capture as OutputCapture;
pub use session::{Manager as SessionManager, Session};

use anyhow::{Context, Result};
use std::process::Command;

/// Check if tmux is available on the system
#[must_use]
pub fn is_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if tmux server is running and accepting commands
#[must_use]
pub fn is_server_running() -> bool {
    Command::new("tmux")
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
    let output = Command::new("tmux")
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
