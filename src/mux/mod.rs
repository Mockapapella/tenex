//! Cross-platform multiplexer integration module.

mod backend;
mod capture;
mod client;
mod daemon;
mod discovery;
mod endpoint;
mod ipc;
mod protocol;
mod server;
mod session;

pub use capture::Capture as OutputCapture;
pub use endpoint::set_socket_override;
pub use session::{Manager as SessionManager, Session, Window};

use anyhow::Result;
use interprocess::local_socket::traits::Stream as StreamTrait;
use std::time::Duration;

const IPC_PING_TIMEOUT: Duration = Duration::from_millis(250);

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
            if stream.set_nonblocking(true).is_err() {
                return false;
            }
            if ipc::write_json_with_timeout(
                &mut stream,
                &protocol::MuxRequest::Ping,
                IPC_PING_TIMEOUT,
            )
            .is_err()
            {
                return false;
            }
            ipc::read_json_with_timeout::<_, protocol::MuxResponse>(&mut stream, IPC_PING_TIMEOUT)
                .is_ok()
        }
        Err(_) => false,
    }
}

/// Determine whether a mux daemon is accepting connections on `socket` but not responding.
///
/// This is used to avoid hangs when an old (or wedged) daemon remains bound to a persisted socket.
/// Connection failures are treated as "not unresponsive" because Tenex can safely start a new
/// daemon on that socket later.
#[must_use]
pub fn socket_is_unresponsive(socket: &str, timeout: Duration) -> bool {
    let Ok(endpoint) = endpoint::socket_endpoint_from_value(socket) else {
        return false;
    };

    match interprocess::local_socket::Stream::connect(endpoint.name) {
        Ok(mut stream) => {
            if stream.set_nonblocking(true).is_err() {
                return true;
            }

            if ipc::write_json_with_timeout(&mut stream, &protocol::MuxRequest::Ping, timeout)
                .is_err()
            {
                return true;
            }

            ipc::read_json_with_timeout::<_, protocol::MuxResponse>(&mut stream, timeout).is_err()
        }
        Err(_) => false,
    }
}

/// Get the mux daemon version string.
///
/// # Errors
///
/// Returns an error if the version cannot be constructed.
pub fn version() -> Result<String> {
    Ok(format!("tenex-mux/{}", env!("CARGO_PKG_VERSION")))
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
    fn test_socket_is_unresponsive_when_server_never_responds() -> Result<()> {
        use interprocess::local_socket::traits::ListenerExt;
        use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};

        let temp_dir = tempfile::tempdir()?;
        let socket_path = temp_dir.path().join("tenex-test-unresponsive.sock");
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()?
            .into_owned();

        let listener = ListenerOptions::new().name(name).create_sync()?;
        let handle = std::thread::spawn(move || {
            if let Some(stream) = listener.incoming().next()
                && let Ok(_stream) = stream
            {
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let display = socket_path.to_string_lossy().into_owned();
        assert!(socket_is_unresponsive(&display, Duration::from_millis(50)));

        let _ = handle.join();
        Ok(())
    }
}
