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
}
