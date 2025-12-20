//! IPC protocol between the Tenex TUI and the mux daemon.

use serde::{Deserialize, Serialize};

/// A running mux session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session name.
    pub name: String,
    /// Unix timestamp when the session was created.
    pub created: i64,
    /// Whether a client is attached (reserved for future use).
    pub attached: bool,
}

/// A mux window inside a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Window index within the session.
    pub index: u32,
    /// Window title/name.
    pub name: String,
}

/// Which capture to perform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaptureKind {
    /// Only the visible screen.
    Visible,
    /// The last N lines (including scrollback).
    History {
        /// How many lines to capture.
        lines: u32,
    },
    /// Entire scrollback history.
    FullHistory,
}

/// A request sent from the client to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MuxRequest {
    /// Health check.
    Ping,
    /// List sessions.
    ListSessions,
    /// Check if a session exists.
    SessionExists {
        /// Session name.
        name: String,
    },
    /// Create a session (root window).
    CreateSession {
        /// Session name.
        name: String,
        /// Working directory.
        working_dir: String,
        /// Command argv (empty means the platform default shell).
        command: Vec<String>,
        /// Columns for initial PTY size.
        cols: u16,
        /// Rows for initial PTY size.
        rows: u16,
    },
    /// Kill a session (and all windows).
    KillSession {
        /// Session name.
        name: String,
    },
    /// Rename a session.
    RenameSession {
        /// Existing session name.
        old_name: String,
        /// New session name.
        new_name: String,
    },
    /// List windows in a session.
    ListWindows {
        /// Session name.
        session: String,
    },
    /// Create a window in a session.
    CreateWindow {
        /// Session name.
        session: String,
        /// Window title/name.
        window_name: String,
        /// Working directory.
        working_dir: String,
        /// Command argv (empty means the platform default shell).
        command: Vec<String>,
        /// Columns for initial PTY size.
        cols: u16,
        /// Rows for initial PTY size.
        rows: u16,
    },
    /// Kill a window by index.
    KillWindow {
        /// Session name.
        session: String,
        /// Window index.
        window_index: u32,
    },
    /// Rename a window by index.
    RenameWindow {
        /// Session name.
        session: String,
        /// Window index.
        window_index: u32,
        /// New window name.
        new_name: String,
    },
    /// Resize a target (`session` or `session:index`).
    Resize {
        /// Target string.
        target: String,
        /// New columns.
        cols: u16,
        /// New rows.
        rows: u16,
    },
    /// Send raw input bytes to a target.
    SendInput {
        /// Target string.
        target: String,
        /// Input bytes.
        data: Vec<u8>,
    },
    /// Capture formatted output for a target.
    Capture {
        /// Target string.
        target: String,
        /// Capture kind.
        kind: CaptureKind,
    },
    /// Get the pane size for a target.
    PaneSize {
        /// Target string.
        target: String,
    },
    /// Get cursor position for a target.
    CursorPosition {
        /// Target string.
        target: String,
    },
    /// Return the current command for a target.
    PaneCurrentCommand {
        /// Target string.
        target: String,
    },
    /// Return the last N non-empty lines from a target.
    Tail {
        /// Target string.
        target: String,
        /// Number of lines.
        lines: u32,
    },
    /// List process IDs for all windows in a session.
    ListPanePids {
        /// Session name.
        session: String,
    },
}

/// A response sent from the daemon to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MuxResponse {
    /// Successful response with no payload.
    Ok,
    /// Health check response.
    Pong {
        /// Daemon version string.
        version: String,
    },
    /// List of sessions.
    Sessions {
        /// Sessions.
        sessions: Vec<SessionInfo>,
    },
    /// List of windows.
    Windows {
        /// Windows.
        windows: Vec<WindowInfo>,
    },
    /// Index of a newly created window.
    WindowCreated {
        /// Window index.
        index: u32,
    },
    /// A UTF-8 string payload.
    Text {
        /// Text payload.
        text: String,
    },
    /// Raw bytes payload.
    Bytes {
        /// Bytes payload.
        data: Vec<u8>,
    },
    /// Boolean payload.
    Bool {
        /// Boolean value.
        value: bool,
    },
    /// Size payload.
    Size {
        /// Columns.
        cols: u16,
        /// Rows.
        rows: u16,
    },
    /// Cursor position payload.
    Position {
        /// Cursor x (column).
        x: u16,
        /// Cursor y (row).
        y: u16,
        /// Whether the cursor should be hidden.
        #[serde(default)]
        hidden: bool,
    },
    /// Process IDs payload.
    Pids {
        /// PIDs.
        pids: Vec<u32>,
    },
    /// Error response.
    Err {
        /// Human-readable error message.
        message: String,
    },
}
