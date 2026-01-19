//! Mux session management (client-side).

use super::protocol::{MuxRequest, MuxResponse};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tracing::debug;

const CLAUDE_ENTER_CSI_U: &[u8] = b"\x1b[13;1u";

/// Manager for mux sessions.
#[derive(Debug, Clone, Copy, Default)]
pub struct Manager;

fn resolve_working_dir(working_dir: &Path) -> Result<PathBuf> {
    let mut resolved = if working_dir.is_absolute() {
        working_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .context("Failed to resolve current directory for mux working dir")?
            .join(working_dir)
    };

    if let Ok(canonical) = resolved.canonicalize() {
        resolved = canonical;
    }

    Ok(resolved)
}

impl Manager {
    /// Create a new session manager.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Create a new mux session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be created.
    pub fn create(&self, name: &str, working_dir: &Path, command: Option<&[String]>) -> Result<()> {
        let working_dir = resolve_working_dir(working_dir)?;
        debug!(name, ?working_dir, ?command, "Creating mux session");

        let command = command
            .unwrap_or(&[])
            .iter()
            .map(String::from)
            .collect::<Vec<_>>();

        match super::client::request(&MuxRequest::CreateSession {
            name: name.to_string(),
            working_dir: working_dir.to_string_lossy().into_owned(),
            command,
            cols: super::backend::DEFAULT_COLS,
            rows: super::backend::DEFAULT_ROWS,
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Kill a mux session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be killed.
    pub fn kill(&self, name: &str) -> Result<()> {
        match super::client::request(&MuxRequest::KillSession {
            name: name.to_string(),
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Check if a session exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the daemon cannot be reached or returns an unexpected response.
    pub fn try_exists(&self, name: &str) -> Result<bool> {
        match super::client::request(&MuxRequest::SessionExists {
            name: name.to_string(),
        })? {
            MuxResponse::Bool { value } => Ok(value),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Check if a session exists.
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        self.try_exists(name).unwrap_or(false)
    }

    /// List all sessions.
    ///
    /// # Errors
    ///
    /// Returns an error if sessions cannot be listed.
    pub fn list(&self) -> Result<Vec<Session>> {
        match super::client::request(&MuxRequest::ListSessions)? {
            MuxResponse::Sessions { sessions } => Ok(sessions
                .into_iter()
                .map(|s| Session {
                    name: s.name,
                    created: s.created,
                    attached: s.attached,
                })
                .collect()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Send keys to a target (without pressing Enter).
    ///
    /// # Errors
    ///
    /// Returns an error if keys cannot be sent.
    pub fn send_keys(&self, target: &str, keys: &str) -> Result<()> {
        self.send_input_bytes(target, keys.as_bytes())
    }

    /// Send keys to a target and submit (normal typing).
    ///
    /// # Errors
    ///
    /// Returns an error if keys cannot be sent.
    pub fn send_keys_and_submit(&self, target: &str, keys: &str) -> Result<()> {
        let mut payload = Vec::with_capacity(keys.len() + 1);
        payload.extend_from_slice(keys.as_bytes());
        payload.push(b'\r');
        self.send_input_bytes(target, &payload)
    }

    /// Send keys to a target and submit using the CSI-u Enter sequence.
    ///
    /// Claude Code enables the kitty keyboard protocol and expects Enter to arrive as a CSI-u
    /// sequence instead of a raw carriage return byte. Without this, "submit" behaves like a
    /// literal newline in the prompt.
    ///
    /// # Errors
    ///
    /// Returns an error if input cannot be sent/submitted.
    pub fn send_keys_and_submit_csi_u_enter(&self, target: &str, keys: &str) -> Result<()> {
        // Claude Code sometimes fails to recognize CSI-u when it arrives in the same read as the
        // typed text. Send the message and the Enter sequence as two writes with a short delay.
        self.send_input_bytes(target, keys.as_bytes())?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        self.send_input_bytes(target, CLAUDE_ENTER_CSI_U)
    }

    /// Paste keys to a target and submit (bracketed paste when supported).
    ///
    /// # Errors
    ///
    /// Returns an error if keys cannot be pasted/submitted.
    pub fn paste_keys_and_submit(&self, target: &str, keys: &str) -> Result<()> {
        let mut payload = Vec::with_capacity(keys.len() + 16);
        payload.extend_from_slice(b"\x1b[200~");
        payload.extend_from_slice(keys.as_bytes());
        payload.extend_from_slice(b"\x1b[201~");
        payload.push(b'\r');
        self.send_input_bytes(target, &payload)
    }

    /// Send input to an agent program, using a program-specific strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if input cannot be sent/submitted.
    pub fn send_keys_and_submit_for_program(
        &self,
        target: &str,
        program: &str,
        keys: &str,
    ) -> Result<()> {
        let exe_stem = program
            .split_whitespace()
            .next()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .and_then(|s| s.to_str());

        if exe_stem == Some("claude") {
            // Claude Code enables the kitty keyboard protocol and may treat a raw carriage return
            // as a literal newline. Use CSI-u Enter to reliably "submit" prompts.
            return self.send_keys_and_submit_csi_u_enter(target, keys);
        }

        if exe_stem == Some("codex") {
            // Bracketed paste sequences break some default shells.
            // Only use the bracketed paste path when the pane is actually running codex.
            let capture = super::OutputCapture::new();
            if let Ok(pane_cmd) = capture.pane_current_command(target) {
                let pane_stem = std::path::Path::new(&pane_cmd)
                    .file_stem()
                    .and_then(|s| s.to_str());
                if pane_stem == Some("codex") {
                    return self.paste_keys_and_submit(target, keys);
                }
            }

            return self.send_keys_and_submit(target, keys);
        }

        self.send_keys_and_submit(target, keys)
    }

    /// Rename a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be renamed.
    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<()> {
        match super::client::request(&MuxRequest::RenameSession {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Get the attach command for a session.
    #[must_use]
    pub fn attach_command(name: &str) -> String {
        format!("tenex attach --session {name}")
    }

    /// Attach to a session.
    ///
    /// # Errors
    ///
    /// Returns an error because attach is not supported via the client API.
    pub fn attach(&self, _name: &str) -> Result<()> {
        bail!("Attach is not supported")
    }

    /// Create a new window in an existing session.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be created.
    pub fn create_window(
        &self,
        session: &str,
        window_name: &str,
        working_dir: &Path,
        command: Option<&[String]>,
    ) -> Result<u32> {
        let working_dir = resolve_working_dir(working_dir)?;
        let command = command
            .unwrap_or(&[])
            .iter()
            .map(String::from)
            .collect::<Vec<_>>();

        match super::client::request(&MuxRequest::CreateWindow {
            session: session.to_string(),
            window_name: window_name.to_string(),
            working_dir: working_dir.to_string_lossy().into_owned(),
            command,
            cols: super::backend::DEFAULT_COLS,
            rows: super::backend::DEFAULT_ROWS,
        })? {
            MuxResponse::WindowCreated { index } => Ok(index),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Kill a specific window in a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be killed.
    pub fn kill_window(&self, session: &str, window_index: u32) -> Result<()> {
        match super::client::request(&MuxRequest::KillWindow {
            session: session.to_string(),
            window_index,
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Get the window target string for a session and window index.
    #[must_use]
    pub fn window_target(session: &str, window_index: u32) -> String {
        format!("{session}:{window_index}")
    }

    /// List all windows in a session with their indices and names.
    ///
    /// # Errors
    ///
    /// Returns an error if the windows cannot be listed.
    pub fn list_windows(&self, session: &str) -> Result<Vec<Window>> {
        match super::client::request(&MuxRequest::ListWindows {
            session: session.to_string(),
        })? {
            MuxResponse::Windows { windows } => Ok(windows
                .into_iter()
                .map(|w| Window {
                    index: w.index,
                    name: w.name,
                })
                .collect()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// List pane PIDs for a session.
    ///
    /// # Errors
    ///
    /// Returns an error if panes cannot be listed.
    pub fn list_pane_pids(&self, session: &str) -> Result<Vec<u32>> {
        match super::client::request(&MuxRequest::ListPanePids {
            session: session.to_string(),
        })? {
            MuxResponse::Pids { pids } => Ok(pids),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Resize a window to specific dimensions.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be resized.
    pub fn resize_window(&self, target: &str, width: u16, height: u16) -> Result<()> {
        match super::client::request(&MuxRequest::Resize {
            target: target.to_string(),
            cols: width,
            rows: height,
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Rename a window in a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be renamed.
    pub fn rename_window(&self, session: &str, window_index: u32, new_name: &str) -> Result<()> {
        match super::client::request(&MuxRequest::RenameWindow {
            session: session.to_string(),
            window_index,
            new_name: new_name.to_string(),
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Send a batch of key sequences to a target.
    ///
    /// # Errors
    ///
    /// Returns an error if the input cannot be written.
    pub fn send_keys_batch(&self, target: &str, keys: &[String]) -> Result<()> {
        let mut payload = Vec::new();
        for key in keys {
            payload.extend_from_slice(key.as_bytes());
        }
        self.send_input_bytes(target, &payload)
    }

    fn send_input_bytes(self, target: &str, data: &[u8]) -> Result<()> {
        let _ = self;
        match super::client::request(&MuxRequest::SendInput {
            target: target.to_string(),
            data: data.to_vec(),
        })? {
            MuxResponse::Ok => Ok(()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }
}

/// Information about a mux session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Session name.
    pub name: String,
    /// Unix timestamp of when the session was created.
    pub created: i64,
    /// Whether a client is attached (reserved for future use).
    pub attached: bool,
}

/// Information about a mux window.
#[derive(Debug, Clone)]
pub struct Window {
    /// Window index.
    pub index: u32,
    /// Window name.
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_attach_command() {
        let cmd = Manager::attach_command("test-session");
        assert_eq!(cmd, "tenex attach --session test-session");
    }

    #[test]
    fn test_session_struct_debug() {
        let session = Session {
            name: "test".to_string(),
            created: 0,
            attached: false,
        };
        assert!(!format!("{session:?}").is_empty());
    }

    #[test]
    fn test_attach_returns_error() -> Result<(), Box<dyn std::error::Error>> {
        let manager = Manager::new();
        match manager.attach("test-session") {
            Ok(()) => Err("Expected attach to fail".into()),
            Err(err) => {
                assert!(err.to_string().contains("Attach is not supported"));
                Ok(())
            }
        }
    }

    #[test]
    fn test_create_returns_error_when_session_already_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let manager = Manager::new();
        let name = format!("tenex-test-session-{}", uuid::Uuid::new_v4());
        let workdir = TempDir::new()?;

        manager.create(&name, workdir.path(), None)?;
        let err = match manager.create(&name, workdir.path(), None) {
            Ok(()) => return Err("Expected duplicate session creation to fail".into()),
            Err(err) => err,
        };
        assert!(!err.to_string().is_empty());

        let _ = manager.kill(&name);
        Ok(())
    }
}
