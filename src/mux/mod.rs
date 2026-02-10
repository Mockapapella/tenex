//! Cross-platform multiplexer integration module.

mod backend;
mod capture;
mod client;
mod daemon;
mod discovery;
mod endpoint;
mod ipc;
mod output;
mod protocol;
pub(crate) mod render;
mod server;
mod session;

pub use capture::Capture as OutputCapture;
pub use endpoint::set_socket_override;
pub use output::{OutputRead, OutputStream};
pub use session::{Manager as SessionManager, Session, Window};

use anyhow::{Context, Result, bail};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
use std::process::Command;
use std::time::{Duration, Instant};

/// Check if the mux backend is available on the system.
#[must_use]
pub const fn is_available() -> bool {
    true
}

/// Check if the mux daemon is currently running.
#[must_use]
pub fn is_server_running() -> bool {
    let Ok(endpoint) = endpoint::socket_endpoint() else {
        return false;
    };

    match interprocess::local_socket::Stream::connect(endpoint.name) {
        Ok(mut stream) => {
            if ipc::write_json(&mut stream, &protocol::MuxRequest::Ping).is_err() {
                return false;
            }
            ipc::read_json::<_, protocol::MuxResponse>(&mut stream).is_ok()
        }
        Err(_) => false,
    }
}

/// Try to query the version string of the currently running mux daemon.
///
/// This function does **not** start the daemon. If no daemon is listening on the current
/// socket endpoint, it returns `Ok(None)`.
///
/// # Errors
///
/// Returns an error if the mux endpoint cannot be resolved or the daemon responds with an error.
pub fn running_daemon_version() -> Result<Option<String>> {
    let endpoint = client::endpoint()?;
    let Ok(mut stream) = Stream::connect(endpoint.name.clone()) else {
        return Ok(None);
    };

    if ipc::write_json(&mut stream, &protocol::MuxRequest::Ping).is_err() {
        return Ok(None);
    }

    let Ok(response) = ipc::read_json::<_, protocol::MuxResponse>(&mut stream) else {
        return Ok(None);
    };

    match response {
        protocol::MuxResponse::Pong { version } => Ok(Some(version)),
        protocol::MuxResponse::Err { message } => Err(anyhow::anyhow!(message)),
        other => Err(anyhow::anyhow!("Unexpected mux response: {other:?}")),
    }
}

