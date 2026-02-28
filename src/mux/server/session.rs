//! PTY-backed session management (server-side).

use anyhow::{Context, Result, bail};
use std::io::Write;
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
                .ok_or_else(|| anyhow::anyhow!("Session '{name}' not found"))?
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
            if let Err(err) = Self::kill(&name)
                && !err.to_string().contains("not found")
            {
                warn!(session = name, error = %err, "Failed to cleanup dead mux session");
            }
        }

        result
    }

    /// Send raw input bytes to a target.
    ///
    /// # Errors
    ///
    /// Returns an error if the bytes cannot be written.
    pub fn send_input(target: &str, data: &[u8]) -> Result<()> {
        write_to_target(target, data)
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
                .ok_or_else(|| anyhow::anyhow!("Session '{old_name}' not found"))?
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
                .ok_or_else(|| anyhow::anyhow!("Session '{session}' not found"))?
        };

        let index = {
            let guard = session_ref.lock();
            u32::try_from(guard.windows.len()).map_or(u32::MAX, |value| value)
        };

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
                .ok_or_else(|| anyhow::anyhow!("Session '{session}' not found"))?
        };

        let window = {
            let mut guard = session_ref.lock();
            let idx = usize::try_from(window_index).context("Invalid window index")?;
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
                .ok_or_else(|| anyhow::anyhow!("Session '{session}' not found"))?
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
                .ok_or_else(|| anyhow::anyhow!("Session '{session}' not found"))?
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
                .ok_or_else(|| anyhow::anyhow!("Session '{session}' not found"))?
        };

        let window = {
            let guard = session_ref.lock();
            let idx = usize::try_from(window_index).context("Invalid window index")?;
            guard
                .windows
                .get(idx)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Window '{window_index}' not found"))?
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

fn write_to_target(target: &str, payload: &[u8]) -> Result<()> {
    let window = super::super::backend::resolve_window(target)?;
    {
        let mut guard = window.lock();
        guard
            .writer
            .write_all(payload)
            .context("Failed to write to PTY")?;
        guard.writer.flush().context("Failed to flush PTY writer")?;
    }
    Ok(())
}

fn kill_window_handle(
    window: &std::sync::Arc<parking_lot::Mutex<super::super::backend::MuxWindow>>,
) -> Result<()> {
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
        window.index = u32::try_from(idx).map_or(u32::MAX, |value| value);
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

    if !session_still_registered(session_name, session) {
        let _ = kill_window_handle(&new_root);
        return false;
    }

    let now = unix_timestamp();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn test_command() -> Vec<String> {
        // Use a long-running process so tests don't race with natural process exit.
        #[cfg(windows)]
        {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 60".to_string(),
            ]
        }
        #[cfg(not(windows))]
        {
            vec!["sh".to_string(), "-c".to_string(), "sleep 60".to_string()]
        }
    }

    fn test_long_command() -> Vec<String> {
        #[cfg(windows)]
        {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 10".to_string(),
            ]
        }
        #[cfg(not(windows))]
        {
            vec!["sh".to_string(), "-c".to_string(), "sleep 10".to_string()]
        }
    }

    fn test_exit_command() -> Vec<String> {
        #[cfg(windows)]
        {
            vec!["cmd".to_string(), "/c".to_string(), "exit 0".to_string()]
        }
        #[cfg(not(windows))]
        {
            vec!["sh".to_string(), "-c".to_string(), "exit 0".to_string()]
        }
    }

    fn test_update_like_root_command(marker_path: &Path) -> Vec<String> {
        #[cfg(windows)]
        {
            let marker = marker_path.display().to_string();
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!(
                    "$marker = '{marker}'; if (-not (Test-Path $marker)) {{ New-Item -ItemType File -Path $marker -Force | Out-Null; exit 0 }} else {{ Start-Sleep -Seconds 60 }}"
                ),
            ]
        }
        #[cfg(not(windows))]
        {
            vec![
                "sh".to_string(),
                "-c".to_string(),
                format!(
                    "if [ ! -f '{marker}' ]; then touch '{marker}'; exit 0; else sleep 60; fi",
                    marker = marker_path.display()
                ),
            ]
        }
    }

    #[test]
    fn test_session_manager_new() {
        let manager = Manager;
        assert!(!format!("{manager:?}").is_empty());
    }

    #[test]
    fn test_create_kill_session() {
        let session_name = "tenex-test-create-kill";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);

        let command = test_command();
        let result = Manager::create(session_name, &tmp, Some(&command));
        assert!(result.is_ok());

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_create_duplicate_session() {
        let session_name = "tenex-test-duplicate";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);

        let command = test_command();
        let result = Manager::create(session_name, &tmp, Some(&command));
        assert!(result.is_ok());

        let result2 = Manager::create(session_name, &tmp, Some(&command));
        assert!(result2.is_err());

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_rename_session() {
        let old_name = "tenex-test-rename-old";
        let new_name = "tenex-test-rename-new";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(old_name);
        let _ = Manager::kill(new_name);

        let command = test_command();
        let result = Manager::create(old_name, &tmp, Some(&command));
        assert!(result.is_ok());

        let rename_result = Manager::rename(old_name, new_name);
        assert!(rename_result.is_ok());
        assert!(!Manager::exists(old_name));
        assert!(Manager::exists(new_name));

        let _ = Manager::kill(new_name);
    }

    #[test]
    fn test_window_ops_and_renumbering() -> Result<()> {
        let session_name = "tenex-test-window-ops";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command()))?;

        let w1 = Manager::create_window(session_name, "w1", &tmp, Some(&test_command()))?;
        let w2 = Manager::create_window(session_name, "w2", &tmp, Some(&test_command()))?;
        assert_eq!(w1, 1);
        assert_eq!(w2, 2);

        Manager::rename_window(session_name, w2, "renamed")?;
        let windows = Manager::list_windows(session_name)?;
        assert!(windows.iter().any(|w| w.name == "renamed"));

        // Remove the middle window and ensure indices are renumbered.
        Manager::kill_window(session_name, w1)?;
        let windows = Manager::list_windows(session_name)?;
        let indices = windows.iter().map(|w| w.index).collect::<Vec<_>>();
        assert_eq!(indices, vec![0, 1]);

        let _ = Manager::kill(session_name);
        Ok(())
    }

    #[test]
    fn test_error_paths() {
        assert!(Manager::kill("tenex-test-nope").is_err());
        assert!(Manager::rename("tenex-test-nope", "tenex-test-new").is_err());
        assert!(Manager::list_windows("tenex-test-nope").is_err());
        assert!(
            Manager::create_window(
                "tenex-test-nope",
                "w",
                &std::env::temp_dir(),
                Some(&test_command())
            )
            .is_err()
        );
        assert!(Manager::kill_window("tenex-test-nope", 1).is_err());
        assert!(Manager::rename_window("tenex-test-nope", 1, "x").is_err());
        assert!(Manager::resize_window("tenex-test-nope", 80, 24).is_err());
        assert!(Manager::send_input("tenex-test-nope", b"").is_err());
    }

    #[test]
    fn test_list_pane_pids_success() -> Result<()> {
        let session_name = "tenex-test-list-pids";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command()))?;

        let pids = Manager::list_pane_pids(session_name)?;
        assert!(!pids.is_empty());

        Manager::kill(session_name)?;
        Ok(())
    }

    #[test]
    fn test_send_input_success() -> Result<()> {
        let session_name = "tenex-test-send-input";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command()))?;

        Manager::send_input(session_name, b"echo tenex\n")?;

        Manager::kill(session_name)?;
        Ok(())
    }

    #[test]
    fn test_resize_window_success() -> Result<()> {
        let session_name = "tenex-test-resize-window";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command()))?;

        Manager::resize_window(session_name, 80, 24)?;

        Manager::kill(session_name)?;
        Ok(())
    }

    #[test]
    fn test_session_alive_when_root_exits_but_child_still_running() -> Result<()> {
        let session_name = "tenex-test-root-exits-child-alive";
        let tmp = std::env::temp_dir();

        if let Err(err) = Manager::kill(session_name)
            && !err.to_string().contains("not found")
        {
            return Err(err);
        }

        Manager::create(session_name, &tmp, Some(&test_exit_command()))?;
        Manager::create_window(session_name, "child", &tmp, Some(&test_long_command()))?;

        // Give the root window time to exit.
        std::thread::sleep(Duration::from_millis(200));

        let child_running = {
            let session_ref = {
                let state = global_state().lock();
                state.sessions.get(session_name).cloned()
            };

            let session_ref = session_ref.ok_or_else(|| anyhow::anyhow!("Session vanished"))?;
            let child_window = {
                let guard = session_ref.lock();
                guard
                    .windows
                    .get(1)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Child window missing"))?
            };

            let mut guard = child_window.lock();
            match guard.child.try_wait() {
                Ok(None) | Err(_) => true,
                Ok(Some(_)) => false,
            }
        };
        assert!(
            child_running,
            "Expected child window process to still be running"
        );

        assert!(Manager::exists(session_name));

        assert!(
            Manager::list().iter().any(|s| s.name == session_name),
            "Expected session list to include {session_name} after root exit"
        );

        let pids = Manager::list_pane_pids(session_name)?;
        assert!(
            !pids.is_empty(),
            "Expected at least one pane PID after root exit"
        );

        if let Err(err) = Manager::kill(session_name)
            && !err.to_string().contains("not found")
        {
            return Err(err);
        }
        Ok(())
    }

    #[test]
    fn test_root_window_restarts_when_root_exits_without_children() -> Result<()> {
        let session_name = "tenex-test-root-restart-grace";
        let tmp = TempDir::new()?;
        let marker = tmp.path().join("updated");

        if let Err(err) = Manager::kill(session_name)
            && !err.to_string().contains("not found")
        {
            return Err(err);
        }

        Manager::create(
            session_name,
            tmp.path(),
            Some(&test_update_like_root_command(&marker)),
        )?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let _ = Manager::exists(session_name);

            let restarted = {
                let state = global_state().lock();
                state
                    .sessions
                    .get(session_name)
                    .is_some_and(|session_ref| session_ref.lock().root_restart_attempts >= 1)
            };

            if restarted {
                break;
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(marker.exists(), "Expected restart marker to be created");
        assert!(
            Manager::exists(session_name),
            "Expected session to remain alive after root restart"
        );

        let restarted = {
            let state = global_state().lock();
            state
                .sessions
                .get(session_name)
                .is_some_and(|session_ref| session_ref.lock().root_restart_attempts >= 1)
        };
        assert!(
            restarted,
            "Expected root window to be restarted at least once"
        );

        if let Err(err) = Manager::kill(session_name)
            && !err.to_string().contains("not found")
        {
            return Err(err);
        }

        Ok(())
    }
}
