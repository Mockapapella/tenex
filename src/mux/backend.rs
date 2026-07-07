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
/// Maximum number of raw output bytes retained per window for per-client replay.
const OUTPUT_MAX_BYTES: usize = 4 * 1024 * 1024;

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
    /// Recent raw output history (used for per-client rendering).
    pub output_history: OutputHistory,
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

/// Buffered output bytes for a mux window with a monotonic sequence number.
#[derive(Debug, Default)]
pub struct OutputHistory {
    /// First sequence number still available in `buf`.
    pub seq_start: u64,
    /// Sequence number after the last byte observed.
    pub seq_end: u64,
    /// Raw PTY output bytes in the range `[seq_start, seq_end)`.
    pub buf: Vec<u8>,
    /// Optional checkpoint stream used to resync when the buffer rolls over.
    pub checkpoint: Option<OutputCheckpoint>,
}

/// A checkpoint byte stream representing the terminal state at `seq`.
#[derive(Debug, Clone)]
pub struct OutputCheckpoint {
    pub seq: u64,
    pub bytes: Vec<u8>,
}

impl OutputHistory {
    const fn should_checkpoint(&self, additional: usize) -> bool {
        self.buf.len().saturating_add(additional) > OUTPUT_MAX_BYTES
    }

    fn record(&mut self, chunk: &[u8], checkpoint_bytes: Option<Vec<u8>>) {
        let chunk_len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        self.seq_end = self.seq_end.saturating_add(chunk_len);

        if let Some(bytes) = checkpoint_bytes {
            self.seq_start = self.seq_end;
            self.buf.clear();
            self.checkpoint = Some(OutputCheckpoint {
                seq: self.seq_end,
                bytes,
            });
            return;
        }

        self.buf.extend_from_slice(chunk);
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
        let idx = parsed.window_index as usize;
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
    spawn_window_with_system(
        pty_system.as_ref(),
        index,
        window_name,
        working_dir,
        command,
        size,
    )
}

fn spawn_window_with_system(
    pty_system: &dyn portable_pty::PtySystem,
    index: u32,
    window_name: &str,
    working_dir: &Path,
    command: Option<&[String]>,
    size: PtySize,
) -> Result<Arc<Mutex<MuxWindow>>> {
    let pair = pty_system.openpty(size).context("Failed to open PTY")?;

    let reader = pair
        .master
        .try_clone_reader()
        .context("Failed to clone PTY reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("Failed to open PTY writer")?;

    let (cmd_builder, command_vec) = build_command_builder(command)?;
    let mut cmd_builder = cmd_builder;
    cmd_builder.cwd(working_dir);
    let child = pair
        .slave
        .spawn_command(cmd_builder)
        .context("Failed to spawn PTY command")?;

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
        output_history: OutputHistory::default(),
        size,
    }));

    spawn_reader_thread(window.clone(), reader);

    Ok(window)
}

fn spawn_reader_thread(window: Arc<Mutex<MuxWindow>>, reader: Box<dyn Read + Send>) {
    if let Err(err) = spawn_reader_thread_inner(window, reader) {
        warn!(error = %err, "Failed to spawn mux reader thread");
    }
}

#[cfg(test)]
thread_local! {
    static FORCE_READER_THREAD_SPAWN_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
struct ForceReaderThreadSpawnErrorGuard;

#[cfg(test)]
impl Drop for ForceReaderThreadSpawnErrorGuard {
    fn drop(&mut self) {
        FORCE_READER_THREAD_SPAWN_ERROR.with(|flag| flag.set(false));
    }
}

#[cfg(test)]
fn force_reader_thread_spawn_error() -> ForceReaderThreadSpawnErrorGuard {
    FORCE_READER_THREAD_SPAWN_ERROR.with(|flag| flag.set(true));
    ForceReaderThreadSpawnErrorGuard
}

fn spawn_reader_thread_inner(
    window: Arc<Mutex<MuxWindow>>,
    mut reader: Box<dyn Read + Send>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    #[cfg(test)]
    if FORCE_READER_THREAD_SPAWN_ERROR.with(std::cell::Cell::get) {
        return Err(std::io::Error::other("forced reader thread spawn failure"));
    }

    let (window_name, window_index) = {
        let guard = window.lock();
        (guard.name.clone(), guard.index)
    };
    let thread_name = format!("tenex-mux-{window_name}-{window_index}");

    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    let builder = std::thread::Builder::new().name(thread_name);
    builder.spawn(move || {
        tracing::dispatcher::with_default(&dispatch, move || {
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
                    let checkpoint_bytes = if guard.output_history.should_checkpoint(chunk.len()) {
                        Some(guard.parser.screen().state_formatted())
                    } else {
                        None
                    };
                    guard.output_history.record(chunk, checkpoint_bytes);
                    drop(guard);
                    continue;
                }

                let response_result = {
                    let mut guard = window.lock();
                    guard.parser.process(chunk);
                    let checkpoint_bytes = if guard.output_history.should_checkpoint(chunk.len()) {
                        Some(guard.parser.screen().state_formatted())
                    } else {
                        None
                    };
                    guard.output_history.record(chunk, checkpoint_bytes);
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
        });
    })
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

        // Some call sites persist custom commands as a single shell string
        // (for example "sh -c 'sleep 3600'"). On Windows, CreateProcessW treats
        // that as a literal executable path unless we split it first.
        let normalized = normalize_command_argv(argv);

        let args = normalized.iter().map(OsString::from).collect::<Vec<_>>();
        let builder = CommandBuilder::from_argv(args);
        return Ok((builder, normalized));
    }

    let builder = CommandBuilder::new_default_prog();
    Ok((builder, Vec::new()))
}

#[cfg(windows)]
fn normalize_command_argv(argv: &[String]) -> Vec<String> {
    if argv.len() != 1 {
        return argv.iter().map(String::from).collect();
    }

    let candidate = argv[0].trim();
    if candidate.contains(char::is_whitespace) {
        match shell_words::split(candidate) {
            Ok(split) if !split.is_empty() => split,
            _ => argv.iter().map(String::from).collect(),
        }
    } else {
        argv.iter().map(String::from).collect()
    }
}

#[cfg(not(windows))]
fn normalize_command_argv(argv: &[String]) -> Vec<String> {
    argv.iter().map(String::from).collect()
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
mod tests;
