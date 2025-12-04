//! Tmux output capture

use anyhow::{Context, Result, bail};
use std::process::Command;

/// Capture output from tmux sessions
#[derive(Debug, Clone, Copy, Default)]
pub struct Capture;

impl Capture {
    /// Create a new output capture instance
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Capture the visible pane content
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails
    pub fn capture_pane(&self, session: &str) -> Result<String> {
        let output = Command::new("tmux")
            .arg("capture-pane")
            .arg("-t")
            .arg(session)
            .arg("-p")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to capture pane for session '{session}': {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Capture pane with scroll-back history
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails
    pub fn capture_pane_with_history(&self, session: &str, lines: u32) -> Result<String> {
        let start = format!("-{lines}");

        let output = Command::new("tmux")
            .arg("capture-pane")
            .arg("-t")
            .arg(session)
            .arg("-p")
            .arg("-S")
            .arg(&start)
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to capture pane with history for session '{session}': {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Capture entire scroll-back buffer
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails
    pub fn capture_full_history(&self, session: &str) -> Result<String> {
        let output = Command::new("tmux")
            .arg("capture-pane")
            .arg("-t")
            .arg(session)
            .arg("-p")
            .arg("-S")
            .arg("-")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to capture full history for session '{session}': {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get the current pane size
    ///
    /// # Errors
    ///
    /// Returns an error if the size cannot be retrieved
    pub fn pane_size(&self, session: &str) -> Result<(u16, u16)> {
        let output = Command::new("tmux")
            .arg("display-message")
            .arg("-t")
            .arg(session)
            .arg("-p")
            .arg("#{pane_width}x#{pane_height}")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to get pane size for session '{session}': {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().split('x').collect();

        if parts.len() != 2 {
            bail!("Invalid pane size format: {stdout}");
        }

        let width: u16 = parts[0].parse().context("Invalid width")?;
        let height: u16 = parts[1].parse().context("Invalid height")?;

        Ok((width, height))
    }

    /// Get the cursor position in the pane
    ///
    /// # Errors
    ///
    /// Returns an error if the position cannot be retrieved
    pub fn cursor_position(&self, session: &str) -> Result<(u16, u16)> {
        let output = Command::new("tmux")
            .arg("display-message")
            .arg("-t")
            .arg(session)
            .arg("-p")
            .arg("#{cursor_x},#{cursor_y}")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to get cursor position for session '{session}': {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.trim().split(',').collect();

        if parts.len() != 2 {
            bail!("Invalid cursor position format: {stdout}");
        }

        let x: u16 = parts[0].parse().context("Invalid x position")?;
        let y: u16 = parts[1].parse().context("Invalid y position")?;

        Ok((x, y))
    }

    /// Check if the pane is running a program
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved
    pub fn pane_current_command(&self, session: &str) -> Result<String> {
        let output = Command::new("tmux")
            .arg("display-message")
            .arg("-t")
            .arg(session)
            .arg("-p")
            .arg("#{pane_current_command}")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to get current command for session '{session}': {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Get the last N lines from the pane
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails
    pub fn tail(&self, session: &str, lines: usize) -> Result<Vec<String>> {
        let line_count = u32::try_from(lines).unwrap_or(u32::MAX);
        let content = self.capture_pane_with_history(session, line_count)?;

        let mut result: Vec<String> = content
            .lines()
            .map(String::from)
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(lines)
            .collect();

        result.reverse();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip_if_no_tmux() -> bool {
        !super::super::is_available() || !super::super::is_server_running()
    }

    #[test]
    fn test_output_capture_new() {
        let capture = Capture::new();
        assert!(!format!("{capture:?}").is_empty());
    }

    #[test]
    fn test_capture_nonexistent_session() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.capture_pane("muster-nonexistent-session");
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_with_history_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.capture_pane_with_history("muster-nonexistent-session", 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_full_history_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.capture_full_history("muster-nonexistent-session");
        assert!(result.is_err());
    }

    #[test]
    fn test_pane_size_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.pane_size("muster-nonexistent-session");
        assert!(result.is_err());
    }

    #[test]
    fn test_cursor_position_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.cursor_position("muster-nonexistent-session");
        assert!(result.is_err());
    }

    #[test]
    fn test_pane_current_command_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.pane_current_command("muster-nonexistent-session");
        assert!(result.is_err() || result.as_ref().map(String::is_empty).unwrap_or(false));
    }

    #[test]
    fn test_tail_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let capture = Capture::new();
        let result = capture.tail("muster-nonexistent-session", 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_trait() {
        let capture = Capture;
        assert!(!format!("{capture:?}").is_empty());
    }

    #[test]
    fn test_capture_with_real_session() {
        use super::super::session::Manager as SessionManager;

        if skip_if_no_tmux() {
            return;
        }

        let manager = SessionManager::new();
        let session_name = "muster-test-capture";

        let _ = manager.kill(session_name);

        let result = manager.create(session_name, std::path::Path::new("/tmp"), None);

        if result.is_err() {
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(200));

        if !manager.exists(session_name) {
            return;
        }

        let capture = Capture::new();

        let _ = capture.capture_pane(session_name);

        if let Ok((width, height)) = capture.pane_size(session_name) {
            assert!(width > 0);
            assert!(height > 0);
        }

        let _ = capture.pane_current_command(session_name);

        let _ = manager.kill(session_name);
    }
}
