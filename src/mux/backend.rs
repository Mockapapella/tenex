//! Internal PTY-backed multiplexer state.

use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

/// Default PTY rows when no UI size is known.
pub const DEFAULT_ROWS: u16 = 24;
/// Default PTY columns when no UI size is known.
pub const DEFAULT_COLS: u16 = 80;
/// Default scrollback buffer size in lines.
pub const DEFAULT_SCROLLBACK: usize = 10_000;
/// Number of trailing bytes retained to detect terminal queries split across reads.
const TERMINAL_QUERY_TAIL: usize = 32;

/// Global mux state shared by the session and capture APIs.
#[derive(Debug, Default)]
pub struct MuxState {
    /// Active sessions keyed by session name.
    pub sessions: HashMap<String, Arc<Mutex<MuxSession>>>,
}

/// A mux session containing a root window and its child windows.
#[derive(Debug)]
pub struct MuxSession {
    /// Session name.
    pub name: String,
    /// Unix timestamp when the session was created.
    pub created: i64,
    /// Number of times the root window has been restarted after exiting.
    pub root_restart_attempts: u32,
    /// Unix timestamp of the most recent root window restart.
    pub last_root_restart: i64,
    /// Windows in index order (index 0 is the root window).
    pub windows: Vec<Arc<Mutex<MuxWindow>>>,
}

/// A window with its PTY and terminal state.
pub struct MuxWindow {
    /// Window index within the session.
    pub index: u32,
    /// Window name.
    pub name: String,
    /// Working directory for the spawned process.
    pub working_dir: PathBuf,
    /// Command argv used to spawn the process.
    pub command: Vec<String>,
    /// PTY master handle.
    pub master: Box<dyn MasterPty + Send>,
    /// PTY writer handle.
    pub writer: Box<dyn Write + Send>,
    /// Child process handle.
    pub child: Box<dyn Child + Send + Sync>,
    /// Terminal parser with scrollback.
    pub parser: vt100::Parser,
    /// Current PTY size.
    pub size: PtySize,
}

impl std::fmt::Debug for MuxWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuxWindow")
            .field("index", &self.index)
            .field("name", &self.name)
            .field("working_dir", &self.working_dir)
            .field("command", &self.command)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// Parsed window target (session + window index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTarget {
    /// Session name.
    pub session: String,
    /// Window index within the session (0 for root window).
    pub window_index: u32,
}

static MUX_STATE: OnceLock<Arc<Mutex<MuxState>>> = OnceLock::new();

/// Access the global mux state.
pub fn global_state() -> &'static Arc<Mutex<MuxState>> {
    MUX_STATE.get_or_init(|| Arc::new(Mutex::new(MuxState::default())))
}

