//! Client for talking to the mux daemon.

#![cfg_attr(all(coverage, not(test)), allow(dead_code))]

use super::endpoint::{SocketEndpoint, socket_endpoint};
use super::ipc;
use super::protocol::{MuxRequest, MuxResponse};
use anyhow::{Context, Result};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
use parking_lot::Mutex;
#[cfg(test)]
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

#[cfg(not(test))]
static CLIENT: OnceLock<Mutex<MuxClient>> = OnceLock::new();
#[cfg(not(test))]
static ENDPOINT: OnceLock<SocketEndpoint> = OnceLock::new();
#[cfg(test)]
static TEST_CLIENTS: OnceLock<Mutex<HashMap<String, MuxClient>>> = OnceLock::new();
#[cfg(test)]
const DAEMON_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(1);
#[cfg(not(test))]
const DAEMON_CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(test)]
const DAEMON_CONNECT_RETRY_ATTEMPTS: usize = 800;
#[cfg(not(test))]
const DAEMON_CONNECT_RETRY_ATTEMPTS: usize = 200;
#[cfg(test)]
const MUXD_TEST_ENTRY: &str = "mux::client::tests::__tenex_muxd_test_entry";
#[cfg(test)]
const MUXD_FALLBACK_CLIENT_ENTRY: &str = "mux::client::tests::__tenex_muxd_fallback_client_entry";
#[cfg(test)]
const RESOLVE_EXE_IS_TENEX_ENTRY: &str = "mux::client::tests::__tenex_resolve_exe_is_tenex_entry";

/// Send a request to the mux daemon.
///
/// This will start the daemon if needed.
///
/// # Errors
///
/// Returns an error if the daemon cannot be reached or the request fails.
pub fn request(req: &MuxRequest) -> Result<MuxResponse> {
    let endpoint = endpoint()?;

    #[cfg(test)]
    {
        let scope = test_scope_key();
        {
            let clients = TEST_CLIENTS.get_or_init(|| Mutex::new(HashMap::new()));
            let mut clients = clients.lock();
            let client = clients
                .entry(scope)
                .or_insert_with(|| MuxClient::new(endpoint.clone()));
            if client.endpoint.display != endpoint.display {
                *client = MuxClient::new(endpoint);
            }
            let response = client.request(req);
            drop(clients);
            response
        }
    }

    #[cfg(not(test))]
    {
        let client = CLIENT.get_or_init(|| Mutex::new(MuxClient::new(endpoint)));
        let mut client = client.lock();
        client.request(req)
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SPAWNED_MUXD_CHILDREN: std::cell::RefCell<TestMuxdChildSet> =
        std::cell::RefCell::new(TestMuxdChildSet::default());
}

#[cfg(test)]
#[derive(Default)]
struct TestMuxdChildSet {
    children: Vec<std::process::Child>,
}

#[cfg(test)]
impl Drop for TestMuxdChildSet {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
fn register_spawned_muxd_child(child: std::process::Child) {
    TEST_SPAWNED_MUXD_CHILDREN.with(|slot| slot.borrow_mut().children.push(child));
}

#[cfg(test)]
thread_local! {
    static TEST_CURRENT_EXE_OVERRIDE: std::cell::RefCell<Option<Vec<CurrentExeOverride>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[derive(Clone)]
enum CurrentExeOverride {
    Ok(PathBuf),
    Err,
}

#[cfg(test)]
fn with_current_exe_override<T>(override_value: CurrentExeOverride, f: impl FnOnce() -> T) -> T {
    with_current_exe_overrides(vec![override_value], f)
}

#[cfg(test)]
fn with_current_exe_overrides<T>(
    override_values: Vec<CurrentExeOverride>,
    f: impl FnOnce() -> T,
) -> T {
    TEST_CURRENT_EXE_OVERRIDE.with(|slot| {
        let previous = slot.replace(Some(override_values));
        let result = f();
        slot.replace(previous);
        result
    })
}

fn current_exe() -> std::io::Result<PathBuf> {
    #[cfg(test)]
    {
        let override_value = TEST_CURRENT_EXE_OVERRIDE.with(|slot| {
            let mut slot = slot.borrow_mut();
            let values = slot.as_mut()?;
            if values.is_empty() {
                *slot = None;
                return None;
            }

            let next = values.remove(0);
            if values.is_empty() {
                *slot = None;
            }
            Some(next)
        });

        if let Some(value) = override_value {
            return match value {
                CurrentExeOverride::Ok(path) => Ok(path),
                CurrentExeOverride::Err => Err(std::io::Error::other("forced current_exe failure")),
            };
        }
    }

    std::env::current_exe()
}

pub(super) fn endpoint() -> Result<SocketEndpoint> {
    #[cfg(test)]
    {
        socket_endpoint()
    }

    #[cfg(not(test))]
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

trait ClientStream: Read + Write {}

impl<T: Read + Write + ?Sized> ClientStream for T {}

fn send_request(stream: &mut dyn ClientStream, req: &MuxRequest) -> Result<MuxResponse> {
    ipc::write_json(stream, req)?;
    ipc::read_json(stream)
}

fn maybe_inherit_state_path(cmd: &mut Command, state_path: Option<&str>) {
    let Some(state_path) = state_path else {
        return;
    };

    let trimmed = state_path.trim();
    if trimmed.is_empty() {
        return;
    }

    cmd.env("TENEX_STATE_PATH", trimmed);
}

fn start_daemon(endpoint: &SocketEndpoint) -> Result<()> {
    let exe = resolve_tenex_executable()?;

    #[cfg(test)]
    let mut cmd = {
        let current_exe = current_exe().context("Failed to resolve current test executable")?;
        let is_test_harness_fallback = exe == current_exe
            && exe
                .file_stem()
                .and_then(|name| name.to_str())
                .is_none_or(|name| name != env!("CARGO_PKG_NAME"));

        if is_test_harness_fallback {
            let mut cmd = Command::new(current_exe);
            cmd.args(["--exact", MUXD_TEST_ENTRY, "--nocapture"])
                .env("TENEX_RUN_MUXD_TEST_ENTRY", "1");
            cmd
        } else {
            let mut cmd = Command::new(exe);
            cmd.arg("muxd");
            cmd
        }
    };

    #[cfg(not(test))]
    let mut cmd = {
        let mut cmd = Command::new(exe);
        cmd.arg("muxd");
        cmd
    };

    cmd.env("TENEX_MUX_SOCKET", &endpoint.display);
    maybe_inherit_state_path(&mut cmd, std::env::var("TENEX_STATE_PATH").ok().as_deref());
    #[cfg(coverage)]
    {
        // The mux daemon is coverage-off and may be signal-terminated by tests.
        cmd.env_remove("LLVM_PROFILE_FILE");
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
    #[cfg(test)]
    register_spawned_muxd_child(child);
    #[cfg(not(test))]
    drop(child);
    Ok(())
}

fn resolve_tenex_executable() -> Result<PathBuf> {
    let exe = current_exe().context("Failed to resolve current executable")?;
    resolve_tenex_executable_from_current_exe(&exe)
}

fn resolve_tenex_executable_from_current_exe(exe: &std::path::Path) -> Result<PathBuf> {
    let is_tenex = exe
        .file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == env!("CARGO_PKG_NAME"));
    if is_tenex {
        return Ok(exe.to_path_buf());
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

    Ok(exe.to_path_buf())
}

#[cfg(test)]
fn test_scope_key() -> String {
    std::thread::current().name().map_or_else(
        || format!("{:?}", std::thread::current().id()),
        std::borrow::ToOwned::to_owned,
    )
}

#[cfg(test)]
mod tests;
