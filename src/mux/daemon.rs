//! Background mux daemon.
#![cfg_attr(coverage_nightly, coverage(off))]

use super::endpoint::SocketEndpoint;
use super::ipc;
use super::protocol::{CaptureKind, MuxRequest, MuxResponse, SessionInfo, WindowInfo};
use anyhow::{Context, Result};
use base64::Engine as _;
use interprocess::local_socket::traits::{ListenerExt, Stream as StreamTrait};
use interprocess::local_socket::{ListenerOptions, Stream};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

static RESIZE_MAX: OnceLock<Mutex<HashMap<String, (u16, u16)>>> = OnceLock::new();

trait ReadWrite: io::Read + io::Write {}

impl<T: io::Read + io::Write + ?Sized> ReadWrite for T {}

fn new_resize_max() -> Mutex<HashMap<String, (u16, u16)>> {
    Mutex::new(HashMap::new())
}

fn resize_max() -> &'static Mutex<HashMap<String, (u16, u16)>> {
    RESIZE_MAX.get_or_init(new_resize_max)
}

/// Run the mux daemon in the foreground.
///
/// # Errors
///
/// Returns an error if the listener cannot be created or if a fatal I/O error occurs.
pub fn run(endpoint: &SocketEndpoint) -> Result<()> {
    run_with_connection_limit(endpoint, None)
}

fn run_with_connection_limit(
    endpoint: &SocketEndpoint,
    connection_limit: Option<usize>,
) -> Result<()> {
    if let Some(path) = endpoint
        .cleanup_path
        .as_ref()
        .and_then(|path| path.parent())
    {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create mux socket directory {}", path.display()))?;
    }

    let listener = match ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
    {
        Ok(listener) => listener,
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            if try_ping_existing(endpoint) {
                info!(endpoint = %endpoint.display, "Mux daemon already running");
                return Ok(());
            }

            if let Some(path) = endpoint.cleanup_path.as_ref() {
                let _ = std::fs::remove_file(path);
                ListenerOptions::new()
                    .name(endpoint.name.clone())
                    .create_sync()
                    .context("Failed to recreate mux listener after cleanup")?
            } else {
                return Err(err).context("Mux endpoint is already in use");
            }
        }
        Err(err) => return Err(err).context("Failed to create mux listener"),
    };

    #[cfg(not(target_os = "linux"))]
    let _pid_guard =
        super::pidfile::PidFileGuard::create(&endpoint.display).with_context(|| {
            format!(
                "Failed to create mux daemon pid file for socket {}",
                endpoint.display
            )
        })?;

    info!(endpoint = %endpoint.display, "Mux daemon listening");

    let mut incoming = listener.incoming();
    serve_incoming(&mut incoming, connection_limit);

    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn serve_incoming(
    incoming: &mut dyn Iterator<Item = io::Result<Stream>>,
    connection_limit: Option<usize>,
) {
    if connection_limit == Some(0) {
        return;
    }

    let limit = connection_limit.unwrap_or(usize::MAX);
    let mut accepted = 0usize;
    for conn in incoming {
        match conn {
            Ok(stream) => {
                accepted = accepted.saturating_add(1);
                std::thread::spawn(move || handle_connection_spawned(stream));

                if accepted >= limit {
                    break;
                }
            }
            Err(err) => {
                warn!(error = %err, "Mux accept failed");
            }
        }
    }
}

