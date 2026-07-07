//! Feature-gated test support utilities.
//!
//! These helpers are exposed for Tenex's internal tests and external test
//! crates that enable `test-support`. They are not stable public API and carry
//! no backwards-compatibility promise.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

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
