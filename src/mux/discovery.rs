//! Mux daemon socket discovery helpers.
//!
//! Tenex can run multiple mux daemons (for example across rebuilds/upgrades when the default
//! socket fingerprint changes). This module helps locate the daemon that owns a set of stored
//! sessions so agents can survive restarts.

#![cfg_attr(all(coverage, not(test)), allow(dead_code))]

use super::endpoint::socket_endpoint_from_value;
use super::ipc;
#[cfg(not(target_os = "linux"))]
use super::pidfile;
use super::protocol::{MuxRequest, MuxResponse};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
#[cfg(any(test, coverage))]
use std::cell::{Cell, RefCell};
#[cfg(any(test, coverage, target_os = "linux"))]
use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(any(test, coverage))]
use std::time::Duration;

#[cfg(any(test, coverage))]
thread_local! {
    static TEST_PIDS_FOR_SOCKET: RefCell<HashMap<String, Vec<u32>>> = RefCell::new(HashMap::new());
    static TEST_PID_IS_ALIVE: RefCell<HashMap<u32, bool>> = RefCell::new(HashMap::new());
    static TEST_PROBE_DELAY_BEFORE_WRITE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(any(test, coverage))]
pub(super) fn set_test_mux_daemon_pids_for_socket(socket: &str, pids: Vec<u32>) {
    TEST_PIDS_FOR_SOCKET.with(|values| {
        values.borrow_mut().insert(socket.to_string(), pids);
    });
}

#[cfg(any(test, coverage))]
pub(super) fn set_test_pid_is_alive(pid: u32, alive: bool) {
    TEST_PID_IS_ALIVE.with(|values| {
        values.borrow_mut().insert(pid, alive);
    });
}

#[cfg(any(test, coverage))]
pub(super) fn clear_test_discovery_overrides() {
    TEST_PIDS_FOR_SOCKET.with(|values| values.borrow_mut().clear());
    TEST_PID_IS_ALIVE.with(|values| values.borrow_mut().clear());
    TEST_PROBE_DELAY_BEFORE_WRITE.with(|cell| cell.set(false));
}

#[cfg(any(test, coverage))]
fn test_mux_daemon_pids_for_socket_override(socket: &str) -> Option<Vec<u32>> {
    TEST_PIDS_FOR_SOCKET.with(|values| values.borrow().get(socket).cloned())
}

#[cfg(any(test, coverage))]
fn test_pid_is_alive_override(pid: u32) -> Option<bool> {
    TEST_PID_IS_ALIVE.with(|values| values.borrow().get(&pid).copied())
}

#[cfg(any(test, coverage))]
struct TestProbeDelayBeforeWriteGuard {
    previous: bool,
}

#[cfg(all(any(test, coverage), target_os = "linux"))]
#[cfg_attr(coverage_nightly, coverage(off))]
fn socket_display_is_path(display: &str) -> bool {
    display.find(&['/', '\\'][..]).is_some()
}

#[cfg(any(test, coverage))]
impl Drop for TestProbeDelayBeforeWriteGuard {
    fn drop(&mut self) {
        TEST_PROBE_DELAY_BEFORE_WRITE.with(|cell| cell.set(self.previous));
    }
}

#[cfg(any(test, coverage))]
fn with_test_probe_delay_before_write<T>(f: impl FnOnce() -> T) -> T {
    let previous = TEST_PROBE_DELAY_BEFORE_WRITE.with(|cell| cell.replace(true));
    let _guard = TestProbeDelayBeforeWriteGuard { previous };
    f()
}

/// Attempt to find a running mux daemon socket that contains at least one of the requested
/// session names.
///
/// `preferred_socket` is checked first when provided.
#[must_use]
pub fn discover_socket_for_sessions(
    wanted_sessions: &HashSet<String, impl std::hash::BuildHasher>,
    preferred_socket: Option<&str>,
) -> Option<String> {
    if wanted_sessions.is_empty() {
        return None;
    }

    let mut candidates = Vec::new();
    if let Some(socket) = preferred_socket
        .map(str::trim)
        .filter(|socket| !socket.is_empty())
    {
        candidates.push(socket.to_string());
    }

    candidates.extend(super::socket_display().ok());

    candidates.extend(running_mux_sockets());

    discover_socket_for_session_candidates(wanted_sessions, candidates, probe_session_matches)
}

