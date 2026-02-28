//! Mux daemon socket discovery helpers.
//!
//! Tenex can run multiple mux daemons (for example across rebuilds/upgrades when the default
//! socket fingerprint changes). This module helps locate the daemon that owns a set of stored
//! sessions so agents can survive restarts.

use super::endpoint::socket_endpoint_from_value;
use super::ipc;
#[cfg(not(target_os = "linux"))]
use super::pidfile;
use super::protocol::{MuxRequest, MuxResponse};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::path::Path;

/// Attempt to find a running mux daemon socket that contains at least one of the requested
/// session names.
///
/// `preferred_socket` is checked first when provided.
#[must_use]
pub fn discover_socket_for_sessions<S: std::hash::BuildHasher>(
    wanted_sessions: &HashSet<String, S>,
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

    if let Ok(default_socket) = super::socket_display() {
        candidates.push(default_socket);
    }

    candidates.extend(running_mux_sockets());

    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));

    let mut best: Option<(usize, String)> = None;
    for candidate in candidates {
        let Some(matches) = probe_session_matches(&candidate, wanted_sessions) else {
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
    #[cfg(target_os = "linux")]
    {
        let proc_dir = format!("/proc/{pid}");
        let Ok(stat) = std::fs::read_to_string(format!("{proc_dir}/stat")) else {
            return std::fs::metadata(proc_dir).is_ok();
        };

        let Some(idx) = stat.rfind(") ") else {
            return true;
        };
        !matches!(stat.as_bytes().get(idx.saturating_add(2)), Some(b'Z'))
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
            .is_ok_and(|status| status.success())
    }
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
    #[cfg(test)]
    let want_path_sockets = super::socket_display()
        .ok()
        .is_some_and(|display| display.contains('/') || display.contains('\\'));
    #[cfg(test)]
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
        if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
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

        #[cfg(test)]
        if let Some(wanted_state_path) = wanted_state_path.as_deref()
            && parsed.get("TENEX_STATE_PATH").map(|value| value.trim()) != Some(wanted_state_path)
        {
            continue;
        }

        if let Some(value) = parsed.get("TENEX_MUX_SOCKET") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                #[cfg(test)]
                {
                    let is_path = trimmed.contains('/') || trimmed.contains('\\');
                    if is_path != want_path_sockets {
                        continue;
                    }
                }
                let _ = sockets.insert(trimmed.to_string());
            }
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
        let Some(key) = parts.next() else {
            continue;
        };
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
mod tests {
    use super::*;
    use anyhow::Result;
    use tempfile::TempDir;

    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::time::{Duration, Instant};

    #[cfg(target_os = "linux")]
    #[test]
    fn test_cmdline_contains_muxd() {
        assert!(cmdline_contains_muxd(b"tenex\0muxd\0"));
        assert!(!cmdline_contains_muxd(b"tenex\0\0"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_environ() {
        let env = b"A=1\0TENEX_MUX_SOCKET=test\0B=2\0";
        let parsed = parse_environ(env);
        assert_eq!(parsed.get("A"), Some(&"1".to_string()));
        assert_eq!(parsed.get("TENEX_MUX_SOCKET"), Some(&"test".to_string()));
        assert_eq!(parsed.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn test_discover_socket_for_sessions_returns_none_for_empty_input() {
        let wanted_sessions: HashSet<String> = HashSet::new();
        assert!(discover_socket_for_sessions(&wanted_sessions, None).is_none());
    }

    #[test]
    fn test_discover_socket_for_sessions_finds_matching_session() -> Result<()> {
        let session_manager = crate::mux::SessionManager::new();
        let session_name = format!("tenex-test-discovery-{}", uuid::Uuid::new_v4());
        let workdir = TempDir::new()?;
        session_manager.create(&session_name, workdir.path(), None)?;

        let socket = crate::mux::socket_display()?;

        let mut wanted_sessions = HashSet::new();
        wanted_sessions.insert(session_name.clone());

        let discovered = discover_socket_for_sessions(&wanted_sessions, Some(&socket));

        let _ = session_manager.kill(&session_name);

        anyhow::ensure!(
            discovered.as_deref() == Some(socket.as_str()),
            "Expected to rediscover mux socket"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_discover_socket_for_sessions_returns_none_when_no_matches() -> Result<()> {
        let session_manager = crate::mux::SessionManager::new();
        let existing_session = format!("tenex-test-discovery-existing-{}", uuid::Uuid::new_v4());
        let workdir = TempDir::new()?;
        session_manager.create(&existing_session, workdir.path(), None)?;

        let socket = crate::mux::socket_display()?;
        let want_path_sockets = socket.contains('/') || socket.contains('\\');
        let state_path = std::env::var("TENEX_STATE_PATH").unwrap_or_else(|_| {
            crate::config::Config::state_path()
                .to_string_lossy()
                .into_owned()
        });

        // Spawn a dummy "muxd" process that advertises a socket but isn't actually listening.
        // This ensures discovery exercises the probe failure path deterministically.
        let dummy_socket = if want_path_sockets {
            std::env::temp_dir()
                .join(format!(
                    "tenex-mux-test-unreachable-{}.sock",
                    uuid::Uuid::new_v4()
                ))
                .to_string_lossy()
                .into_owned()
        } else {
            format!("tenex-mux-test-unreachable-{}", uuid::Uuid::new_v4())
        };
        let mut dummy = Command::new("bash")
            .arg("-c")
            .arg("exec -a muxd sleep 60")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TENEX_MUX_SOCKET", &dummy_socket)
            .env("TENEX_STATE_PATH", &state_path)
            .spawn()?;
        let dummy_pid = dummy.id();
        #[cfg(not(target_os = "linux"))]
        let _dummy_pid_guard =
            super::pidfile::PidFileGuard::create_for_pid(&dummy_socket, dummy_pid)?;

        let mut found_dummy = false;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if mux_daemon_pids_for_socket(&dummy_socket).contains(&dummy_pid) {
                found_dummy = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !found_dummy {
            let _ = dummy.kill();
            let _ = dummy.wait();
            return Err(anyhow::anyhow!(
                "Expected dummy muxd process to be discoverable"
            ));
        }

        let mut wanted_sessions = HashSet::new();
        wanted_sessions.insert(format!(
            "tenex-test-discovery-missing-{}",
            uuid::Uuid::new_v4()
        ));

        let discovered = discover_socket_for_sessions(&wanted_sessions, Some(&socket));
        let _ = session_manager.kill(&existing_session);
        let _ = dummy.kill();
        let _ = dummy.wait();
        assert!(discovered.is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_mux_daemon_pids_for_socket_finds_process() -> Result<(), Box<dyn std::error::Error>> {
        let socket = format!("tenex-mux-test-socket-{}", uuid::Uuid::new_v4());
        let state_path = std::env::var("TENEX_STATE_PATH").unwrap_or_else(|_| {
            crate::config::Config::state_path()
                .to_string_lossy()
                .into_owned()
        });
        let mut child = Command::new("bash")
            .arg("-c")
            .arg("exec -a muxd sleep 60")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TENEX_MUX_SOCKET", &socket)
            .env("TENEX_STATE_PATH", &state_path)
            .spawn()?;
        let pid = child.id();
        #[cfg(not(target_os = "linux"))]
        let _pid_guard = super::pidfile::PidFileGuard::create_for_pid(&socket, pid)?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if mux_daemon_pids_for_socket(&socket).contains(&pid) {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = child.kill();
        let _ = child.wait();
        Err("Expected to discover spawned muxd process".into())
    }

    #[test]
    fn test_mux_daemon_pids_for_socket_returns_empty_for_empty_socket() {
        assert!(mux_daemon_pids_for_socket("").is_empty());
        assert!(mux_daemon_pids_for_socket("   ").is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_mux_daemon_pids_for_socket_handles_missing_cmdline_files() -> Result<()> {
        let proc_root = TempDir::new()?;

        // Missing cmdline file should be skipped (covers cmdline read failure path).
        std::fs::create_dir_all(proc_root.path().join("1000"))?;

        let wanted_socket = "tenex-test-socket";
        let pid_dir = proc_root.path().join("2000");
        std::fs::create_dir_all(&pid_dir)?;
        std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0")?;
        std::fs::write(
            pid_dir.join("environ"),
            format!("TENEX_MUX_SOCKET={wanted_socket}\0").as_bytes(),
        )?;

        let pids = mux_daemon_pids_for_socket_in_proc_root(proc_root.path(), wanted_socket);
        assert_eq!(pids, vec![2000]);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_running_mux_sockets_handles_missing_cmdline_files() -> Result<()> {
        let proc_root = TempDir::new()?;
        std::fs::create_dir_all(proc_root.path().join("1000"))?;

        let want_path_sockets = crate::mux::socket_display()
            .ok()
            .is_some_and(|display| display.contains('/') || display.contains('\\'));

        let socket_good = if want_path_sockets {
            proc_root
                .path()
                .join("good.sock")
                .to_string_lossy()
                .into_owned()
        } else {
            "tenex-mux-test-good".to_string()
        };

        let pid_dir = proc_root.path().join("2000");
        std::fs::create_dir_all(&pid_dir)?;
        std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0")?;

        let mut environ = format!("TENEX_MUX_SOCKET={socket_good}\0");
        if let Some(wanted_state_path) = std::env::var("TENEX_STATE_PATH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            environ.push_str("TENEX_STATE_PATH=");
            environ.push_str(&wanted_state_path);
            environ.push('\0');
        }
        std::fs::write(pid_dir.join("environ"), environ.as_bytes())?;

        let sockets = running_mux_sockets_in_proc_root(proc_root.path());
        assert!(sockets.contains(&socket_good));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_running_mux_sockets_filters_mismatched_state_path() -> Result<()> {
        let Some(wanted_state_path) = std::env::var("TENEX_STATE_PATH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            // The state-path filter only applies when TENEX_STATE_PATH is set.
            return Ok(());
        };

        let want_path_sockets = crate::mux::socket_display()
            .ok()
            .is_some_and(|display| display.contains('/') || display.contains('\\'));

        let proc_root = TempDir::new()?;
        let socket_good = if want_path_sockets {
            proc_root
                .path()
                .join("good.sock")
                .to_string_lossy()
                .into_owned()
        } else {
            "tenex-mux-test-good".to_string()
        };
        let socket_bad = if want_path_sockets {
            proc_root
                .path()
                .join("bad.sock")
                .to_string_lossy()
                .into_owned()
        } else {
            "tenex-mux-test-bad".to_string()
        };

        let good_pid = proc_root.path().join("1000");
        std::fs::create_dir_all(&good_pid)?;
        std::fs::write(good_pid.join("cmdline"), b"tenex\0muxd\0")?;
        std::fs::write(
            good_pid.join("environ"),
            format!("TENEX_MUX_SOCKET={socket_good}\0TENEX_STATE_PATH={wanted_state_path}\0")
                .as_bytes(),
        )?;

        let bad_pid = proc_root.path().join("2000");
        std::fs::create_dir_all(&bad_pid)?;
        std::fs::write(bad_pid.join("cmdline"), b"tenex\0muxd\0")?;
        std::fs::write(
            bad_pid.join("environ"),
            format!(
                "TENEX_MUX_SOCKET={socket_bad}\0TENEX_STATE_PATH={wanted_state_path}.mismatch\0"
            )
            .as_bytes(),
        )?;

        let sockets = running_mux_sockets_in_proc_root(proc_root.path());
        assert!(sockets.contains(&socket_good));
        assert!(!sockets.contains(&socket_bad));
        Ok(())
    }
}
