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
pub use endpoint::{SocketEndpoint, set_socket_override, socket_endpoint};
pub use output::{OutputCursor, OutputRead, OutputStream};
pub use session::{Manager as SessionManager, Session, Window};

#[cfg(any(test, feature = "test-support"))]
pub use ipc::{read_json, write_json};
#[cfg(any(test, feature = "test-support"))]
pub use protocol::{CaptureKind, MuxRequest, MuxResponse};

use anyhow::{Context, Result, bail};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(test)]
thread_local! {
    static TEST_KILL_PROGRAM_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

fn kill_program() -> PathBuf {
    #[cfg(test)]
    if let Some(program) = TEST_KILL_PROGRAM_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return program;
    }

    PathBuf::from("kill")
}

#[cfg(test)]
pub(crate) fn with_kill_program_override_for_tests<T>(
    program: PathBuf,
    f: impl FnOnce() -> T,
) -> T {
    TEST_KILL_PROGRAM_OVERRIDE.with(|slot| {
        let previous = slot.replace(Some(program));
        let result = f();
        slot.replace(previous);
        result
    })
}

fn try_ping(stream: &mut (impl std::io::Read + std::io::Write)) -> Option<protocol::MuxResponse> {
    ipc::write_json(stream, &protocol::MuxRequest::Ping).ok()?;
    ipc::read_json(stream).ok()
}

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

    interprocess::local_socket::Stream::connect(endpoint.name)
        .is_ok_and(|mut stream| try_ping(&mut stream).is_some())
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

    let Some(response) = try_ping(&mut stream) else {
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
                Command::new(kill_program())
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

    let mut signal_failures = Vec::new();
    for pid in &pids {
        if let Err(err) = send_signal(*pid, "-TERM") {
            signal_failures.push(err);
        }
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
        if let Err(err) = send_signal(*pid, "-KILL") {
            signal_failures.push(err);
        }
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

    if signal_failures.is_empty() {
        bail!("Failed to terminate mux daemon (pids: {remaining:?})");
    }

    let failures = signal_failures
        .into_iter()
        .map(|err| err.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    bail!("Failed to terminate mux daemon (pids: {remaining:?}): {failures}");
}

/// Get the mux daemon version string.
#[must_use]
pub fn version() -> String {
    // Bump this when Tenex makes incompatible changes to mux IPC payloads.
    const MUX_PROTOCOL_VERSION: u32 = 3;

    format!(
        "tenex-mux/{}/proto-{}",
        env!("CARGO_PKG_VERSION"),
        MUX_PROTOCOL_VERSION
    )
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

    use interprocess::local_socket::traits::ListenerExt as _;
    use interprocess::local_socket::{ListenerOptions, prelude::*};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[cfg(unix)]
    use std::io::{BufRead, BufReader};

    #[cfg(unix)]
    use std::process::{Child, Command, Stdio};

    enum MockPingMode {
        CloseOnAccept,
        CloseAfterRead,
        Respond(Arc<protocol::MuxResponse>),
    }

    fn make_mock_socket(dir: &TempDir) -> (String, interprocess::local_socket::Name<'static>) {
        #[cfg(windows)]
        {
            use interprocess::local_socket::GenericNamespaced;

            let display = format!("tenex-mux-mod-test-{}", uuid::Uuid::new_v4());
            let name = display
                .clone()
                .to_ns_name::<GenericNamespaced>()
                .expect("Expected namespaced socket name")
                .into_owned();
            return (display, name);
        }

        #[cfg(not(windows))]
        {
            use interprocess::local_socket::GenericFilePath;

            let socket_path = dir.path().join("mux.sock");
            let display = socket_path.to_string_lossy().into_owned();
            let name = socket_path
                .as_path()
                .to_fs_name::<GenericFilePath>()
                .expect("Expected fs socket name to be valid")
                .into_owned();
            (display, name)
        }
    }

    fn spawn_mock_ping_server(
        name: interprocess::local_socket::Name<'static>,
        mode: MockPingMode,
    ) -> std::thread::JoinHandle<()> {
        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("Expected mock mux listener to start");

        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten().take(1) {
                match &mode {
                    MockPingMode::CloseOnAccept => {}
                    MockPingMode::CloseAfterRead => {
                        let _ = crate::mux::read_json::<_, protocol::MuxRequest>(&mut stream);
                    }
                    MockPingMode::Respond(response) => {
                        let _ = crate::mux::read_json::<_, protocol::MuxRequest>(&mut stream);
                        let _ = crate::mux::write_json(&mut stream, &**response);
                    }
                }
            }
        })
    }

    #[cfg(unix)]
    fn spawn_test_muxd(socket: &str, script: &str) -> Child {
        Command::new("bash")
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
            .spawn()
            .expect("Expected muxd child to spawn")
    }

    #[cfg(unix)]
    fn wait_for_ready(child: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
        let stdout = child.stdout.take().ok_or_else(|| {
            std::io::Error::other("Expected spawned muxd process to expose stdout")
        })?;
        let mut ready = String::new();
        BufReader::new(stdout)
            .read_line(&mut ready)
            .expect("Expected muxd readiness line");

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

    #[cfg(unix)]
    fn wait_for_ready_or_cleanup(child: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(err) = wait_for_ready(child) {
            cleanup_child(child);
            return Err(err);
        }
        Ok(())
    }

    #[cfg(unix)]
    fn ensure_discoverable_or_cleanup(
        child: &mut Child,
        socket: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let pid = child.id();
        if discovery::mux_daemon_pids_for_socket(socket).contains(&pid) {
            return Ok(());
        }
        cleanup_child(child);
        Err("Expected spawned muxd process to be discoverable".into())
    }

    #[test]
    fn test_is_available() {
        assert!(is_available());
    }

    #[test]
    fn test_version() {
        let version = version();
        assert!(version.starts_with("tenex-mux/"));
    }

    #[test]
    fn test_is_server_running_false_with_override() {
        let name = format!("tenex-mux-test-{}", std::process::id());
        set_socket_override(&name).expect("Expected socket override to be set");
        assert!(!is_server_running());
    }

    #[test]
    fn test_is_server_running_false_when_socket_endpoint_errors() {
        set_socket_override("\0").expect("Expected override to be set");
        assert!(!is_server_running());
    }

    #[test]
    fn test_is_server_running_true_with_mock_pong() {
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (display, name) = make_mock_socket(&socket_dir);
        let response = Arc::new(protocol::MuxResponse::Pong {
            version: "mock".to_string(),
        });
        let server = spawn_mock_ping_server(name, MockPingMode::Respond(response));

        set_socket_override(&display).expect("Expected socket override to be set");
        assert!(is_server_running());
        server.join().expect("mock server join");
    }

    struct FailingIo;

    impl std::io::Read for FailingIo {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl std::io::Write for FailingIo {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write boom"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flush boom"))
        }
    }

    #[test]
    fn test_try_ping_returns_none_when_write_fails() {
        let mut failing = FailingIo;
        assert!(try_ping(&mut failing).is_none());
    }

    #[test]
    fn test_try_ping_returns_none_when_stream_write_fails() {
        use std::sync::mpsc;
        use std::time::Duration;

        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (_display, name) = make_mock_socket(&socket_dir);

        let (tx, rx) = mpsc::channel::<()>();

        let listener = ListenerOptions::new()
            .name(name.clone())
            .create_sync()
            .expect("Expected mock mux listener to start");

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(1) {
                drop(stream);
                let _ = tx.send(());
            }
        });

        let mut stream = interprocess::local_socket::Stream::connect(name)
            .expect("Expected client stream to connect");
        rx.recv_timeout(Duration::from_secs(1))
            .expect("Expected server to drop stream");

        assert!(ipc::write_json(&mut stream, &protocol::MuxRequest::Ping).is_err());
        assert!(try_ping(&mut stream).is_none());
    }

    #[test]
    fn test_failing_io_helpers_cover_read_and_flush() {
        use std::io::{Read, Write};

        let mut failing = FailingIo;
        let mut buf = [0u8; 1];
        assert_eq!(failing.read(&mut buf).expect("read"), 0);
        failing.flush().expect_err("flush should fail");
    }

    #[test]
    fn test_is_server_running_false_when_write_fails() {
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (display, name) = make_mock_socket(&socket_dir);
        let server = spawn_mock_ping_server(name, MockPingMode::CloseOnAccept);

        set_socket_override(&display).expect("Expected socket override to be set");
        assert!(!is_server_running());
        server.join().expect("mock server join");
    }

    #[test]
    fn test_socket_display() {
        let display = socket_display().expect("Expected socket display to resolve");
        assert!(!display.trim().is_empty());
    }

    #[test]
    fn test_discover_socket_for_sessions_returns_none_for_empty_set() {
        let wanted = std::collections::HashSet::<String>::new();
        assert!(discover_socket_for_sessions(&wanted, None).is_none());
    }

    #[test]
    fn test_terminate_mux_daemon_for_socket_rejects_empty_socket() {
        let err =
            terminate_mux_daemon_for_socket("   ").expect_err("Expected empty socket to error");
        assert!(err.to_string().contains("Mux socket cannot be empty"));
    }

    #[test]
    fn test_terminate_mux_daemon_for_socket_noops_when_no_matches() {
        let socket = format!("tenex-mux-test-no-matches-{}", uuid::Uuid::new_v4());
        terminate_mux_daemon_for_socket(&socket).expect("Expected no-op termination to succeed");
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_for_ready_reports_error_when_stdout_missing() {
        let socket = format!("tenex-mux-test-stdout-missing-{}", uuid::Uuid::new_v4());
        let mut child = spawn_test_muxd(
            &socket,
            r#"
import sys
import time

sys.stdout.write("ready\n")
sys.stdout.flush()

while True:
    time.sleep(60)
	"#,
        );

        let _ = child.stdout.take();
        let err = wait_for_ready(&mut child).expect_err("Expected stdout missing to error");
        assert!(err.to_string().contains("expose stdout"));
        cleanup_child(&mut child);
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_reports_signal_failure() {
        let socket = format!("tenex-mux-test-signal-fail-{}", uuid::Uuid::new_v4());
        let pid = 999_999u32;

        discovery::clear_test_discovery_overrides();
        discovery::set_test_mux_daemon_pids_for_socket(&socket, vec![pid]);
        discovery::set_test_pid_is_alive(pid, true);

        let err = terminate_mux_daemon_for_socket(&socket)
            .expect_err("Expected termination to report send failure");
        assert!(err.to_string().contains("kill -TERM"));
        assert!(err.to_string().contains(&pid.to_string()));

        discovery::clear_test_discovery_overrides();
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_reports_failure_when_pids_never_exit() {
        use std::os::unix::process::ExitStatusExt;

        let socket = format!("tenex-mux-test-alive-override-{}", uuid::Uuid::new_v4());
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
        );

        wait_for_ready_or_cleanup(&mut child).expect("Expected ready check to succeed");

        let pid = child.id();
        discovery::clear_test_discovery_overrides();
        discovery::set_test_mux_daemon_pids_for_socket(&socket, vec![pid]);
        discovery::set_test_pid_is_alive(pid, true);

        let err = terminate_mux_daemon_for_socket(&socket)
            .expect_err("Expected termination to bail when pids stay alive");
        assert!(err.to_string().contains("Failed to terminate mux daemon"));

        discovery::clear_test_discovery_overrides();

        let status = child.wait().expect("Expected muxd child to exit");
        assert!(status.signal() == Some(9));
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_kills_matching_process() {
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
        );
        #[cfg(not(target_os = "linux"))]
        let _pid_guard = super::pidfile::PidFileGuard::create_for_pid(&socket, child.id())
            .expect("Expected pidfile to be created");

        wait_for_ready_or_cleanup(&mut child).expect("Expected ready check to succeed");
        ensure_discoverable_or_cleanup(&mut child, &socket)
            .expect("Expected muxd child to be discoverable");

        terminate_mux_daemon_for_socket(&socket).expect("Expected muxd child to be terminated");
        let status = child.wait().expect("Expected muxd child to exit");
        assert!(status.success());
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_escalates_to_kill_when_ignoring_term() {
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
        );
        #[cfg(not(target_os = "linux"))]
        let _pid_guard = super::pidfile::PidFileGuard::create_for_pid(&socket, child.id())
            .expect("Expected pidfile to be created");

        wait_for_ready_or_cleanup(&mut child).expect("Expected ready check to succeed");
        ensure_discoverable_or_cleanup(&mut child, &socket)
            .expect("Expected muxd child to be discoverable");

        terminate_mux_daemon_for_socket(&socket).expect("Expected muxd child to be terminated");
        let status = child.wait().expect("Expected muxd child to exit");
        assert!(status.signal() == Some(9));
    }

    #[test]
    fn test_running_daemon_version_reports_endpoint_errors() {
        set_socket_override("/tmp/tenex-mux-test\0bad.sock").expect("Expected override to be set");
        assert!(running_daemon_version().is_err());
    }

    #[test]
    fn test_run_mux_daemon_reports_endpoint_errors() {
        set_socket_override("/tmp/tenex-mux-test\0bad.sock").expect("Expected override to be set");
        assert!(run_mux_daemon().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_accepts_dead_pids() {
        let socket = format!("tenex-mux-test-dead-pid-{}", uuid::Uuid::new_v4());
        let pid = 999_999u32;

        discovery::clear_test_discovery_overrides();
        discovery::set_test_mux_daemon_pids_for_socket(&socket, vec![pid]);
        discovery::set_test_pid_is_alive(pid, false);

        terminate_mux_daemon_for_socket(&socket).expect("Expected dead pid termination to succeed");
        discovery::clear_test_discovery_overrides();
    }

    #[cfg(unix)]
    #[test]
    fn test_terminate_mux_daemon_for_socket_reports_kill_spawn_failures() {
        let socket = format!("tenex-mux-test-kill-spawn-fail-{}", uuid::Uuid::new_v4());
        let pid = 999_999u32;

        discovery::clear_test_discovery_overrides();
        discovery::set_test_mux_daemon_pids_for_socket(&socket, vec![pid]);
        discovery::set_test_pid_is_alive(pid, true);

        let missing = PathBuf::from("/definitely/missing/tenex-kill");
        let err = with_kill_program_override_for_tests(missing, || {
            terminate_mux_daemon_for_socket(&socket).expect_err("Expected kill spawn to fail")
        });
        assert!(err.to_string().contains("Failed to invoke kill -TERM"));
        discovery::clear_test_discovery_overrides();
    }

    #[test]
    fn test_running_daemon_version_returns_none_when_write_fails() {
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (display, name) = make_mock_socket(&socket_dir);
        let server = spawn_mock_ping_server(name, MockPingMode::CloseOnAccept);

        set_socket_override(&display).expect("Expected socket override to be set");
        assert!(
            running_daemon_version()
                .expect("Expected running daemon version to succeed")
                .is_none()
        );
        server.join().expect("mock server join");
    }

    #[test]
    fn test_running_daemon_version_returns_none_when_read_fails() {
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (display, name) = make_mock_socket(&socket_dir);
        let server = spawn_mock_ping_server(name, MockPingMode::CloseAfterRead);

        set_socket_override(&display).expect("Expected socket override to be set");
        assert!(
            running_daemon_version()
                .expect("Expected running daemon version to succeed")
                .is_none()
        );
        server.join().expect("mock server join");
    }

    #[test]
    fn test_running_daemon_version_reports_response_error() {
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (display, name) = make_mock_socket(&socket_dir);
        let response = Arc::new(protocol::MuxResponse::Err {
            message: "mock error".to_string(),
        });
        let server = spawn_mock_ping_server(name, MockPingMode::Respond(response));

        set_socket_override(&display).expect("Expected socket override to be set");
        let err = running_daemon_version().expect_err("Expected daemon version to fail");
        assert!(err.to_string().contains("mock error"));
        server.join().expect("mock server join");
    }

    #[test]
    fn test_running_daemon_version_reports_unexpected_response() {
        let socket_dir = TempDir::new().expect("Expected temp dir");
        let (display, name) = make_mock_socket(&socket_dir);
        let response = Arc::new(protocol::MuxResponse::Ok);
        let server = spawn_mock_ping_server(name, MockPingMode::Respond(response));

        set_socket_override(&display).expect("Expected socket override to be set");
        let err = running_daemon_version().expect_err("Expected daemon version to fail");
        assert!(err.to_string().contains("Unexpected mux response"));
        server.join().expect("mock server join");
    }

    #[test]
    fn test_running_daemon_version_returns_none_when_connect_fails() {
        let socket = format!("tenex-mux-test-missing-{}", uuid::Uuid::new_v4());
        set_socket_override(&socket).expect("Expected socket override to be set");
        assert!(
            running_daemon_version()
                .expect("Expected running daemon version to succeed")
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_for_ready_or_cleanup_reports_error() {
        let socket = format!("tenex-mux-test-ready-fail-{}", uuid::Uuid::new_v4());
        let mut child = spawn_test_muxd(
            &socket,
            r#"
import sys
import time

sys.stdout.write("not-ready\n")
sys.stdout.flush()

while True:
    time.sleep(60)
	"#,
        );

        let err = wait_for_ready_or_cleanup(&mut child).expect_err("Expected readiness to fail");
        assert!(err.to_string().contains("signal readiness"));
    }

    #[cfg(unix)]
    #[test]
    fn test_ensure_discoverable_or_cleanup_reports_error() {
        let socket = format!("tenex-mux-test-discover-fail-{}", uuid::Uuid::new_v4());
        let mut child = spawn_test_muxd(
            &socket,
            r#"
import sys
import time

sys.stdout.write("ready\n")
sys.stdout.flush()

while True:
    time.sleep(60)
	"#,
        );

        wait_for_ready_or_cleanup(&mut child).expect("Expected ready check to succeed");
        let wrong_socket = format!("tenex-mux-test-wrong-{}", uuid::Uuid::new_v4());
        let err = ensure_discoverable_or_cleanup(&mut child, &wrong_socket)
            .expect_err("Expected discoverability check to fail");
        assert!(err.to_string().contains("discoverable"));
    }
}
