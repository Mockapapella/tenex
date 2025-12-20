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

/// Send a request to the mux daemon.
///
/// This will start the daemon if needed.
///
/// # Errors
///
/// Returns an error if the daemon cannot be reached or the request fails.
pub fn request(req: &MuxRequest) -> Result<MuxResponse> {
    let endpoint = if let Some(endpoint) = ENDPOINT.get() {
        endpoint.clone()
    } else {
        let endpoint = socket_endpoint()?;
        let _ = ENDPOINT.set(endpoint.clone());
        endpoint
    };

    let client = CLIENT.get_or_init(|| Mutex::new(MuxClient::new(endpoint)));
    let mut client = client.lock();
    client.request(req)
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
        self.ensure_connected()?;

        if let Some(stream) = self.stream.as_mut()
            && let Ok(response) = send_request(stream, req)
        {
            return Ok(response);
        }

        self.stream = None;
        self.ensure_connected()?;
        let stream = self
            .stream
            .as_mut()
            .context("Mux stream missing after reconnect")?;
        send_request(stream, req)
    }

    fn ensure_connected(&mut self) -> Result<()> {
        if self.stream.is_some() {
            return Ok(());
        }

        if let Ok(stream) = Stream::connect(self.endpoint.name.clone()) {
            self.stream = Some(stream);
            return Ok(());
        }

        start_daemon(&self.endpoint)?;

        for _ in 0..20 {
            match Stream::connect(self.endpoint.name.clone()) {
                Ok(stream) => {
                    self.stream = Some(stream);
                    return Ok(());
                }
                Err(_) => std::thread::sleep(Duration::from_millis(25)),
            }
        }

        Err(anyhow::anyhow!(
            "Failed to connect to mux daemon at {}",
            self.endpoint.display
        ))
    }
}

fn send_request(stream: &mut Stream, req: &MuxRequest) -> Result<MuxResponse> {
    ipc::write_json(stream, req)?;
    ipc::read_json(stream)
}

fn start_daemon(endpoint: &SocketEndpoint) -> Result<()> {
    let exe = resolve_tenex_executable()?;
    Command::new(exe)
        .arg("muxd")
        .env("TENEX_MUX_SOCKET", &endpoint.display)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to spawn mux daemon")?;
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

    let exe_name = if cfg!(windows) {
        format!("{}.exe", env!("CARGO_PKG_NAME"))
    } else {
        env!("CARGO_PKG_NAME").to_string()
    };

    let candidate = candidate_dir.join(exe_name);
    if candidate.exists() {
        return Ok(candidate);
    }

    Ok(exe)
}
