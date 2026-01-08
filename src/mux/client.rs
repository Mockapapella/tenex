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
    let mut cmd = Command::new(exe);
    cmd.arg("muxd")
        .env("TENEX_MUX_SOCKET", &endpoint.display)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Put the mux daemon in a separate process group so Ctrl+C in the Tenex TUI
    // doesn't tear down the daemon (and thus all agent sessions).
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }

    cmd.spawn().context("Failed to spawn mux daemon")?;
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

    let exe_name = env!("CARGO_PKG_NAME").to_string();

    let candidate = candidate_dir.join(exe_name);
    if candidate.exists() {
        return Ok(candidate);
    }

    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_tenex_executable_returns_existing_path() -> Result<()> {
        let exe = resolve_tenex_executable()?;
        assert!(exe.exists());
        Ok(())
    }

    #[test]
    fn test_mux_client_reconnects_after_failed_request() -> Result<()> {
        use interprocess::local_socket::traits::ListenerExt;
        use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};

        let temp_dir = tempfile::tempdir()?;
        let socket_path = temp_dir.path().join("mux-client.sock");
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()?
            .into_owned();

        let endpoint = SocketEndpoint {
            name: name.clone(),
            cleanup_path: Some(socket_path.clone()),
            display: socket_path.to_string_lossy().into_owned(),
        };

        let listener = ListenerOptions::new().name(name).create_sync()?;
        let handle = std::thread::spawn(move || -> Result<()> {
            let mut incoming = listener.incoming();

            let mut stream = incoming
                .next()
                .ok_or_else(|| anyhow::anyhow!("Missing initial connection"))??;
            let _: MuxRequest = ipc::read_json(&mut stream)?;
            drop(stream);

            let mut stream = incoming
                .next()
                .ok_or_else(|| anyhow::anyhow!("Missing reconnect connection"))??;
            let _: MuxRequest = ipc::read_json(&mut stream)?;
            ipc::write_json(
                &mut stream,
                &MuxResponse::Pong {
                    version: "test".to_string(),
                },
            )?;
            Ok(())
        });

        let mut client = MuxClient::new(endpoint);
        let response = client.request(&MuxRequest::Ping)?;
        assert!(matches!(response, MuxResponse::Pong { .. }));

        match handle.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(anyhow::anyhow!("Server thread panicked")),
        }
    }
}