fn discover_socket_for_session_candidates<S, F>(
    wanted_sessions: &HashSet<String, S>,
    mut candidates: Vec<String>,
    mut probe: F,
) -> Option<String>
where
    S: std::hash::BuildHasher,
    F: FnMut(&str, &HashSet<String, S>) -> Option<usize>,
{
    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));

    let mut best: Option<(usize, String)> = None;
    for candidate in candidates {
        let Some(matches) = probe(&candidate, wanted_sessions) else {
            continue;
        };
        if matches == 0 {
            continue;
        }

        match &best {
            None => best = Some((matches, candidate)),
            Some((best_matches, _)) if matches > *best_matches => {
                best = Some((matches, candidate));
            }
            _ => {}
        }
    }

    best.map(|(_, socket)| socket)
}

pub(super) fn mux_daemon_pids_for_socket(socket: &str) -> Vec<u32> {
    #[cfg(any(test, coverage))]
    if let Some(pids) = test_mux_daemon_pids_for_socket_override(socket) {
        return pids;
    }

    #[cfg(target_os = "linux")]
    {
        mux_daemon_pids_for_socket_in_proc_root(Path::new("/proc"), socket)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let Some(pid) = pidfile::read_pid(socket) else {
            return Vec::new();
        };

        if pid_is_alive(pid) {
            return vec![pid];
        }

        pidfile::remove(socket);
        Vec::new()
    }
}

pub(super) fn pid_is_alive(pid: u32) -> bool {
    #[cfg(any(test, coverage))]
    if let Some(alive) = test_pid_is_alive_override(pid) {
        return alive;
    }

    #[cfg(target_os = "linux")]
    {
        pid_is_alive_in_proc_root(Path::new("/proc"), pid)
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        let Ok(output) = output else {
            return false;
        };
        if !output.status.success() {
            return false;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.contains("No tasks are running") {
            return false;
        }

        stdout.contains(&format!("\"{pid}\""))
    }

    #[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
    {
        use std::process::Command;

        let ps = Command::new("ps")
            .args(["-o", "stat=", "-p"])
            .arg(pid.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output();

        if let Ok(output) = ps {
            if !output.status.success() {
                return false;
            }

            let stat = String::from_utf8_lossy(&output.stdout);
            let state = stat.trim();
            if state.is_empty() {
                return false;
            }

            return !state.contains('Z');
        }

        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .as_ref()
            .is_ok_and(std::process::ExitStatus::success)
    }
}

#[cfg(target_os = "linux")]
fn pid_is_alive_in_proc_root(proc_root: &Path, pid: u32) -> bool {
    let proc_dir = proc_root.join(pid.to_string());
    let stat_path = proc_dir.join("stat");
    let Ok(stat) = std::fs::read_to_string(stat_path) else {
        return std::fs::metadata(proc_dir).is_ok();
    };

    let Some(idx) = stat.rfind(") ") else {
        return true;
    };
    !matches!(stat.as_bytes().get(idx.saturating_add(2)), Some(b'Z'))
}

#[cfg(target_os = "linux")]
fn mux_daemon_pids_for_socket_in_proc_root(proc_root: &Path, socket: &str) -> Vec<u32> {
    let wanted_socket = socket.trim();
    if wanted_socket.is_empty() {
        return Vec::new();
    }

    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid_str) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };

        let base = entry.path();
        let Ok(cmdline) = std::fs::read(base.join("cmdline")) else {
            continue;
        };
        if !cmdline_contains_muxd(&cmdline) {
            continue;
        }

        let Ok(environ) = std::fs::read(base.join("environ")) else {
            continue;
        };

        let parsed = parse_environ(&environ);
        let Some(value) = parsed.get("TENEX_MUX_SOCKET") else {
            continue;
        };
        if value.trim() == wanted_socket {
            pids.push(pid);
        }
    }

    pids
}

