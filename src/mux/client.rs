//! Client for talking to the mux daemon.

use super::endpoint::{SocketEndpoint, socket_endpoint};
use super::ipc;
use super::protocol::{MuxRequest, MuxResponse};
use anyhow::{Context, Result};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
use parking_lot::Mutex;

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

static CLIENT: OnceLock<Mutex<MuxClient>> = OnceLock::new();

static ENDPOINT: OnceLock<SocketEndpoint> = OnceLock::new();

const DAEMON_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(25);

const DAEMON_CONNECT_RETRY_ATTEMPTS: usize = 200;

/// Send a request to the mux daemon.
///
/// This will start the daemon if needed.
///
/// # Errors
///
/// Returns an error if the daemon cannot be reached or the request fails.
pub fn request(req: &MuxRequest) -> Result<MuxResponse> {
    let endpoint = endpoint()?;

    {
        let client = CLIENT.get_or_init(|| Mutex::new(MuxClient::new(endpoint)));
        let mut client = client.lock();
        client.request(req)
    }
}

pub(super) fn endpoint() -> Result<SocketEndpoint> {
    {
        if let Some(endpoint) = ENDPOINT.get() {
            return Ok(endpoint.clone());
        }

        let endpoint = socket_endpoint()?;
        let _ = ENDPOINT.set(endpoint.clone());
        Ok(endpoint)
    }
}

/// A synchronous request/response mux client.
#[derive(Debug)]
pub struct MuxClient {
    /// IPC endpoint.
    pub endpoint: SocketEndpoint,
    /// Active connection when available.
    pub stream: Option<Stream>,
}

impl MuxClient {
    /// Create a new client for an endpoint.
    #[must_use]
    pub const fn new(endpoint: SocketEndpoint) -> Self {
        Self {
            endpoint,
            stream: None,
        }
    }

    /// Send a request.
    ///
    /// # Errors
    ///
    /// Returns an error if the daemon cannot be reached or the request fails.
    pub fn request(&mut self, req: &MuxRequest) -> Result<MuxResponse> {
        let stream = self.ensure_connected()?;
        if let Ok(response) = send_request(stream, req) {
            return Ok(response);
        }

        self.stream = None;
        let stream = self.ensure_connected()?;
        send_request(stream, req)
    }

    fn ensure_connected(&mut self) -> Result<&mut Stream> {
        if self.stream.is_none() {
            if let Ok(stream) = Stream::connect(self.endpoint.name.clone()) {
                self.stream = Some(stream);
            } else {
                start_daemon(&self.endpoint)?;

                for _ in 0..DAEMON_CONNECT_RETRY_ATTEMPTS {
                    match Stream::connect(self.endpoint.name.clone()) {
                        Ok(stream) => {
                            self.stream = Some(stream);
                            break;
                        }
                        Err(_) => std::thread::sleep(DAEMON_CONNECT_RETRY_INTERVAL),
                    }
                }

                if self.stream.is_none() {
                    return Err(anyhow::anyhow!(
                        "Failed to connect to mux daemon at {}",
                        self.endpoint.display
                    ));
                }
            }
        }

        self.stream
            .as_mut()
            .context("Mux stream missing after connect")
    }
}

fn send_request(stream: &mut Stream, req: &MuxRequest) -> Result<MuxResponse> {
    ipc::write_json(stream, req)?;
    ipc::read_json(stream)
}

fn start_daemon(endpoint: &SocketEndpoint) -> Result<()> {
    let exe = resolve_tenex_executable()?;

    let mut cmd = {
        let mut cmd = Command::new(exe);
        cmd.arg("muxd");
        cmd
    };

    cmd.env("TENEX_MUX_SOCKET", &endpoint.display);
    if let Ok(state_path) = std::env::var("TENEX_STATE_PATH") {
        let state_path = state_path.trim();
        if !state_path.is_empty() {
            cmd.env("TENEX_STATE_PATH", state_path);
        }
    }

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Put the mux daemon in a separate process group so Ctrl+C in the Tenex TUI
    // doesn't tear down the daemon (and thus all agent sessions).
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }

    let child = cmd.spawn().context("Failed to spawn mux daemon")?;

    drop(child);
    Ok(())
}

fn resolve_tenex_executable() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let is_tenex = exe
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == env!("CARGO_PKG_NAME"));
    if is_tenex {
        return Ok(exe);
    }

    let candidate_dir = exe
        .parent()
        .and_then(std::path::Path::parent)
        .context("Failed to resolve build output directory")?;

    let base_name = env!("CARGO_PKG_NAME");
    let candidates = [
        candidate_dir.join(format!("{base_name}{}", std::env::consts::EXE_SUFFIX)),
        candidate_dir.join(base_name),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Ok(exe)
}
