//! Cross-platform multiplexer integration module.

mod backend;
mod capture;
mod client;
mod daemon;
mod discovery;
mod endpoint;
mod ipc;
mod output;
#[cfg(not(target_os = "linux"))]
mod pidfile;
mod protocol;
pub(crate) mod render;
mod server;
mod session;

pub use capture::Capture as OutputCapture;
pub use endpoint::set_socket_override;
pub use output::{OutputCursor, OutputRead, OutputStream};
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
    fn send_signal(pid: u32, signal: &str) -> Result<()> {
        let status = {
            #[cfg(windows)]
            {
                let mut command = Command::new("taskkill");
                if signal == "-KILL" {
                    command.arg("/F");
                }

                command
                    .arg("/PID")
                    .arg(pid.to_string())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .with_context(|| format!("Failed to invoke taskkill for pid {pid}"))?
            }

            #[cfg(not(windows))]
            {
                Command::new("kill")
                    .arg(signal)
                    .arg(pid.to_string())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .with_context(|| format!("Failed to invoke kill {signal} {pid}"))?
            }
        };

        if status.success() || !discovery::pid_is_alive(pid) {
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
        remaining.retain(|pid| discovery::pid_is_alive(*pid));
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
        remaining.retain(|pid| discovery::pid_is_alive(*pid));
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
    const MUX_PROTOCOL_VERSION: u32 = 3;

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

    #[cfg(unix)]
    use std::io::{BufRead, BufReader};

    #[cfg(unix)]
    use std::process::{Child, Command, Stdio};

    #[cfg(unix)]
    fn spawn_test_muxd(socket: &str, script: &str) -> Result<Child, Box<dyn std::error::Error>> {
        Ok(Command::new("bash")
            .arg("-c")
            .arg("exec -a \"$0\" python3 -c \"$1\"")
            .arg("muxd")
            .arg(script)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TENEX_MUX_SOCKET", socket)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?)
    }

    #[cfg(unix)]
    fn wait_for_ready(child: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::other("Expected spawned muxd process to expose stdout")
        })?;
        let mut ready = String::new();
        BufReader::new(stdout).read_line(&mut ready)?;

        if ready.trim_end() == "ready" {
            return Ok(());
        }

        Err(format!("Expected spawned muxd process to signal readiness, got {ready:?}").into())
    }

    #[cfg(unix)]
    fn cleanup_child(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

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
    fn test_discover_socket_for_sessions_returns_none_for_empty_set() {
        let wanted = std::collections::HashSet::<String>::new();
        assert!(discover_socket_for_sessions(&wanted, None).is_none());
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

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_kills_matching_process()
    -> Result<(), Box<dyn std::error::Error>> {
        let socket = format!("tenex-mux-test-socket-{}", uuid::Uuid::new_v4());
        let mut child = spawn_test_muxd(
            &socket,
            r#"
import signal
import sys
import time

def on_term(_signum, _frame):
    time.sleep(0.05)
    raise SystemExit(0)

signal.signal(signal.SIGTERM, on_term)
sys.stdout.write("ready\n")
sys.stdout.flush()

while True:
    time.sleep(60)
"#,
        )?;
        let pid = child.id();
        #[cfg(not(target_os = "linux"))]
        let _pid_guard = super::pidfile::PidFileGuard::create_for_pid(&socket, pid)?;

        if let Err(err) = wait_for_ready(&mut child) {
            cleanup_child(&mut child);
            return Err(err);
        }
        if !discovery::mux_daemon_pids_for_socket(&socket).contains(&pid) {
            cleanup_child(&mut child);
            return Err("Expected spawned muxd process to be discoverable".into());
        }

        terminate_mux_daemon_for_socket(&socket)?;
        let status = child.wait()?;
        if status.success() {
            return Ok(());
        }

        Err(format!(
            "Expected spawned muxd process to terminate successfully, got status {status:?}"
        )
        .into())
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_escalates_to_kill_when_ignoring_term()
    -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;

        let socket = format!("tenex-mux-test-socket-{}", uuid::Uuid::new_v4());
        let mut child = spawn_test_muxd(
            &socket,
            r#"
import signal
import sys
import time

signal.signal(signal.SIGTERM, signal.SIG_IGN)
sys.stdout.write("ready\n")
sys.stdout.flush()

while True:
    time.sleep(60)
"#,
        )?;
        let pid = child.id();
        #[cfg(not(target_os = "linux"))]
        let _pid_guard = super::pidfile::PidFileGuard::create_for_pid(&socket, pid)?;

        if let Err(err) = wait_for_ready(&mut child) {
            cleanup_child(&mut child);
            return Err(err);
        }
        if !discovery::mux_daemon_pids_for_socket(&socket).contains(&pid) {
            cleanup_child(&mut child);
            return Err("Expected spawned muxd process to be discoverable".into());
        }

        terminate_mux_daemon_for_socket(&socket)?;
        let status = child.wait()?;
        if status.signal() == Some(9) {
            return Ok(());
        }

        Err(
            format!("Expected spawned muxd process to exit with SIGKILL, got status {status:?}")
                .into(),
        )
    }

    #[test]
    fn test_running_daemon_version_returns_none_when_not_running()
    -> Result<(), Box<dyn std::error::Error>> {
        let _guard = crate::test_support::lock_mux_test_environment();

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
        let _guard = crate::test_support::lock_mux_test_environment();

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
