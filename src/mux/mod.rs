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

use anyhow::{Context, Result, bail};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
use std::process::Command;
use std::time::{Duration, Instant};

fn try_ping(stream: &mut Stream) -> Option<protocol::MuxResponse> {
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
pub fn discover_socket_for_sessions(
    wanted_sessions: &std::collections::HashSet<String, impl std::hash::BuildHasher>,
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
