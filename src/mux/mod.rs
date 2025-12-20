//! Cross-platform multiplexer integration module.

mod backend;
mod capture;
mod client;
mod daemon;
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
}