fn probe_session_matches<S: std::hash::BuildHasher>(
    socket: &str,
    wanted_sessions: &HashSet<String, S>,
) -> Option<usize> {
    let endpoint = socket_endpoint_from_value(socket).ok()?;
    let mut stream = Stream::connect(endpoint.name).ok()?;

    #[cfg(any(test, coverage))]
    if TEST_PROBE_DELAY_BEFORE_WRITE.with(Cell::get) {
        std::thread::sleep(Duration::from_millis(30));
    }

    ipc::write_json(&mut stream, &MuxRequest::ListSessions).ok()?;
    let response: MuxResponse = ipc::read_json(&mut stream).ok()?;

    let MuxResponse::Sessions { sessions } = response else {
        return None;
    };

    Some(
        sessions
            .into_iter()
            .filter(|session| wanted_sessions.contains(&session.name))
            .count(),
    )
}

#[cfg(target_os = "linux")]
fn running_mux_sockets() -> Vec<String> {
    running_mux_sockets_in_proc_root(Path::new("/proc"))
}

#[cfg(target_os = "linux")]
fn running_mux_sockets_in_proc_root(proc_root: &Path) -> Vec<String> {
    let mut sockets = HashSet::new();
    #[cfg(any(test, coverage))]
    let want_path_sockets = super::socket_display()
        .ok()
        .is_some_and(|display| socket_display_is_path(&display));
    #[cfg(any(test, coverage))]
    let wanted_state_path = std::env::var("TENEX_STATE_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str() else {
            continue;
        };
        if pid.parse::<u32>().is_err() {
            continue;
        }

        let base = entry.path();
        let Ok(cmdline) = std::fs::read(base.join("cmdline")) else {
            continue;
        };
        if !cmdline_contains_muxd(&cmdline) {
            continue;
        }

        let Ok(environ) = std::fs::read(base.join("environ")) else {
            continue;
        };

        let parsed = parse_environ(&environ);

        #[cfg(any(test, coverage))]
        if let Some(wanted_state_path) = wanted_state_path.as_deref()
            && parsed.get("TENEX_STATE_PATH").map(|value| value.trim()) != Some(wanted_state_path)
        {
            continue;
        }

        if let Some(value) = parsed.get("TENEX_MUX_SOCKET") {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }

            #[cfg(any(test, coverage))]
            let is_path = socket_display_is_path(trimmed);
            #[cfg(any(test, coverage))]
            if is_path != want_path_sockets {
                continue;
            }

            let _ = sockets.insert(trimmed.to_string());
        }
    }

    sockets.into_iter().collect()
}

#[cfg(not(target_os = "linux"))]
fn running_mux_sockets() -> Vec<String> {
    let mut sockets = Vec::new();

    for socket in pidfile::list_sockets() {
        let Some(pid) = pidfile::read_pid(&socket) else {
            pidfile::remove(&socket);
            continue;
        };

        if pid_is_alive(pid) {
            sockets.push(socket);
        } else {
            pidfile::remove(&socket);
        }
    }

    sockets
}

#[cfg(target_os = "linux")]
fn cmdline_contains_muxd(cmdline: &[u8]) -> bool {
    cmdline
        .split(|b| *b == 0)
        .filter(|arg| !arg.is_empty())
        .any(|arg| arg == b"muxd")
}

#[cfg(target_os = "linux")]
fn parse_environ(environ: &[u8]) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    for entry in environ.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
        let mut parts = entry.splitn(2, |b| *b == b'=');
        let key = parts.next().unwrap_or_default();
        let Some(value) = parts.next() else {
            continue;
        };

        let (Ok(key), Ok(value)) = (std::str::from_utf8(key), std::str::from_utf8(value)) else {
            continue;
        };

        vars.insert(key.to_string(), value.to_string());
    }

    vars
}

#[cfg(test)]
mod tests;
