//! Mux session management (client-side).
#![cfg_attr(coverage_nightly, coverage(off))]

use super::protocol::{MuxRequest, MuxResponse};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tracing::debug;

const CLAUDE_ENTER_CSI_U: &[u8] = b"\x1b[13;1u";
const CLAUDE_CSI_U_SPLIT_DELAY_MS: u64 = 150;
/// Maximum raw input bytes accepted for one logical send.
///
/// This matches the server's per-window queue capacity so a send is accepted or rejected as one
/// queue entry. A single queue-sized JSON `SendInput` frame stays comfortably below the 16 MiB IPC
/// frame cap, even when `Vec<u8>` is encoded as JSON numbers.
pub const MAX_SEND_INPUT_BYTES: usize = super::backend::INPUT_QUEUE_CAPACITY_BYTES;
/// Conservative allowance for JSON enum tags, field names, brackets, commas, and target names.
///
/// JSON can encode each input byte as up to four payload bytes such as `255,`, so the compile-time
/// frame check multiplies the raw send cap by four and then adds this margin. The IPC boundary
/// remains authoritative for unusually large targets or future request shapes.
const SEND_INPUT_JSON_FRAME_OVERHEAD_BYTES: usize = 64 * 1024;
const SEND_INPUT_JSON_BYTE_OVERHEAD_FACTOR: usize = 4;

const _: () = assert!(MAX_SEND_INPUT_BYTES <= super::backend::INPUT_QUEUE_CAPACITY_BYTES);
const _: () = assert!(
    MAX_SEND_INPUT_BYTES
        .saturating_mul(SEND_INPUT_JSON_BYTE_OVERHEAD_FACTOR)
        .saturating_add(SEND_INPUT_JSON_FRAME_OVERHEAD_BYTES)
        <= super::ipc::MAX_FRAME_BYTES
);

/// Manager for mux sessions.
#[derive(Debug, Clone, Copy, Default)]
pub struct Manager;

type CurrentDir<'a> = dyn Fn() -> std::io::Result<PathBuf> + 'a;

fn resolve_working_dir_with_deps(
    working_dir: &Path,
    current_dir: &CurrentDir<'_>,
) -> Result<PathBuf> {
    let mut resolved = if working_dir.is_absolute() {
        working_dir.to_path_buf()
    } else {
        (current_dir)()
            .context("Failed to resolve current directory for mux working dir")?
            .join(working_dir)
    };

    if let Ok(canonical) = resolved.canonicalize() {
        resolved = canonical;
    }

    Ok(resolved)
}

