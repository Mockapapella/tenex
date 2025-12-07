//! Tmux session management

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use tracing::{debug, error, info};

/// Manager for tmux sessions
#[derive(Debug, Clone, Copy, Default)]
pub struct Manager;

impl Manager {
    /// Create a new session manager
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Create a new tmux session
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be created
    pub fn create(&self, name: &str, working_dir: &Path, command: Option<&str>) -> Result<()> {
        debug!(name, ?working_dir, command, "Creating tmux session");

        if self.exists(name) {
            error!(name, "Session already exists");
            bail!("Session '{name}' already exists");
        }

        let mut cmd = Command::new("tmux");
        cmd.arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(name)
            .arg("-c")
            .arg(working_dir);

        if let Some(shell_cmd) = command {
            // Wrap in shell to properly handle commands with arguments
            cmd.args(["sh", "-c", shell_cmd]);
        }

        let output = cmd.output().context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(name, %stderr, "Failed to create session");
            bail!("Failed to create session '{name}': {stderr}");
        }

        info!(name, "Tmux session created");
        Ok(())
    }

    /// Kill a tmux session
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be killed
    pub fn kill(&self, name: &str) -> Result<()> {
        debug!(name, "Killing tmux session");

        let output = Command::new("tmux")
            .arg("kill-session")
            .arg("-t")
            .arg(name)
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(name, %stderr, "Failed to kill session");
            bail!("Failed to kill session '{name}': {stderr}");
        }

        info!(name, "Tmux session killed");
        Ok(())
    }

    /// Check if a session exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        Command::new("tmux")
            .arg("has-session")
            .arg("-t")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// List all sessions
    ///
    /// # Errors
    ///
    /// Returns an error if sessions cannot be listed
    pub fn list(&self) -> Result<Vec<Session>> {
        let output = Command::new("tmux")
            .arg("list-sessions")
            .arg("-F")
            .arg("#{session_name}:#{session_created}:#{session_attached}")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let sessions = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 {
                    Some(Session {
                        name: parts[0].to_string(),
                        created: parts[1].parse().unwrap_or(0),
                        attached: parts[2] == "1",
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(sessions)
    }

    /// Send keys to a session
    ///
    /// # Errors
    ///
    /// Returns an error if keys cannot be sent
    pub fn send_keys(&self, name: &str, keys: &str) -> Result<()> {
        let output = Command::new("tmux")
            .arg("send-keys")
            .arg("-t")
            .arg(name)
            .arg(keys)
            .arg("Enter")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to send keys to session '{name}': {stderr}");
        }

        Ok(())
    }

    /// Rename a session
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be renamed
    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<()> {
        let output = Command::new("tmux")
            .arg("rename-session")
            .arg("-t")
            .arg(old_name)
            .arg(new_name)
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to rename session '{old_name}' to '{new_name}': {stderr}");
        }

        Ok(())
    }

    /// Get the attach command for a session
    #[must_use]
    pub fn attach_command(name: &str) -> String {
        format!("tmux attach-session -t {name}")
    }

    /// Attach to a session (this will replace the current process)
    ///
    /// # Errors
    ///
    /// Returns an error if exec fails
    pub fn attach(&self, name: &str) -> Result<()> {
        use std::os::unix::process::CommandExt;

        let err = Command::new("tmux")
            .arg("attach-session")
            .arg("-t")
            .arg(name)
            .exec();

        Err(err).context("Failed to attach to tmux session")
    }

    /// Create a new window in an existing session
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be created
    pub fn create_window(
        &self,
        session: &str,
        window_name: &str,
        working_dir: &Path,
        command: Option<&str>,
    ) -> Result<u32> {
        debug!(session, window_name, ?working_dir, "Creating tmux window");

        let mut cmd = Command::new("tmux");
        cmd.arg("new-window")
            .arg("-d") // Don't switch to the new window
            .arg("-t")
            .arg(session)
            .arg("-n")
            .arg(window_name)
            .arg("-c")
            .arg(working_dir)
            .arg("-P") // Print window info
            .arg("-F")
            .arg(concat!("#", "{window_index}"));

        if let Some(shell_cmd) = command {
            cmd.args(["sh", "-c", shell_cmd]);
        }

        let output = cmd.output().context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(session, window_name, %stderr, "Failed to create window");
            bail!("Failed to create window in session '{session}': {stderr}");
        }

        // Parse window index from output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let window_index = stdout
            .trim()
            .parse::<u32>()
            .context("Failed to parse window index")?;

        info!(session, window_name, window_index, "Tmux window created");
        Ok(window_index)
    }

    /// Kill a specific window in a session
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be killed
    pub fn kill_window(&self, session: &str, window_index: u32) -> Result<()> {
        let target = format!("{session}:{window_index}");
        debug!(%target, "Killing tmux window");

        let output = Command::new("tmux")
            .arg("kill-window")
            .arg("-t")
            .arg(&target)
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(%target, %stderr, "Failed to kill window");
            bail!("Failed to kill window '{target}': {stderr}");
        }

        debug!(%target, "Tmux window killed");
        Ok(())
    }

    /// Get the window target string for a session and window index
    #[must_use]
    pub fn window_target(session: &str, window_index: u32) -> String {
        format!("{session}:{window_index}")
    }

    /// List all windows in a session with their indices and names
    ///
    /// # Errors
    ///
    /// Returns an error if the windows cannot be listed
    pub fn list_windows(&self, session: &str) -> Result<Vec<Window>> {
        let output = Command::new("tmux")
            .arg("list-windows")
            .arg("-t")
            .arg(session)
            .arg("-F")
            .arg("#{window_index}:#{window_name}")
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let windows = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() >= 2 {
                    Some(Window {
                        index: parts[0].parse().ok()?,
                        name: parts[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(windows)
    }

    /// Resize a tmux window to specific dimensions
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be resized
    pub fn resize_window(&self, target: &str, width: u16, height: u16) -> Result<()> {
        let output = Command::new("tmux")
            .arg("resize-window")
            .arg("-t")
            .arg(target)
            .arg("-x")
            .arg(width.to_string())
            .arg("-y")
            .arg(height.to_string())
            .output()
            .context("Failed to execute tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to resize window '{target}': {stderr}");
        }

        Ok(())
    }
}

/// Information about a tmux session
#[derive(Debug, Clone)]
pub struct Session {
    /// Session name
    pub name: String,
    /// Unix timestamp of when the session was created
    pub created: i64,
    /// Whether a client is attached to this session
    pub attached: bool,
}

/// Information about a tmux window
#[derive(Debug, Clone)]
pub struct Window {
    /// Window index
    pub index: u32,
    /// Window name
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip_if_no_tmux() -> bool {
        !super::super::is_available() || !super::super::is_server_running()
    }

    #[test]
    fn test_session_manager_new() {
        let manager = Manager::new();
        assert!(!format!("{manager:?}").is_empty());
    }

    #[test]
    fn test_exists_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }
        let manager = Manager::new();
        assert!(!manager.exists("tenex-test-nonexistent-session-xyz"));
    }

    #[test]
    fn test_list_sessions() {
        if skip_if_no_tmux() {
            return;
        }
        let manager = Manager::new();
        let _sessions = manager.list();
    }

    #[test]
    fn test_attach_command() {
        let cmd = Manager::attach_command("test-session");
        assert_eq!(cmd, "tmux attach-session -t test-session");
    }

    #[test]
    fn test_tmux_session_struct() {
        let session = Session {
            name: "test".to_string(),
            created: 1_234_567_890,
            attached: false,
        };

        assert_eq!(session.name, "test");
        assert_eq!(session.created, 1_234_567_890);
        assert!(!session.attached);
    }

    #[test]
    fn test_create_kill_session() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let session_name = "tenex-test-create-kill";

        let _ = manager.kill(session_name);

        let result = manager.create(session_name, std::path::Path::new("/tmp"), None);

        if result.is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(100));

            if manager.exists(session_name) {
                let _ = manager.kill(session_name);
            }
        }
    }

    #[test]
    fn test_create_duplicate_session() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let session_name = "tenex-test-duplicate";

        let _ = manager.kill(session_name);

        let result = manager.create(session_name, std::path::Path::new("/tmp"), None);

        if result.is_ok() {
            let result2 = manager.create(session_name, std::path::Path::new("/tmp"), None);
            assert!(result2.is_err());

            let _ = manager.kill(session_name);
        }
    }

    #[test]
    fn test_kill_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let result = manager.kill("tenex-test-nonexistent-xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let result = manager.rename("tenex-nonexistent", "tenex-new");
        assert!(result.is_err());
    }

    #[test]
    fn test_send_keys_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let result = manager.send_keys("tenex-nonexistent", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_window_target() {
        let target = Manager::window_target("my-session", 5);
        assert_eq!(target, "my-session:5");
    }

    #[test]
    fn test_resize_window_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let result = manager.resize_window("tenex-nonexistent-xyz", 80, 24);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_window_nonexistent_session() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let result = manager.create_window(
            "tenex-nonexistent-xyz",
            "test",
            std::path::Path::new("/tmp"),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_kill_window_nonexistent() {
        if skip_if_no_tmux() {
            return;
        }

        let manager = Manager::new();
        let result = manager.kill_window("tenex-nonexistent-xyz", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_session_attached_true() {
        let session = Session {
            name: "test".to_string(),
            created: 1_234_567_890,
            attached: true,
        };
        assert!(session.attached);
    }
}