/// Build a PTY size struct with defaults.
#[must_use]
pub const fn default_pty_size() -> PtySize {
    PtySize {
        rows: DEFAULT_ROWS,
        cols: DEFAULT_COLS,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Parse a session target string into a window target.
///
/// Targets are either `session` (root window) or `session:index`.
///
/// # Errors
///
/// Returns an error if the window index cannot be parsed.
pub fn parse_target(target: &str) -> Result<WindowTarget> {
    if let Some((session, raw_index)) = target.split_once(':') {
        let index = raw_index
            .trim()
            .parse::<u32>()
            .context("Invalid window index")?;
        return Ok(WindowTarget {
            session: session.to_string(),
            window_index: index,
        });
    }

    Ok(WindowTarget {
        session: target.to_string(),
        window_index: 0,
    })
}

/// Resolve a window from a target string.
///
/// # Errors
///
/// Returns an error if the session or window cannot be found.
pub fn resolve_window(target: &str) -> Result<Arc<Mutex<MuxWindow>>> {
    let parsed = parse_target(target)?;
    let state = global_state();
    let session = {
        let guard = state.lock();
        guard
            .sessions
            .get(&parsed.session)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Session '{}' not found", parsed.session))?
    };

    let window = {
        let session_guard = session.lock();
        let idx = usize::try_from(parsed.window_index).context("Invalid window index")?;
        session_guard
            .windows
            .get(idx)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Window '{}' not found", parsed.window_index))?
    };

    Ok(window)
}

/// Spawn a PTY-backed window and start its reader thread.
///
/// # Errors
///
/// Returns an error if the PTY or child process cannot be created.
pub fn spawn_window(
    index: u32,
    window_name: &str,
    working_dir: &Path,
    command: Option<&[String]>,
    size: PtySize,
) -> Result<Arc<Mutex<MuxWindow>>> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(size).context("Failed to open PTY")?;

    let (cmd_builder, command_vec) = build_command_builder(command)?;
    let mut cmd_builder = cmd_builder;
    cmd_builder.cwd(working_dir);
    let child = pair
        .slave
        .spawn_command(cmd_builder)
        .context("Failed to spawn PTY command")?;

    let reader = pair
        .master
        .try_clone_reader()
        .context("Failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("Failed to open PTY writer")?;

    let parser = vt100::Parser::new(size.rows, size.cols, DEFAULT_SCROLLBACK);

    let window = Arc::new(Mutex::new(MuxWindow {
        index,
        name: window_name.to_string(),
        working_dir: working_dir.to_path_buf(),
        command: command_vec,
        master: pair.master,
        writer,
        child,
        parser,
        size,
    }));

    spawn_reader_thread(window.clone(), reader);

    Ok(window)
}

fn spawn_reader_thread(window: Arc<Mutex<MuxWindow>>, mut reader: Box<dyn Read + Send>) {
    let (window_name, window_index) = {
        let guard = window.lock();
        (guard.name.clone(), guard.index)
    };
    let thread_name = format!("tenex-mux-{window_name}-{window_index}");

    let builder = std::thread::Builder::new().name(thread_name);
    if let Err(err) = builder.spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut query_tail = Vec::new();
        let mut scan_buf = Vec::new();
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) => {
                    debug!(error = %err, "Mux reader exiting");
                    break;
                }
            };

            let chunk = &buffer[..read];
            let (cpr_queries, da_queries, osc10_queries, osc11_queries) =
                scan_terminal_queries(&mut scan_buf, &query_tail, chunk);
            update_terminal_query_tail(&mut query_tail, &scan_buf);

            if cpr_queries == 0 && da_queries == 0 && osc10_queries == 0 && osc11_queries == 0 {
                let mut guard = window.lock();
                guard.parser.process(chunk);
                drop(guard);
                continue;
            }

            let response_result = {
                let mut guard = window.lock();
                guard.parser.process(chunk);
                let result = respond_to_terminal_queries(
                    &mut guard,
                    cpr_queries,
                    da_queries,
                    osc10_queries,
                    osc11_queries,
                );
                drop(guard);
                result
            };

            if let Err(err) = response_result {
                debug!(error = %err, "Failed to respond to terminal query");
                break;
            }
        }
    }) {
        warn!(error = %err, "Failed to spawn mux reader thread");
    }
}

fn scan_terminal_queries(
    scan_buf: &mut Vec<u8>,
    tail: &[u8],
    chunk: &[u8],
) -> (usize, usize, usize, usize) {
    scan_buf.clear();
    scan_buf.reserve(tail.len().saturating_add(chunk.len()));
    scan_buf.extend_from_slice(tail);
    scan_buf.extend_from_slice(chunk);

    let tail_len = tail.len();
    let cpr = count_pattern(scan_buf, b"\x1b[6n", tail_len);
    let da = count_pattern(scan_buf, b"\x1b[c", tail_len);
    let osc10 = count_pattern(scan_buf, b"\x1b]10;?\x07", tail_len).saturating_add(count_pattern(
        scan_buf,
        b"\x1b]10;?\x1b\\",
        tail_len,
    ));
    let osc11 = count_pattern(scan_buf, b"\x1b]11;?\x07", tail_len).saturating_add(count_pattern(
        scan_buf,
        b"\x1b]11;?\x1b\\",
        tail_len,
    ));
    (cpr, da, osc10, osc11)
}

