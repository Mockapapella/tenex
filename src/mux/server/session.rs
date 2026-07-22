//! PTY-backed session management (server-side).

use anyhow::{Context, Result, bail};
use std::path::Path;
use tracing::{debug, info, warn};

use super::super::backend::{default_pty_size, global_state, spawn_window, unix_timestamp};

/// Manager for mux sessions.
#[derive(Debug, Clone, Copy, Default)]
pub struct Manager;

impl Manager {
    /// Create a new session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be created.
    pub fn create(name: &str, working_dir: &Path, command: Option<&[String]>) -> Result<()> {
        debug!(name, ?working_dir, ?command, "Creating mux session");

        if Self::exists(name) {
            bail!("Session '{name}' already exists");
        }

        let window = spawn_window(0, name, working_dir, command, default_pty_size())?;

        {
            let mut state = global_state().lock();
            state.sessions.insert(
                name.to_string(),
                std::sync::Arc::new(parking_lot::Mutex::new(super::super::backend::MuxSession {
                    name: name.to_string(),
                    created: unix_timestamp(),
                    root_restart_attempts: 0,
                    last_root_restart: 0,
                    windows: vec![window],
                })),
            );
        }

        info!(name, "Mux session created");
        Ok(())
    }

    /// Kill a session and all its windows.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be killed.
    pub fn kill(name: &str) -> Result<()> {
        debug!(name, "Killing mux session");

        let session = {
            let mut state = global_state().lock();
            state
                .sessions
                .remove(name)
                .context(format!("Session '{name}' not found"))?
        };

        let windows = { session.lock().windows.clone() };
        for window in windows.into_iter().rev() {
            if let Err(err) = kill_window_handle(&window) {
                warn!(error = %err, "Failed to kill mux window");
            }
        }

        info!(name, "Mux session killed");
        Ok(())
    }

    /// Check if a session exists.
    #[must_use]
    pub fn exists(name: &str) -> bool {
        let session = {
            let state = global_state().lock();
            state.sessions.get(name).cloned()
        };

        let Some(session) = session else {
            return false;
        };

        is_session_alive(&session)
    }

    /// List all sessions.
    #[must_use]
    pub fn list() -> Vec<Session> {
        let sessions = {
            let state = global_state().lock();
            state.sessions.values().cloned().collect::<Vec<_>>()
        };

        let mut dead_names = Vec::new();
        let mut result = Vec::new();

        for session in sessions {
            if !is_session_alive(&session) {
                dead_names.push(session.lock().name.clone());
                continue;
            }

            let guard = session.lock();
            result.push(Session {
                name: guard.name.clone(),
                created: guard.created,
                attached: false,
            });
        }

        for name in dead_names {
            let _ = Self::kill(&name);
        }

        result
    }

    /// Send raw input bytes to a target.
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes cannot be accepted into the target input queue.
    pub fn send_input(target: &str, data: &[u8]) -> Result<()> {
        enqueue_to_target(target, data)
    }

    /// Rename a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot be renamed.
    pub fn rename(old_name: &str, new_name: &str) -> Result<()> {
        if old_name == new_name {
            return Ok(());
        }

        let session = {
            let mut state = global_state().lock();
            state
                .sessions
                .remove(old_name)
                .context(format!("Session '{old_name}' not found"))?
        };

        let new_name = new_name.to_string();
        let root = {
            let mut guard = session.lock();
            guard.name.clone_from(&new_name);
            guard.windows.first().cloned()
        };

        if let Some(root) = root {
            let mut guard = root.lock();
            guard.name.clone_from(&new_name);
        }

        {
            let mut state = global_state().lock();
            state.sessions.insert(new_name, session);
        }
        Ok(())
    }

    /// Create a new window in an existing session.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be created.
    pub fn create_window(
        session: &str,
        window_name: &str,
        working_dir: &Path,
        command: Option<&[String]>,
    ) -> Result<u32> {
        debug!(session, window_name, ?working_dir, "Creating mux window");

        let session_ref = {
            let state = global_state().lock();
            state
                .sessions
                .get(session)
                .cloned()
                .context(format!("Session '{session}' not found"))?
        };

        let window_count = session_ref.lock().windows.len();

        let index = u32::try_from(window_count).context("Mux session has too many windows")?;

        let window = spawn_window(index, window_name, working_dir, command, default_pty_size())?;

        {
            let mut guard = session_ref.lock();
            guard.windows.push(window);
        }

        info!(
            session,
            window_name,
            window_index = index,
            "Mux window created"
        );
        Ok(index)
    }

