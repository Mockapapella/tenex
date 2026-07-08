//! Feature-gated test support utilities.
//!
//! These helpers are exposed for Tenex's internal tests and external test
//! crates that enable `test-support`. They are not stable public API and carry
//! no backwards-compatibility promise.

use std::io;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

fn mux_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn env_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Acquire the process-wide mux test lock for deterministic mux tests.
///
/// This is a test-support boundary for Tenex tests, not a stable public API.
pub fn lock_mux_test_environment() -> MutexGuard<'static, ()> {
    mux_test_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Acquire the process-wide environment mutation lock for deterministic tests.
///
/// This is a test-support boundary for Tenex tests, not a stable public API.
pub fn lock_env_test_environment() -> MutexGuard<'static, ()> {
    env_test_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Return a unique `/tmp` mux socket path with a sanitized test tag.
///
/// This is a test-support boundary for Tenex tests, not a stable public API.
#[must_use]
pub fn unique_mux_socket_path(tag: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tag = tag
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>();

    if tag.is_empty() {
        return format!("/tmp/tx-mux-{pid}-{counter}.sock");
    }

    format!("/tmp/tx-{tag}-{pid}-{counter}.sock")
}

/// Test writer that blocks the first write until its control handle releases it.
///
/// This is a test-support boundary for Tenex tests, not a stable public API.
#[derive(Debug)]
pub struct BlockingWriter {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    blocked: bool,
}

impl BlockingWriter {
    /// Create a blocking writer and its control handle.
    ///
    /// This is a test-support boundary for Tenex tests, not a stable public API.
    #[must_use]
    pub fn new() -> (Self, BlockingWriterHandle) {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        (
            Self {
                entered: entered_tx,
                release: release_rx,
                blocked: false,
            },
            BlockingWriterHandle {
                entered: entered_rx,
                release: release_tx,
            },
        )
    }
}

impl io::Write for BlockingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.blocked {
            self.blocked = true;
            let _ = self.entered.send(());
            let _ = self.release.recv();
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Control handle for a `BlockingWriter`.
///
/// This is a test-support boundary for Tenex tests, not a stable public API.
#[derive(Debug)]
pub struct BlockingWriterHandle {
    entered: mpsc::Receiver<()>,
    release: mpsc::SyncSender<()>,
}

impl BlockingWriterHandle {
    /// Wait until the writer has entered its blocking write.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer does not enter the blocked write before the timeout.
    pub fn wait_until_blocked(&self, timeout: Duration) -> Result<(), mpsc::RecvTimeoutError> {
        self.entered.recv_timeout(timeout)
    }

    /// Release the writer so its first write can complete.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer has already gone away.
    pub fn release(&self) -> Result<(), mpsc::SendError<()>> {
        self.release.send(())
    }
}

/// Split a command line into argv using Tenex's internal command parser.
///
/// This is a test-support boundary for Tenex tests, not a stable public API.
///
/// # Errors
///
/// Returns an error when the command line is empty or cannot be parsed.
pub fn parse_command_line(command_line: &str) -> anyhow::Result<Vec<String>> {
    crate::command::parse_command_line(command_line)
}