fn update_terminal_query_tail(tail: &mut Vec<u8>, scan_buf: &[u8]) {
    let keep = scan_buf.len().min(TERMINAL_QUERY_TAIL);
    tail.clear();
    if keep == 0 {
        return;
    }
    let start = scan_buf.len().saturating_sub(keep);
    tail.extend_from_slice(&scan_buf[start..]);
}

fn count_pattern(haystack: &[u8], needle: &[u8], tail_len: usize) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }

    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(idx, window)| *window == needle && idx.saturating_add(needle.len()) > tail_len)
        .count()
}

fn respond_to_terminal_queries(
    window: &mut MuxWindow,
    cpr: usize,
    da: usize,
    osc10: usize,
    osc11: usize,
) -> Result<()> {
    let outbound = build_terminal_query_responses(window.parser.screen(), cpr, da, osc10, osc11);
    if outbound.is_empty() {
        return Ok(());
    }

    window
        .writer
        .write_all(&outbound)
        .context("Failed to write terminal query response")?;
    window
        .writer
        .flush()
        .context("Failed to flush terminal query response")?;
    Ok(())
}

fn build_terminal_query_responses(
    screen: &vt100::Screen,
    cpr: usize,
    da: usize,
    osc10: usize,
    osc11: usize,
) -> Vec<u8> {
    let mut outbound = Vec::new();

    if cpr > 0 {
        // vt100 reports 0-based positions, but terminals respond 1-based.
        let (row, col) = screen.cursor_position();
        let row = row.saturating_add(1);
        let col = col.saturating_add(1);
        let response = format!("\x1b[{row};{col}R");
        for _ in 0..cpr {
            outbound.extend_from_slice(response.as_bytes());
        }
    }

    if da > 0 {
        // A minimal "VT100" response is sufficient for most clients.
        for _ in 0..da {
            outbound.extend_from_slice(b"\x1b[?1;0c");
        }
    }

    if osc10 > 0 {
        for _ in 0..osc10 {
            outbound.extend_from_slice(b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
        }
    }

    if osc11 > 0 {
        for _ in 0..osc11 {
            outbound.extend_from_slice(b"\x1b]11;rgb:0000/0000/0000\x1b\\");
        }
    }

    outbound
}

fn build_command_builder(command: Option<&[String]>) -> Result<(CommandBuilder, Vec<String>)> {
    if let Some(argv) = command {
        if argv.is_empty() {
            bail!("Cannot spawn PTY: empty argv");
        }
        let args = argv.iter().map(OsString::from).collect::<Vec<_>>();
        let builder = CommandBuilder::from_argv(args);
        return Ok((builder, argv.iter().map(String::from).collect()));
    }

    let builder = CommandBuilder::new_default_prog();
    Ok((builder, Vec::new()))
}

/// Return current Unix timestamp in seconds.
#[must_use]
pub fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or_default()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_pty_size_has_nonzero_dims() {
        let size = default_pty_size();
        assert_eq!(size.rows, DEFAULT_ROWS);
        assert_eq!(size.cols, DEFAULT_COLS);
    }

    #[test]
    fn test_parse_target_root_and_indexed() -> Result<()> {
        let root = parse_target("session")?;
        assert_eq!(
            root,
            WindowTarget {
                session: "session".to_string(),
                window_index: 0
            }
        );

        let indexed = parse_target("session:3")?;
        assert_eq!(
            indexed,
            WindowTarget {
                session: "session".to_string(),
                window_index: 3
            }
        );

        assert!(parse_target("session:not-a-number").is_err());

        Ok(())
    }

    #[test]
    fn test_count_pattern_respects_tail_boundary() {
        let haystack = b"abcd\x1b[c----\x1b[c";
        let needle = b"\x1b[c";

        // Tail boundary after first match should only count second match.
        let count = count_pattern(haystack, needle, 10);
        assert_eq!(count, 1);

        // No tail boundary counts both.
        let count = count_pattern(haystack, needle, 0);
        assert_eq!(count, 2);
    }

    #[test]
    fn test_scan_terminal_queries_across_chunks() {
        let mut scan_buf = Vec::new();
        let tail = b"\x1b[";
        let chunk = b"6n\x1b[c";
        let (cpr, da, osc10, osc11) = scan_terminal_queries(&mut scan_buf, tail, chunk);
        assert_eq!(cpr, 1);
        assert_eq!(da, 1);
        assert_eq!(osc10, 0);
        assert_eq!(osc11, 0);

        let mut tail_buf = Vec::new();
        update_terminal_query_tail(&mut tail_buf, &scan_buf);
        assert!(!tail_buf.is_empty());
        assert!(tail_buf.len() <= TERMINAL_QUERY_TAIL);
    }

    #[test]
    fn test_build_command_builder_rejects_empty_argv() {
        let argv: Vec<String> = Vec::new();
        let err = build_command_builder(Some(&argv));
        assert!(err.is_err());
    }

    #[test]
    fn test_unix_timestamp_non_negative() {
        assert!(unix_timestamp() >= 0);
    }

    #[test]
    fn test_terminal_query_responses_end_to_end() -> Result<()> {
        let session_name = format!("tenex-test-backend-{}", uuid::Uuid::new_v4());
        let tmp = std::env::temp_dir();

        let script = concat!(
            "stty raw -echo min 0 time 10; ",
            "printf 'DA:'; printf '\\033[c\\n'; dd bs=1 count=16 2>/dev/null | od -An -tx1; printf '\\n'; ",
            "printf 'CPR:'; printf '\\033[6n\\n'; dd bs=1 count=16 2>/dev/null | od -An -tx1; printf '\\n'; ",
            "printf 'OSC10:'; printf '\\033]10;?\\a\\n'; dd bs=1 count=25 2>/dev/null | od -An -tx1; printf '\\n'; ",
            "printf 'OSC11:'; printf '\\033]11;?\\a\\n'; dd bs=1 count=25 2>/dev/null | od -An -tx1; printf '\\n'; ",
            "stty sane",
        );

        let command = vec!["sh".to_string(), "-c".to_string(), script.to_string()];

        let _ = super::super::server::SessionManager::kill(&session_name);
        super::super::server::SessionManager::create(&session_name, &tmp, Some(&command))?;

        std::thread::sleep(std::time::Duration::from_secs(4));

        let output = super::super::server::OutputCapture::capture_full_history(&session_name)?;
        let _ = super::super::server::SessionManager::kill(&session_name);

        assert!(output.contains("DA:"), "full output: {output:?}");
        assert!(output.contains("CPR:"), "full output: {output:?}");
        assert!(output.contains("OSC10:"), "full output: {output:?}");
        assert!(output.contains("OSC11:"), "full output: {output:?}");

        // Primary device attributes response: ESC [ ? 1 ; 0 c
        assert!(output.contains("1b 5b 3f 31 3b 30 63"));

        // Text color and background color responses are OSC 10/11.
        assert!(output.contains("1b 5d 31 30"), "full output: {output:?}");
        assert!(output.contains("1b 5d 31 31"), "full output: {output:?}");

        Ok(())
    }

    #[test]
    fn test_build_terminal_query_responses_da() {
        let parser = vt100::Parser::new(2, 3, 0);
        let bytes = build_terminal_query_responses(parser.screen(), 0, 1, 0, 0);
        assert_eq!(bytes, b"\x1b[?1;0c");
    }

    #[test]
    fn test_build_terminal_query_responses_cpr_uses_one_based_coords() {
        let mut parser = vt100::Parser::new(2, 3, 0);
        parser.process(b"A");
        let bytes = build_terminal_query_responses(parser.screen(), 1, 0, 0, 0);
        assert_eq!(bytes, b"\x1b[1;2R");
    }

    #[test]
    fn test_build_terminal_query_responses_osc10_and_osc11() {
        let parser = vt100::Parser::new(2, 3, 0);
        let bytes = build_terminal_query_responses(parser.screen(), 0, 0, 1, 1);
        assert_eq!(
            bytes,
            b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]11;rgb:0000/0000/0000\x1b\\"
        );
    }
}