pub(crate) fn terminate_mux_daemon_for_socket(socket: &str) -> Result<()> {
    fn pid_is_alive(pid: u32) -> bool {
        let proc_dir = format!("/proc/{pid}");
        let Ok(stat) = std::fs::read_to_string(format!("{proc_dir}/stat")) else {
            return std::fs::metadata(proc_dir).is_ok();
        };

        let Some(idx) = stat.rfind(") ") else {
            return true;
        };
        !matches!(stat.as_bytes().get(idx.saturating_add(2)), Some(b'Z'))
    }

    fn send_signal(pid: u32, signal: &str) -> Result<()> {
        let status = Command::new("kill")
            .arg(signal)
            .arg(pid.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("Failed to invoke kill {signal} {pid}"))?;

        if status.success() || !pid_is_alive(pid) {
            return Ok(());
        }

        bail!("kill {signal} {pid} failed");
    }

    let socket = socket.trim();
    if socket.is_empty() {
        bail!("Mux socket cannot be empty");
    }

    let pids = discovery::mux_daemon_pids_for_socket(socket);
    if pids.is_empty() {
        return Ok(());
    }

    for pid in &pids {
        let _ = send_signal(*pid, "-TERM");
    }

    let deadline = Instant::now() + Duration::from_millis(500);
    let mut remaining: Vec<u32> = pids;
    while Instant::now() < deadline {
        remaining.retain(|pid| pid_is_alive(*pid));
        if remaining.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    if remaining.is_empty() {
        return Ok(());
    }

    for pid in &remaining {
        let _ = send_signal(*pid, "-KILL");
    }

    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        remaining.retain(|pid| pid_is_alive(*pid));
        if remaining.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    if remaining.is_empty() {
        return Ok(());
    }

    bail!("Failed to terminate mux daemon (pids: {remaining:?})");
}

/// Get the mux daemon version string.
///
/// # Errors
///
/// Returns an error if the version cannot be constructed.
pub fn version() -> Result<String> {
    // Bump this when Tenex makes incompatible changes to mux IPC payloads.
    const MUX_PROTOCOL_VERSION: u32 = 2;

    Ok(format!(
        "tenex-mux/{}/proto-{}",
        env!("CARGO_PKG_VERSION"),
        MUX_PROTOCOL_VERSION
    ))
}

/// Get the mux daemon socket name/path Tenex will use by default for this process.
///
/// # Errors
///
/// Returns an error if the socket endpoint cannot be constructed.
pub fn socket_display() -> Result<String> {
    Ok(endpoint::socket_endpoint()?.display)
}

/// Attempt to discover a running mux daemon socket that contains one of the provided session names.
///
/// This is primarily used to keep agents alive across upgrades/rebuilds when the default socket
/// fingerprint changes.
#[must_use]
pub fn discover_socket_for_sessions<S: std::hash::BuildHasher>(
    wanted_sessions: &std::collections::HashSet<String, S>,
    preferred_socket: Option<&str>,
) -> Option<String> {
    discovery::discover_socket_for_sessions(wanted_sessions, preferred_socket)
}

/// Run the mux daemon in the foreground.
///
/// This is intended to be invoked by the `tenex muxd` CLI subcommand.
///
/// # Errors
///
/// Returns an error if the daemon fails to start.
pub fn run_mux_daemon() -> Result<()> {
    let endpoint = endpoint::socket_endpoint()?;
    daemon::run(&endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::process::Command;

    #[test]
    fn test_is_available() {
        assert!(is_available());
    }

    #[test]
    fn test_version() -> Result<(), Box<dyn std::error::Error>> {
        let version = version()?;
        assert!(version.starts_with("tenex-mux/"));
        Ok(())
    }

    #[test]
    fn test_is_server_running_false_with_override() {
        let name = format!("tenex-mux-test-{}", std::process::id());
        if let Err(err) = set_socket_override(&name) {
            assert!(
                err.to_string().contains("already set"),
                "Unexpected set_socket_override error: {err}"
            );
        }
        assert!(!is_server_running());
    }

    #[test]
    fn test_socket_display() -> Result<(), Box<dyn std::error::Error>> {
        let display = socket_display()?;
        assert!(!display.trim().is_empty());
        Ok(())
    }

    #[test]
    fn test_terminate_mux_daemon_for_socket_rejects_empty_socket()
    -> Result<(), Box<dyn std::error::Error>> {
        match terminate_mux_daemon_for_socket("   ") {
            Ok(()) => Err("Expected empty socket to error".into()),
            Err(err) => {
                assert!(err.to_string().contains("Mux socket cannot be empty"));
                Ok(())
            }
        }
    }

    #[test]
    fn test_terminate_mux_daemon_for_socket_noops_when_no_matches()
    -> Result<(), Box<dyn std::error::Error>> {
        let socket = format!("tenex-mux-test-no-matches-{}", uuid::Uuid::new_v4());
        terminate_mux_daemon_for_socket(&socket)?;
        Ok(())
    }

    #[test]
    fn test_terminate_mux_daemon_for_socket_kills_matching_process()
    -> Result<(), Box<dyn std::error::Error>> {
        let socket = format!("tenex-mux-test-socket-{}", uuid::Uuid::new_v4());
        let mut child = Command::new("bash")
            .arg("-c")
            .arg("exec -a muxd sleep 60")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TENEX_MUX_SOCKET", &socket)
            .spawn()?;
        let pid = child.id();

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if discovery::mux_daemon_pids_for_socket(&socket).contains(&pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        terminate_mux_daemon_for_socket(&socket)?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = child.kill();
        let _ = child.wait();
        Err("Expected spawned muxd process to terminate".into())
    }

    #[test]
    fn test_terminate_mux_daemon_for_socket_escalates_to_kill_when_ignoring_term()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::process::ExitStatusExt;

        let socket = format!("tenex-mux-test-socket-{}", uuid::Uuid::new_v4());
        let ready_path =
            std::env::temp_dir().join(format!("tenex-mux-test-ready-{}", uuid::Uuid::new_v4()));
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(
                r#"
import os
import signal
import time

signal.signal(signal.SIGTERM, signal.SIG_IGN)
ready_path = os.environ.get("TENEX_TEST_READY_PATH")
if ready_path:
    with open(ready_path, "w", encoding="utf-8") as f:
        f.write("ready")
time.sleep(60)
"#,
            )
            .arg("muxd")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TENEX_MUX_SOCKET", &socket)
            .env("TENEX_TEST_READY_PATH", &ready_path)
            .spawn()?;
        let pid = child.id();

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if ready_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !ready_path.exists() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Expected spawned muxd process to signal readiness".into());
        }

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut found = false;
        while Instant::now() < deadline {
            if discovery::mux_daemon_pids_for_socket(&socket).contains(&pid) {
                found = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !found {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Expected spawned muxd process to be discoverable".into());
        }

        terminate_mux_daemon_for_socket(&socket)?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait()? {
                if status.signal() == Some(9) {
                    return Ok(());
                }

                return Err(format!(
                    "Expected spawned muxd process to exit with SIGKILL, got status {status:?}"
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = child.kill();
        let _ = child.wait();
        Err("Expected spawned muxd process to terminate after SIGKILL".into())
    }

    #[test]
    fn test_running_daemon_version_returns_none_when_not_running()
    -> Result<(), Box<dyn std::error::Error>> {
        // Ensure the daemon is actually running so we exercise the termination path.
        // (If it's already stopped, terminate_mux_daemon_for_socket is a no-op.)
        let _ = SessionManager::new().exists("tenex-version-probe-nonexistent");

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if running_daemon_version()?.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        let endpoint = client::endpoint()?;
        terminate_mux_daemon_for_socket(&endpoint.display)?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if running_daemon_version()?.is_none() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(25));
        }

        Err("Expected mux daemon to be stopped".into())
    }

    #[test]
    fn test_running_daemon_version_returns_some_when_running()
    -> Result<(), Box<dyn std::error::Error>> {
        // Starting a mux request should boot the daemon if it isn't running yet.
        let _ = SessionManager::new().exists("tenex-version-probe-nonexistent");

        let deadline = Instant::now() + Duration::from_secs(5);
        let version = loop {
            match running_daemon_version()? {
                Some(version) => break version,
                None if Instant::now() >= deadline => {
                    return Err("Expected running daemon version to be available".into());
                }
                None => std::thread::sleep(Duration::from_millis(25)),
            }
        };

        assert!(version.starts_with("tenex-mux/"));
        Ok(())
    }
}