    /// Kill a specific window in a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be killed.
    pub fn kill_window(session: &str, window_index: u32) -> Result<()> {
        let session_ref = {
            let state = global_state().lock();
            state
                .sessions
                .get(session)
                .cloned()
                .context(format!("Session '{session}' not found"))?
        };

        let window = {
            let mut guard = session_ref.lock();
            let idx = window_index as usize;
            if idx >= guard.windows.len() {
                bail!("Window '{window_index}' not found");
            }
            guard.windows.remove(idx)
        };

        if let Err(err) = kill_window_handle(&window) {
            warn!(error = %err, "Failed to kill mux window");
        }

        renumber_windows(&session_ref);
        Ok(())
    }

    /// List all windows in a session with their indices and names.
    ///
    /// # Errors
    ///
    /// Returns an error if the windows cannot be listed.
    pub fn list_windows(session: &str) -> Result<Vec<Window>> {
        let session_ref = {
            let state = global_state().lock();
            state
                .sessions
                .get(session)
                .cloned()
                .context(format!("Session '{session}' not found"))?
        };

        let windows = {
            let guard = session_ref.lock();
            guard.windows.clone()
        };

        Ok(windows
            .into_iter()
            .map(|window| {
                let window = window.lock();
                Window {
                    index: window.index,
                    name: window.name.clone(),
                }
            })
            .collect())
    }

    /// List pane PIDs for a session.
    ///
    /// # Errors
    ///
    /// Returns an error if panes cannot be listed.
    pub fn list_pane_pids(session: &str) -> Result<Vec<u32>> {
        let session_ref = {
            let state = global_state().lock();
            state
                .sessions
                .get(session)
                .cloned()
                .context(format!("Session '{session}' not found"))?
        };

        let windows = {
            let guard = session_ref.lock();
            guard.windows.clone()
        };

        let mut pids = Vec::new();
        for window in windows {
            let window = window.lock();
            if let Some(pid) = window.child.process_id()
                && pid != 0
            {
                pids.push(pid);
            }
        }
        Ok(pids)
    }

    /// Resize a window to specific dimensions.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be resized.
    pub fn resize_window(target: &str, width: u16, height: u16) -> Result<()> {
        let window = super::super::backend::resolve_window(target)?;
        let size = portable_pty::PtySize {
            rows: height,
            cols: width,
            pixel_width: 0,
            pixel_height: 0,
        };

        {
            let mut guard = window.lock();
            guard.master.resize(size).context("Failed to resize PTY")?;
            guard.size = size;
            guard.parser.screen_mut().set_size(size.rows, size.cols);
        }
        Ok(())
    }

    /// Rename a window in a session.
    ///
    /// # Errors
    ///
    /// Returns an error if the window cannot be renamed.
    pub fn rename_window(session: &str, window_index: u32, new_name: &str) -> Result<()> {
        let session_ref = {
            let state = global_state().lock();
            state
                .sessions
                .get(session)
                .cloned()
                .context(format!("Session '{session}' not found"))?
        };

        let window = {
            let guard = session_ref.lock();
            let idx = window_index as usize;
            guard
                .windows
                .get(idx)
                .cloned()
                .context(format!("Window '{window_index}' not found"))?
        };

        {
            let mut guard = window.lock();
            guard.name = new_name.to_string();
        }
        Ok(())
    }
}

/// Information about a session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Session name.
    pub name: String,
    /// Unix timestamp of when the session was created.
    pub created: i64,
    /// Whether a client is attached to this session.
    pub attached: bool,
}

/// Information about a window.
#[derive(Debug, Clone)]
pub struct Window {
    /// Window index.
    pub index: u32,
    /// Window name.
    pub name: String,
}

fn enqueue_to_target(target: &str, payload: &[u8]) -> Result<()> {
    let window = super::super::backend::resolve_window(target)?;
    let input = {
        let guard = window.lock();
        guard.input.clone()
    };

    if let Err(err) = input.enqueue(payload) {
        bail!("failed to queue input for '{target}': {err}");
    }

    Ok(())
}