#[cfg(coverage)]
pub(super) fn exercise_working_dir_paths_for_coverage() {
    let temp_dir = std::env::temp_dir();
    let _ = resolve_working_dir_with_deps(&temp_dir, &std::env::current_dir);
    let _ = resolve_working_dir_with_deps(Path::new("relative"), &|| Ok(temp_dir.clone()));
    let _ = resolve_working_dir_with_deps(Path::new("relative"), &|| {
        Err(std::io::Error::other("forced coverage current dir failure"))
    });
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
        Self::create_with_deps(name, working_dir, command, &std::env::current_dir)
    }

    fn create_with_deps(
        name: &str,
        working_dir: &Path,
        command: Option<&[String]>,
        current_dir: &CurrentDir<'_>,
    ) -> Result<()> {
        let working_dir = resolve_working_dir_with_deps(working_dir, current_dir)?;
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
            MuxResponse::Sessions { sessions } => {
                Ok(sessions.into_iter().map(Self::session_from_info).collect())
            }
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    #[inline(never)]
    fn session_from_info(info: super::protocol::SessionInfo) -> Session {
        Session {
            name: info.name,
            created: info.created,
            attached: info.attached,
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
        // On loaded PTYs, a very short split can still collapse into the same read on the client
        // side. Keep the gap comfortably above a typical poll interval.
        self.send_input_bytes(target, keys.as_bytes())?;
        std::thread::sleep(std::time::Duration::from_millis(
            CLAUDE_CSI_U_SPLIT_DELAY_MS,
        ));
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
    #[cfg_attr(coverage_nightly, coverage(off))]
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

    /// Send input to an agent, preserving Docker-specific Codex/Claude behavior.
    ///
    /// # Errors
    ///
    /// Returns an error if input cannot be sent/submitted.
    pub fn send_keys_and_submit_for_agent(
        &self,
        target: &str,
        agent: &crate::agent::Agent,
        keys: &str,
    ) -> Result<()> {
        match crate::conversation::detect_agent_cli(&agent.program) {
            crate::conversation::AgentCli::Claude => {
                self.send_keys_and_submit_csi_u_enter(target, keys)
            }
            crate::conversation::AgentCli::Codex => {
                if agent.runtime == crate::agent::AgentRuntime::Docker {
                    return self.paste_keys_and_submit(target, keys);
                }

                self.send_keys_and_submit_for_program(target, &agent.program, keys)
            }
            crate::conversation::AgentCli::Other => self.send_keys_and_submit(target, keys),
        }
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
    #[inline(never)]
    pub fn attach_command(name: &str) -> String {
        format!("tenex attach --session {name}")
    }

    /// Attach to a session.
    ///
    /// # Errors
    ///
    /// Returns an error because attach is not supported via the client API.
    #[inline(never)]
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
        Self::create_window_with_deps(
            session,
            window_name,
            working_dir,
            command,
            &std::env::current_dir,
        )
    }

    fn create_window_with_deps(
        session: &str,
        window_name: &str,
        working_dir: &Path,
        command: Option<&[String]>,
        current_dir: &CurrentDir<'_>,
    ) -> Result<u32> {
        let working_dir = resolve_working_dir_with_deps(working_dir, current_dir)?;
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
    /// Returns an error if the logical send is too large or cannot be accepted by the mux.
    pub fn send_keys_batch(&self, target: &str, keys: &[String]) -> Result<()> {
        let mut payload = Vec::new();
        for key in keys {
            append_key_to_logical_send(&mut payload, key.as_bytes())?;
        }

        if !payload.is_empty() {
            self.send_input_frame(target, &payload)?;
        }

        Ok(())
    }

    fn send_input_bytes(self, target: &str, data: &[u8]) -> Result<()> {
        self.send_input_frame(target, data)
    }

    fn send_input_frame(self, target: &str, data: &[u8]) -> Result<()> {
        let _ = self;
        validate_send_input_len(data.len())?;
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

fn append_key_to_logical_send(payload: &mut Vec<u8>, key: &[u8]) -> Result<()> {
    let new_len = payload
        .len()
        .checked_add(key.len())
        .ok_or_else(|| anyhow::anyhow!("input too large: input length overflow"))?;
    validate_send_input_len(new_len)?;
    payload.extend_from_slice(key);
    Ok(())
}

fn validate_send_input_len(len: usize) -> Result<()> {
    if len > MAX_SEND_INPUT_BYTES {
        bail!(
            "input too large: {len} bytes exceeds max single send size {MAX_SEND_INPUT_BYTES} bytes; input was not sent"
        );
    }
    Ok(())
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
    use interprocess::local_socket::traits::ListenerExt as _;
    use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
    use std::cell::Cell;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn current_dir_boom() -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("boom"))
    }

    fn maybe_send_input_data(request: MuxRequest) -> Option<Vec<u8>> {
        match request {
            MuxRequest::SendInput { data, .. } => Some(data),
            _ => None,
        }
    }

    fn is_pane_current_command(request: &MuxRequest) -> bool {
        matches!(request, MuxRequest::PaneCurrentCommand { .. })
    }

    fn make_mock_socket(dir: &TempDir) -> (String, interprocess::local_socket::Name<'static>) {
        #[cfg(windows)]
        {
            use interprocess::local_socket::GenericNamespaced;

            let display = format!("tenex-mux-session-test-{}", uuid::Uuid::new_v4());
            let name = display
                .clone()
                .to_ns_name::<GenericNamespaced>()
                .expect("Expected namespaced socket name")
                .into_owned();
            return (display, name);
        }

        #[cfg(not(windows))]
        {
            let socket_path = dir.path().join("mux.sock");
            let display = socket_path.to_string_lossy().into_owned();
            let name = socket_path
                .as_path()
                .to_fs_name::<GenericFilePath>()
                .expect("Expected filesystem socket name")
                .into_owned();
            (display, name)
        }
    }

    fn current_dir_from_temp<'a>(
        tmp: &'a TempDir,
        called: &'a Cell<bool>,
    ) -> impl Fn() -> std::io::Result<std::path::PathBuf> + 'a {
        || {
            called.set(true);
            Ok(tmp.path().to_path_buf())
        }
    }

    fn spawn_mock_server(
        name: interprocess::local_socket::Name<'static>,
        response: Arc<MuxResponse>,
        expected_requests: usize,
    ) -> std::thread::JoinHandle<()> {
        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("Expected mock mux listener to start");

        std::thread::spawn(move || {
            let mut handled = 0usize;
            for mut stream in listener.incoming().flatten() {
                while handled < expected_requests {
                    let Ok(_request) = crate::mux::read_json::<MuxRequest>(&mut stream) else {
                        break;
                    };

                    let _ = crate::mux::write_json(&mut stream, &*response);
                    handled = handled.saturating_add(1);
                }

                if handled >= expected_requests {
                    break;
                }
            }
        })
    }

    #[test]
    fn test_attach_command() {
        let cmd = Manager::attach_command("test-session");
        assert_eq!(cmd, "tenex attach --session test-session");
    }

    #[test]
    fn test_is_pane_current_command_smoke() {
        assert!(!is_pane_current_command(&MuxRequest::Ping));
        assert!(is_pane_current_command(&MuxRequest::PaneCurrentCommand {
            target: "session:0".to_string(),
        }));
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
    fn test_attach_returns_error() {
        let manager = Manager::new();
        let err = manager
            .attach("test-session")
            .expect_err("Expected attach to fail");
        assert!(err.to_string().contains("Attach is not supported"));
    }

    #[test]
    fn test_resolve_working_dir_with_deps_errors_when_current_dir_fails() {
        let current_dir = || Err(std::io::Error::other("boom"));
        let err = resolve_working_dir_with_deps(std::path::Path::new("relative"), &current_dir)
            .expect_err("Expected current_dir failure to error");
        assert!(
            err.to_string()
                .contains("Failed to resolve current directory for mux working dir")
        );

        let tmp = TempDir::new().expect("Expected temp dir");
        let absolute = tmp.path().join(".");
        let _ = current_dir_boom();
        let resolved = resolve_working_dir_with_deps(&absolute, &current_dir_boom)
            .expect("Expected absolute resolve");
        assert_eq!(
            resolved,
            absolute.canonicalize().expect("canonicalize absolute")
        );
    }

    #[test]
    fn test_resolve_working_dir_with_deps_joins_relative_paths_without_canonicalize() {
        let tmp = TempDir::new().expect("Expected temp dir");
        let called = Cell::new(false);
        let rel = std::path::PathBuf::from(format!("missing-{}", uuid::Uuid::new_v4()));

        let current_dir = current_dir_from_temp(&tmp, &called);
        let resolved = resolve_working_dir_with_deps(&rel, &current_dir)
            .expect("Expected resolve working dir");

        assert!(called.get());
        assert_eq!(resolved, tmp.path().join(&rel));
    }

    #[test]
    fn test_resolve_working_dir_with_deps_preserves_absolute_paths_and_canonicalizes_when_possible()
    {
        let tmp = TempDir::new().expect("Expected temp dir");
        let called = Cell::new(false);
        let subdir = tmp.path().join("subdir");
        std::fs::create_dir_all(&subdir).expect("create subdir");
        let with_dot = subdir.join(".");

        let current_dir = current_dir_from_temp(&tmp, &called);
        let resolved = resolve_working_dir_with_deps(&with_dot, &current_dir)
            .expect("Expected resolve working dir");

        assert!(!called.get());
        assert_eq!(
            resolved,
            subdir.canonicalize().expect("canonicalize subdir")
        );
    }

    #[test]
    fn test_create_with_deps_propagates_working_dir_resolution_failure() {
        let current_dir = || Err(std::io::Error::other("boom"));
        let err = Manager::create_with_deps(
            "session",
            std::path::Path::new("relative"),
            None,
            &current_dir,
        )
        .expect_err("Expected create to fail");
        assert!(
            err.to_string()
                .contains("Failed to resolve current directory for mux working dir")
        );

        crate::mux::set_socket_override("tenex-mux\0invalid/socket")
            .expect("Expected socket override");

        let tmp = TempDir::new().expect("Expected temp dir");
        let _ = current_dir_boom();
        let err = Manager::create_with_deps("session", tmp.path(), None, &current_dir_boom)
            .expect_err("Expected create to fail");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_create_window_with_deps_propagates_working_dir_resolution_failure() {
        let current_dir = || Err(std::io::Error::other("boom"));
        let err = Manager::create_window_with_deps(
            "session",
            "window",
            std::path::Path::new("relative"),
            None,
            &current_dir,
        )
        .expect_err("Expected window create to fail");
        assert!(
            err.to_string()
                .contains("Failed to resolve current directory for mux working dir")
        );

        crate::mux::set_socket_override("tenex-mux\0invalid/socket")
            .expect("Expected socket override");

        let tmp = TempDir::new().expect("Expected temp dir");
        let _ = current_dir_boom();
        let err = Manager::create_window_with_deps(
            "session",
            "window",
            tmp.path(),
            None,
            &current_dir_boom,
        )
        .expect_err("Expected window create to fail");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_create_resolves_relative_working_dir_before_request() {
        crate::mux::set_socket_override("tenex-mux\0invalid/socket")
            .expect("Expected socket override");

        let manager = Manager::new();
        let relative = std::path::PathBuf::from(format!("missing-{}", uuid::Uuid::new_v4()));
        let err = manager
            .create("session", &relative, None)
            .expect_err("Expected create to fail");
        assert!(!err.to_string().is_empty());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_manager_propagates_request_error() {
        let manager = Manager::new();

        crate::mux::set_socket_override("tenex-mux\0invalid/socket")
            .expect("Expected socket override");

        for result in [
            manager.list().map(|_| ()),
            manager.rename("old", "new"),
            manager.list_windows("session").map(|_| ()),
            manager.resize_window("session:0", 80, 24),
            manager.rename_window("session", 0, "new-name"),
        ] {
            let err = result.expect_err("Expected request error");
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn test_create_returns_error_when_session_already_exists() {
        let manager = Manager::new();
        let name = format!("tenex-test-session-{}", uuid::Uuid::new_v4());
        let workdir = TempDir::new().expect("Expected temp dir");

        manager
            .create(&name, workdir.path(), None)
            .expect("Expected session create");
        let err = manager
            .create(&name, workdir.path(), None)
            .expect_err("Expected duplicate session creation to fail");
        assert!(!err.to_string().is_empty());

        let _ = manager.kill(&name);
    }

    #[test]
    fn test_session_manager_reports_unexpected_responses() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);

        let response = Arc::new(MuxResponse::Pong {
            version: "mock".to_string(),
        });
        let server = spawn_mock_server(socket_name, response, 12);

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");

        let manager = Manager::new();
        let tmp = TempDir::new().expect("Expected temp dir");

        for result in [
            manager.create("session", tmp.path(), None),
            manager.kill("session"),
            manager.try_exists("session").map(|_| ()),
            manager.list().map(|_| ()),
            manager
                .create_window("session", "window", tmp.path(), None)
                .map(|_| ()),
            manager.kill_window("session", 0),
            manager.list_windows("session").map(|_| ()),
            manager.list_pane_pids("session").map(|_| ()),
            manager.resize_window("session:0", 80, 24),
            manager.rename_window("session", 0, "new-name"),
            manager.rename("session", "new-session"),
            manager.send_keys("session:0", "hi"),
        ] {
            let err = result.expect_err("expected unexpected response error");
            assert!(err.to_string().contains("Unexpected response"));
        }

        server.join().expect("mock server join");
    }

    #[test]
    fn test_spawn_mock_server_breaks_on_read_error_then_completes() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);

        let response = Arc::new(MuxResponse::Ok);
        let server = spawn_mock_server(socket_name, response, 1);

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        let endpoint = crate::mux::socket_endpoint().expect("Expected socket endpoint");

        let _ = interprocess::local_socket::Stream::connect(endpoint.name.clone())
            .expect("Expected connect to mock server");

        let mut stream = interprocess::local_socket::Stream::connect(endpoint.name)
            .expect("Expected second connect to mock server");
        crate::mux::write_json(&mut stream, &MuxRequest::Ping).expect("Expected ping write");

        server.join().expect("mock server join");
    }

    #[test]
    fn test_try_exists_propagates_err_response() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");
            let _request: MuxRequest =
                crate::mux::read_json(&mut stream).expect("Expected request");
            crate::mux::write_json(
                &mut stream,
                &MuxResponse::Err {
                    message: "mock exists error".to_string(),
                },
            )
            .expect("Expected response write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        let err = Manager::new()
            .try_exists("session")
            .expect_err("Expected err response");
        assert!(err.to_string().contains("mock exists error"));
        server.join().expect("mock server join");
    }

    #[test]
    fn test_list_propagates_err_response() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");
            let _request: MuxRequest =
                crate::mux::read_json(&mut stream).expect("Expected request");
            crate::mux::write_json(
                &mut stream,
                &MuxResponse::Err {
                    message: "mock list error".to_string(),
                },
            )
            .expect("Expected response write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        let err = Manager::new().list().expect_err("Expected err response");
        assert!(err.to_string().contains("mock list error"));
        server.join().expect("mock server join");
    }

    #[test]
    fn test_send_keys_and_submit_for_program_uses_paste_for_codex() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            assert!(is_pane_current_command(&request));
            crate::mux::write_json(
                &mut stream,
                &MuxResponse::Text {
                    text: "codex".to_string(),
                },
            )
            .expect("Expected command response write");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            let data = maybe_send_input_data(request).expect("Expected SendInput request");
            *captured_server.lock().expect("lock") = Some(data);

            let ping_data = maybe_send_input_data(MuxRequest::Ping);
            assert!(ping_data.is_none());
            crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        Manager::new()
            .send_keys_and_submit_for_program("session:0", "codex", "hi")
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("Expected captured data");
        assert!(data.starts_with(b"\x1b[200~"));
        assert!(data.ends_with(b"\x1b[201~\r"));
    }

    #[test]
    fn test_send_keys_and_submit_for_program_falls_back_when_pane_cmd_is_not_codex() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            assert!(is_pane_current_command(&request));
            crate::mux::write_json(
                &mut stream,
                &MuxResponse::Text {
                    text: "/bin/bash".to_string(),
                },
            )
            .expect("Expected response write");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            let data = maybe_send_input_data(request).expect("Expected SendInput request");
            *captured_server.lock().expect("lock") = Some(data);
            crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        Manager::new()
            .send_keys_and_submit_for_program("session:0", "codex", "hi")
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("Expected captured data");
        assert_eq!(data, b"hi\r");
    }

    #[test]
    fn test_send_keys_and_submit_for_program_falls_back_when_pane_command_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            assert!(is_pane_current_command(&request));
            crate::mux::write_json(
                &mut stream,
                &MuxResponse::Err {
                    message: "boom".to_string(),
                },
            )
            .expect("Expected command error write");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            let data = maybe_send_input_data(request).expect("Expected SendInput request");
            *captured_server.lock().expect("lock") = Some(data);
            crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        Manager::new()
            .send_keys_and_submit_for_program("session:0", "codex", "hi")
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("Expected captured data");
        assert_eq!(data, b"hi\r");
    }

    #[test]
    fn test_send_keys_and_submit_for_program_uses_csi_u_enter_for_claude() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            for _ in 0..2 {
                let request: MuxRequest =
                    crate::mux::read_json(&mut stream).expect("Expected request");
                let data = maybe_send_input_data(request).expect("Expected SendInput request");
                captured_server.lock().expect("lock").push(data);
                crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
            }
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        Manager::new()
            .send_keys_and_submit_for_program("session:0", "claude", "hi")
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured.lock().expect("lock");
        assert_eq!(data.len(), 2);
        assert_eq!(data[0], b"hi");
        assert_eq!(data[1], CLAUDE_ENTER_CSI_U);
    }

    #[test]
    fn test_send_keys_batch_sends_max_payload_as_single_frame() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            let data = maybe_send_input_data(request).expect("Expected SendInput request");
            *captured_server.lock().expect("lock") = Some(data);
            crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
        });

        let large = "x".repeat(MAX_SEND_INPUT_BYTES);
        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        Manager::new()
            .send_keys_batch("session:0", std::slice::from_ref(&large))
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("Expected captured data");
        assert_eq!(
            data,
            large.as_bytes(),
            "a logical send should stay in one SendInput frame"
        );
    }

    #[test]
    fn test_send_keys_batch_rejects_oversize_logical_send_before_ipc() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let large = "x".repeat(MAX_SEND_INPUT_BYTES + 1);

        let err = Manager::new()
            .send_keys_batch("session:0", std::slice::from_ref(&large))
            .expect_err("Expected oversized logical send to fail before IPC");
        let message = err.to_string();
        assert!(message.contains("input too large"));
        assert!(message.contains("max single send size"));
        assert!(message.contains("input was not sent"));
    }

    #[test]
    fn test_paste_keys_and_submit_rejects_oversize_payload_before_ipc() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let large = "x".repeat(MAX_SEND_INPUT_BYTES);

        let err = Manager::new()
            .paste_keys_and_submit("session:0", &large)
            .expect_err("Expected oversized bracketed paste to fail before IPC");
        let message = err.to_string();
        assert!(message.contains("input too large"));
        assert!(message.contains("input was not sent"));
    }

    #[test]
    fn test_send_keys_and_submit_for_agent_uses_program_path_for_host_codex() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            assert!(is_pane_current_command(&request));
            crate::mux::write_json(
                &mut stream,
                &MuxResponse::Text {
                    text: "codex".to_string(),
                },
            )
            .expect("Expected command response write");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            let data = maybe_send_input_data(request).expect("Expected SendInput request");
            *captured_server.lock().expect("lock") = Some(data);
            crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        let mut agent = crate::agent::Agent::new(
            "Codex".to_string(),
            "codex".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        agent.runtime = crate::agent::AgentRuntime::Host;

        Manager::new()
            .send_keys_and_submit_for_agent("session:0", &agent, "hi")
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("Expected captured data");
        assert!(data.starts_with(b"\x1b[200~"));
        assert!(data.ends_with(b"\x1b[201~\r"));
    }

    #[test]
    fn test_send_keys_and_submit_for_agent_uses_paste_for_docker_codex() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (socket_display, socket_name) = make_mock_socket(&socket_dir);
        let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_server = Arc::clone(&captured);

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        let server = std::thread::spawn(move || {
            let mut stream = listener
                .incoming()
                .next()
                .expect("Expected connection")
                .expect("Expected accept");

            let request: MuxRequest = crate::mux::read_json(&mut stream).expect("Expected request");
            let data = maybe_send_input_data(request).expect("Expected SendInput request");
            *captured_server.lock().expect("lock") = Some(data);

            let ping_data = maybe_send_input_data(MuxRequest::Ping);
            assert!(ping_data.is_none());
            crate::mux::write_json(&mut stream, &MuxResponse::Ok).expect("Expected ok write");
        });

        crate::mux::set_socket_override(&socket_display).expect("Expected socket override");
        let mut agent = crate::agent::Agent::new(
            "Codex".to_string(),
            "codex".to_string(),
            "branch".to_string(),
            std::path::PathBuf::from("/tmp"),
        );
        agent.runtime = crate::agent::AgentRuntime::Docker;

        Manager::new()
            .send_keys_and_submit_for_agent("session:0", &agent, "hi")
            .expect("Expected send keys");

        server.join().expect("mock server join");
        let data = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("Expected captured data");
        assert!(data.starts_with(b"\x1b[200~"));
        assert!(data.ends_with(b"\x1b[201~\r"));
    }
}