fn handle_connection_spawned(mut stream: Stream) {
    handle_connection(&mut stream).unwrap_or_else(|err| {
        debug!(error = %err, "Mux client connection closed");
    });
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn try_ping_stream(stream: &mut dyn ReadWrite) -> bool {
    if ipc::write_json(stream, &MuxRequest::Ping).is_err() {
        return false;
    }

    ipc::read_json::<MuxResponse>(stream).is_ok()
}

fn try_ping_existing(endpoint: &SocketEndpoint) -> bool {
    let Ok(mut stream) = Stream::connect(endpoint.name.clone()) else {
        return false;
    };

    try_ping_stream(&mut stream)
}

fn handle_connection(stream: &mut dyn ReadWrite) -> Result<()> {
    loop {
        let request: MuxRequest = match ipc::read_json(stream) {
            Ok(req) => req,
            Err(err) => {
                return Err(err);
            }
        };

        let response = match dispatch_request(request) {
            Ok(response) => response,
            Err(err) => MuxResponse::Err {
                message: err.to_string(),
            },
        };

        ipc::write_json(stream, &response)?;
    }
}

fn dispatch_request(request: MuxRequest) -> Result<MuxResponse> {
    match request {
        MuxRequest::Ping => Ok(handle_ping()),
        MuxRequest::ListSessions => Ok(handle_list_sessions()),
        MuxRequest::SessionExists { name } => Ok(handle_session_exists(&name)),
        MuxRequest::CreateSession {
            name,
            working_dir,
            command,
            ..
        } => handle_create_session(&name, &working_dir, &command),
        MuxRequest::KillSession { name } => handle_kill_session(&name),
        MuxRequest::RenameSession { old_name, new_name } => {
            handle_rename_session(&old_name, &new_name)
        }
        MuxRequest::ListWindows { session } => handle_list_windows(&session),
        MuxRequest::CreateWindow {
            session,
            window_name,
            working_dir,
            command,
            ..
        } => handle_create_window(&session, &window_name, &working_dir, &command),
        MuxRequest::KillWindow {
            session,
            window_index,
        } => handle_kill_window(&session, window_index),
        MuxRequest::RenameWindow {
            session,
            window_index,
            new_name,
        } => handle_rename_window(&session, window_index, &new_name),
        MuxRequest::Resize { target, cols, rows } => handle_resize(&target, cols, rows),
        MuxRequest::SendInput { target, data } => handle_send_input(&target, &data),
        MuxRequest::Capture { target, kind } => {
            let content = handle_capture(&target, kind)?;
            Ok(MuxResponse::Text { text: content })
        }
        MuxRequest::PaneSize { target } => handle_pane_size(&target),
        MuxRequest::CursorPosition { target } => handle_cursor_position(&target),
        MuxRequest::PaneCurrentCommand { target } => handle_pane_current_command(&target),
        MuxRequest::Tail { target, lines } => {
            let content = handle_tail(&target, lines)?;
            Ok(MuxResponse::Text { text: content })
        }
        MuxRequest::ReadOutput {
            target,
            after,
            max_bytes,
        } => handle_read_output(&target, after, max_bytes),
        MuxRequest::OutputCursor { target } => handle_output_cursor(&target),
        MuxRequest::ListPanePids { session } => handle_list_pids(&session),
    }
}

fn handle_ping() -> MuxResponse {
    MuxResponse::Pong {
        version: super::version(),
    }
}

fn handle_list_sessions() -> MuxResponse {
    let sessions = super::server::SessionManager::list()
        .into_iter()
        .map(|s| SessionInfo {
            name: s.name,
            created: s.created,
            attached: s.attached,
        })
        .collect();
    MuxResponse::Sessions { sessions }
}

fn handle_session_exists(name: &str) -> MuxResponse {
    MuxResponse::Bool {
        value: super::server::SessionManager::exists(name),
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn handle_create_session(name: &str, working_dir: &str, command: &[String]) -> Result<MuxResponse> {
    let dir = Path::new(working_dir);
    let command = if command.is_empty() {
        None
    } else {
        Some(command)
    };
    super::server::SessionManager::create(name, dir, command)?;
    Ok(MuxResponse::Ok)
}

fn handle_kill_session(name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::kill(name)?;
    let prefix = format!("{name}:");
    resize_max()
        .lock()
        .retain(|target, _| target != name && !target.starts_with(&prefix));
    Ok(MuxResponse::Ok)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn handle_rename_session(old_name: &str, new_name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::rename(old_name, new_name)?;
    {
        let mut guard = resize_max().lock();
        if let Some(value) = guard.remove(old_name) {
            guard.insert(new_name.to_string(), value);
        }

        let old_prefix = format!("{old_name}:");
        let new_prefix = format!("{new_name}:");
        let updates: Vec<(String, String, (u16, u16))> = guard
            .iter()
            .filter_map(|(target, dims)| {
                target
                    .strip_prefix(&old_prefix)
                    .map(|suffix| (target.clone(), format!("{new_prefix}{suffix}"), *dims))
            })
            .collect();
        for (old_target, _, _) in &updates {
            guard.remove(old_target);
        }
        for (_, new_target, dims) in updates {
            guard.insert(new_target, dims);
        }
    }
    Ok(MuxResponse::Ok)
}

fn handle_list_windows(session: &str) -> Result<MuxResponse> {
    let windows = super::server::SessionManager::list_windows(session)?
        .into_iter()
        .map(|w| WindowInfo {
            index: w.index,
            name: w.name,
        })
        .collect();
    Ok(MuxResponse::Windows { windows })
}

fn handle_create_window(
    session: &str,
    window_name: &str,
    working_dir: &str,
    command: &[String],
) -> Result<MuxResponse> {
    let dir = Path::new(working_dir);
    let command = if command.is_empty() {
        None
    } else {
        Some(command)
    };
    let index = super::server::SessionManager::create_window(session, window_name, dir, command)?;
    Ok(MuxResponse::WindowCreated { index })
}

fn handle_kill_window(session: &str, window_index: u32) -> Result<MuxResponse> {
    super::server::SessionManager::kill_window(session, window_index)?;
    resize_max()
        .lock()
        .remove(&format!("{session}:{window_index}"));
    Ok(MuxResponse::Ok)
}

fn handle_rename_window(session: &str, window_index: u32, new_name: &str) -> Result<MuxResponse> {
    super::server::SessionManager::rename_window(session, window_index, new_name)?;
    Ok(MuxResponse::Ok)
}

fn handle_resize(target: &str, cols: u16, rows: u16) -> Result<MuxResponse> {
    let current = resize_max().lock().get(target).copied().unwrap_or((0, 0));
    let proposed = (current.0.max(cols), current.1.max(rows));

    super::server::SessionManager::resize_window(target, proposed.0, proposed.1)?;
    resize_max().lock().insert(target.to_string(), proposed);
    Ok(MuxResponse::Ok)
}

fn handle_send_input(target: &str, data: &[u8]) -> Result<MuxResponse> {
    super::server::SessionManager::send_input(target, data)?;
    Ok(MuxResponse::Ok)
}

fn handle_capture(target: &str, kind: CaptureKind) -> Result<String> {
    match kind {
        CaptureKind::Visible => super::server::OutputCapture::capture_pane(target),
        CaptureKind::History { lines } => {
            super::server::OutputCapture::capture_pane_with_history(target, lines)
        }
        CaptureKind::FullHistory => super::server::OutputCapture::capture_full_history(target),
    }
}

fn handle_pane_size(target: &str) -> Result<MuxResponse> {
    let (cols, rows) = super::server::OutputCapture::pane_size(target)?;
    Ok(MuxResponse::Size { cols, rows })
}

fn handle_cursor_position(target: &str) -> Result<MuxResponse> {
    let (x, y, hidden) = super::server::OutputCapture::cursor_position(target)?;
    Ok(MuxResponse::Position { x, y, hidden })
}

fn handle_pane_current_command(target: &str) -> Result<MuxResponse> {
    let cmd = super::server::OutputCapture::pane_current_command(target)?;
    Ok(MuxResponse::Text { text: cmd })
}

fn handle_tail(target: &str, lines: u32) -> Result<String> {
    let lines = usize::try_from(lines).map_or(usize::MAX, |value| value);
    Ok(super::server::OutputCapture::tail(target, lines)?.join("\n"))
}

enum ReadResult {
    Chunk { start: u64, data: Vec<u8> },
    Reset { start: u64, checkpoint: Vec<u8> },
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn handle_read_output(target: &str, after: u64, max_bytes: u32) -> Result<MuxResponse> {
    use base64::engine::general_purpose::STANDARD as BASE64;

    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let window = super::backend::resolve_window(target)?;

    let result = {
        let guard = window.lock();

        if after < guard.output_history.seq_start {
            let checkpoint = guard
                .output_history
                .checkpoint
                .as_ref()
                .filter(|checkpoint| checkpoint.seq == guard.output_history.seq_start)
                .map(|checkpoint| checkpoint.bytes.clone())
                .unwrap_or_default();

            ReadResult::Reset {
                start: guard.output_history.seq_start,
                checkpoint,
            }
        } else if after >= guard.output_history.seq_end {
            ReadResult::Chunk {
                start: after,
                data: Vec::new(),
            }
        } else {
            let offset =
                usize::try_from(after.saturating_sub(guard.output_history.seq_start)).unwrap_or(0);
            let take = guard
                .output_history
                .buf
                .len()
                .saturating_sub(offset)
                .min(max_bytes);
            let end_offset = offset.saturating_add(take);
            ReadResult::Chunk {
                start: after,
                data: guard
                    .output_history
                    .buf
                    .get(offset..end_offset)
                    .unwrap_or_default()
                    .to_vec(),
            }
        }
    };

    match result {
        ReadResult::Chunk { start, data } => {
            let end = start.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
            Ok(MuxResponse::OutputChunk {
                start,
                end,
                data_b64: if data.is_empty() {
                    String::new()
                } else {
                    BASE64.encode(data)
                },
            })
        }
        ReadResult::Reset { start, checkpoint } => Ok(MuxResponse::OutputReset {
            start,
            checkpoint_b64: if checkpoint.is_empty() {
                String::new()
            } else {
                BASE64.encode(checkpoint)
            },
        }),
    }
}

fn handle_output_cursor(target: &str) -> Result<MuxResponse> {
    let window = super::backend::resolve_window(target)?;
    let (start, end) = {
        let guard = window.lock();
        (guard.output_history.seq_start, guard.output_history.seq_end)
    };
    Ok(MuxResponse::OutputCursor { start, end })
}

fn handle_list_pids(session: &str) -> Result<MuxResponse> {
    let pids = super::server::SessionManager::list_pane_pids(session)?;
    Ok(MuxResponse::Pids { pids })
}

#[cfg(test)]
mod tests;