fn kill_window_handle(
    window: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxWindow>>,
) -> Result<()> {
    let input = {
        let guard = window.lock();
        guard.input.clone()
    };
    input.close();

    {
        let mut guard = window.lock();
        guard
            .child
            .kill()
            .context("Failed to terminate PTY child")?;
    }
    Ok(())
}

fn renumber_windows(
    session: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxSession>>,
) {
    let guard = session.lock();
    for (idx, window) in guard.windows.iter().enumerate() {
        let mut window = window.lock();
        window.index = u32::try_from(idx).unwrap_or(u32::MAX);
    }
}

fn is_session_alive(
    session: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxSession>>,
) -> bool {
    const ROOT_RESTART_MAX_ATTEMPTS: u32 = 3;
    const ROOT_RESTART_COOLDOWN_SECS: i64 = 1;

    let (session_name, windows, root_restart_attempts, last_root_restart) = {
        let guard = session.lock();
        (
            guard.name.clone(),
            guard.windows.clone(),
            guard.root_restart_attempts,
            guard.last_root_restart,
        )
    };

    let Some(root) = windows.first().cloned() else {
        return false;
    };

    let root_alive = window_is_alive(&root);
    if root_alive {
        return true;
    }

    let non_root_alive = windows.iter().skip(1).any(window_is_alive);
    if non_root_alive {
        if should_restart_root_window(
            root_restart_attempts,
            unix_timestamp(),
            last_root_restart,
            ROOT_RESTART_MAX_ATTEMPTS,
            ROOT_RESTART_COOLDOWN_SECS,
        ) {
            restart_root_window(session, &session_name);
        }
        return true;
    }

    let now = unix_timestamp();
    if !should_restart_root_window(
        root_restart_attempts,
        now,
        last_root_restart,
        ROOT_RESTART_MAX_ATTEMPTS,
        ROOT_RESTART_COOLDOWN_SECS,
    ) {
        return false;
    }

    restart_root_window(session, &session_name)
}

fn window_is_alive(
    window: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxWindow>>,
) -> bool {
    let mut guard = window.lock();
    match guard.child.try_wait() {
        Ok(None) | Err(_) => true,
        Ok(Some(_)) => false,
    }
}

const fn should_restart_root_window(
    attempts: u32,
    now: i64,
    last_restart: i64,
    max_attempts: u32,
    cooldown_secs: i64,
) -> bool {
    if attempts >= max_attempts {
        return false;
    }

    let since_last = now.saturating_sub(last_restart);
    since_last >= cooldown_secs
}

fn restart_root_window(
    session: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxSession>>,
    session_name: &str,
) -> bool {
    if !session_still_registered(session_name, session) {
        return false;
    }

    let (working_dir, command, size) = {
        let root = {
            let guard = session.lock();
            guard.windows.first().cloned()
        };

        let Some(root) = root else {
            return false;
        };

        let guard = root.lock();
        (guard.working_dir.clone(), guard.command.clone(), guard.size)
    };

    let new_root = match spawn_window(
        0,
        session_name,
        &working_dir,
        if command.is_empty() {
            None
        } else {
            Some(&command)
        },
        size,
    ) {
        Ok(window) => window,
        Err(err) => {
            warn!(
                session = session_name,
                error = %err,
                "Failed to restart root mux window"
            );
            return false;
        }
    };

    let now = unix_timestamp();
    apply_root_restart(session, session_name, new_root, now)
}

fn apply_root_restart(
    session: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxSession>>,
    session_name: &str,
    new_root: std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxWindow>>,
    now: i64,
) -> bool {
    if !session_still_registered(session_name, session) {
        let _ = kill_window_handle(&new_root);
        return false;
    }

    {
        let mut guard = session.lock();
        if guard.windows.is_empty() {
            let _ = kill_window_handle(&new_root);
            return false;
        }

        guard.windows[0] = new_root;
        guard.root_restart_attempts = guard.root_restart_attempts.saturating_add(1);
        guard.last_root_restart = now;
    }

    info!(session = session_name, "Restarted root mux window");
    true
}

fn session_still_registered(
    session_name: &str,
    session: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxSession>>,
) -> bool {
    let state = global_state().lock();
    state
        .sessions
        .get(session_name)
        .is_some_and(|stored| std::sync::Arc::ptr_eq(stored, session))
}
