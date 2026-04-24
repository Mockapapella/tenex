//! PTY-backed session management (server-side).

use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::Path;
use tracing::{debug, info, warn};

use super::super::backend::{default_pty_size, global_state, spawn_window, unix_timestamp};

#[cfg(any(test, coverage))]
thread_local! {
    static FORCE_SESSION_WINDOW_INDEX_OVERFLOW: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
fn with_forced_session_window_index_overflow_for_tests<T>(f: impl FnOnce() -> T) -> T {
    FORCE_SESSION_WINDOW_INDEX_OVERFLOW.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

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
        #[cfg(any(test, coverage))]
        let window_count = if FORCE_SESSION_WINDOW_INDEX_OVERFLOW.with(std::cell::Cell::get) {
            (u32::MAX as usize).saturating_add(1)
        } else {
            window_count
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::{Child, ChildKiller, MasterPty};
    use std::io;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[derive(Debug, Clone, Copy)]
    enum TryWaitBehavior {
        Running,
        Exited,
        Error,
    }

    #[derive(Debug, Clone, Copy)]
    enum KillBehavior {
        Ok,
        Error,
    }

    #[derive(Debug, Clone)]
    struct StubChild {
        try_wait: TryWaitBehavior,
        kill: KillBehavior,
        pid: Option<u32>,
    }

    impl portable_pty::ChildKiller for StubChild {
        fn kill(&mut self) -> io::Result<()> {
            match self.kill {
                KillBehavior::Ok => Ok(()),
                KillBehavior::Error => Err(io::Error::other("kill failed")),
            }
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(Self {
                try_wait: self.try_wait,
                kill: self.kill,
                pid: self.pid,
            })
        }
    }

    impl portable_pty::Child for StubChild {
        fn try_wait(&mut self) -> io::Result<Option<portable_pty::ExitStatus>> {
            match self.try_wait {
                TryWaitBehavior::Running => Ok(None),
                TryWaitBehavior::Exited => Ok(Some(portable_pty::ExitStatus::with_exit_code(0))),
                TryWaitBehavior::Error => Err(io::Error::other("try_wait failed")),
            }
        }

        fn wait(&mut self) -> io::Result<portable_pty::ExitStatus> {
            Ok(portable_pty::ExitStatus::with_exit_code(0))
        }

        fn process_id(&self) -> Option<u32> {
            self.pid
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    #[derive(Debug)]
    struct StubWriter {
        write_ok: bool,
        flush_ok: bool,
    }

    impl io::Write for StubWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.write_ok {
                Ok(buf.len())
            } else {
                Err(io::Error::other("write failed"))
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.flush_ok {
                Ok(())
            } else {
                Err(io::Error::other("flush failed"))
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct FailingMasterPty;

    impl MasterPty for FailingMasterPty {
        fn resize(&self, _size: portable_pty::PtySize) -> Result<(), anyhow::Error> {
            Err(anyhow::anyhow!("resize failed"))
        }

        fn get_size(&self) -> Result<portable_pty::PtySize, anyhow::Error> {
            Ok(portable_pty::PtySize::default())
        }

        fn try_clone_reader(&self) -> Result<Box<dyn std::io::Read + Send>, anyhow::Error> {
            Err(anyhow::anyhow!("reader unavailable"))
        }

        fn take_writer(&self) -> Result<Box<dyn std::io::Write + Send>, anyhow::Error> {
            Err(anyhow::anyhow!("writer unavailable"))
        }

        #[cfg(unix)]
        fn process_group_leader(&self) -> Option<libc::pid_t> {
            None
        }

        #[cfg(unix)]
        fn as_raw_fd(&self) -> Option<std::os::unix::prelude::RawFd> {
            None
        }

        #[cfg(unix)]
        fn tty_name(&self) -> Option<std::path::PathBuf> {
            None
        }
    }

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
        let _guard = crate::test_support::lock_mux_test_environment();
        let manager = Manager;
        assert!(!format!("{manager:?}").is_empty());
    }

    #[test]
    fn test_create_kill_session() {
        let _guard = crate::test_support::lock_mux_test_environment();
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
        let _guard = crate::test_support::lock_mux_test_environment();
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
        let _guard = crate::test_support::lock_mux_test_environment();
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
    fn test_window_ops_and_renumbering() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-window-ops";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let w1 = Manager::create_window(session_name, "w1", &tmp, Some(&test_command())).unwrap();
        let w2 = Manager::create_window(session_name, "w2", &tmp, Some(&test_command())).unwrap();
        assert_eq!(w1, 1);
        assert_eq!(w2, 2);

        Manager::rename_window(session_name, w2, "renamed").unwrap();
        let windows = Manager::list_windows(session_name).unwrap();
        assert!(windows.iter().any(|w| w.name == "renamed"));

        // Remove the middle window and ensure indices are renumbered.
        Manager::kill_window(session_name, w1).unwrap();
        let windows = Manager::list_windows(session_name).unwrap();
        let indices = windows.iter().map(|w| w.index).collect::<Vec<_>>();
        assert_eq!(indices, vec![0, 1]);

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_error_paths() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let tmp = std::env::temp_dir();
        assert!(Manager::kill("tenex-test-nope").is_err());
        assert!(Manager::rename("tenex-test-nope", "tenex-test-new").is_err());
        assert!(Manager::list_windows("tenex-test-nope").is_err());
        assert!(Manager::list_pane_pids("tenex-test-nope").is_err());
        assert!(Manager::rename_window("tenex-test-nope", 1, "x").is_err());
        assert!(
            Manager::create_window("tenex-test-nope", "w", &tmp, Some(&test_command())).is_err()
        );
        assert!(Manager::kill_window("tenex-test-nope", 1).is_err());
        assert!(Manager::resize_window("tenex-test-nope", 80, 24).is_err());
        assert!(Manager::send_input("tenex-test-nope", b"").is_err());

        let empty_argv: Vec<String> = Vec::new();
        assert!(Manager::create("tenex-test-create-empty-argv", &tmp, Some(&empty_argv)).is_err());

        let session_name = "tenex-test-create-window-empty-argv";
        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();
        assert!(Manager::create_window(session_name, "w", &tmp, Some(&empty_argv)).is_err());
        assert!(Manager::rename_window(session_name, 999, "x").is_err());
        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_create_window_errors_when_session_window_index_overflows_u32() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-window-index-overflow";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        with_forced_session_window_index_overflow_for_tests(|| {
            let err = Manager::create_window(session_name, "w", &tmp, Some(&test_command()))
                .unwrap_err();
            assert!(err.to_string().contains("Mux session has too many windows"));
        });

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_list_pane_pids_success() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-list-pids";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let pids = Manager::list_pane_pids(session_name).unwrap();
        assert!(!pids.is_empty());

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        let window = {
            let guard = session_ref.lock();
            let window = guard.windows.first().cloned().unwrap();
            drop(guard);
            window
        };
        {
            let mut window = window.lock();
            let _ = window.child.kill();
            window.child = Box::new(StubChild {
                try_wait: TryWaitBehavior::Running,
                kill: KillBehavior::Ok,
                pid: None,
            });
        }
        assert!(Manager::list_pane_pids(session_name).unwrap().is_empty());
        {
            let mut window = window.lock();
            window.child = Box::new(StubChild {
                try_wait: TryWaitBehavior::Running,
                kill: KillBehavior::Ok,
                pid: Some(0),
            });
        }
        assert!(Manager::list_pane_pids(session_name).unwrap().is_empty());

        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_send_input_success() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-send-input";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        Manager::send_input(session_name, b"echo tenex\n").unwrap();

        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_resize_window_success() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-resize-window";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        Manager::resize_window(session_name, 80, 24).unwrap();

        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_session_alive_when_root_exits_but_child_still_running() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-root-exits-child-alive";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);

        Manager::create(session_name, &tmp, Some(&test_exit_command())).unwrap();
        Manager::create_window(session_name, "child", &tmp, Some(&test_long_command())).unwrap();

        // Give the root window time to exit.
        std::thread::sleep(Duration::from_millis(200));

        let child_running = {
            let session_ref = {
                let state = global_state().lock();
                state.sessions.get(session_name).cloned()
            };

            let session_ref = session_ref.expect("Session vanished");
            let child_window = {
                let guard = session_ref.lock();
                guard.windows.get(1).cloned().expect("Child window missing")
            };

            window_is_alive(&child_window)
        };
        assert!(child_running);

        // Keep this test focused on child liveness; root restart behavior is covered separately.
        {
            let session_ref = {
                let state = global_state().lock();
                state.sessions.get(session_name).cloned()
            };
            let session_ref = session_ref.expect("Session vanished");
            let mut guard = session_ref.lock();
            guard.root_restart_attempts = u32::MAX;
            guard.last_root_restart = unix_timestamp();
        }

        assert!(Manager::exists(session_name));

        assert!(Manager::list().iter().any(|s| s.name == session_name));

        let pids = Manager::list_pane_pids(session_name).unwrap();
        assert!(!pids.is_empty());

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_root_window_restarts_when_root_exits_without_children() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-root-restart-grace";
        let tmp = TempDir::new().unwrap();
        let marker = tmp.path().join("updated");

        let _ = Manager::kill(session_name);

        Manager::create(
            session_name,
            tmp.path(),
            Some(&test_update_like_root_command(&marker)),
        )
        .unwrap();

        let mut deadline = Instant::now() + Duration::from_secs(5);
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
                // Exit via the loop condition rather than `break` so both outcomes of the
                // `Instant::now() < deadline` check are exercised for coverage.
                deadline = Instant::now();
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        assert!(marker.exists());
        assert!(Manager::exists(session_name));

        let restarted = {
            let state = global_state().lock();
            state
                .sessions
                .get(session_name)
                .is_some_and(|session_ref| session_ref.lock().root_restart_attempts >= 1)
        };
        assert!(restarted);

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_list_cleans_up_dead_sessions() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let dead_name = "tenex-test-dead-session";
        let alive_name = "tenex-test-alive-session";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(dead_name);
        let _ = Manager::kill(alive_name);
        Manager::create(alive_name, &tmp, Some(&test_long_command())).unwrap();
        {
            let mut state = global_state().lock();
            state.sessions.insert(
                dead_name.to_string(),
                std::sync::Arc::new(parking_lot::Mutex::new(crate::mux::backend::MuxSession {
                    name: dead_name.to_string(),
                    created: unix_timestamp(),
                    root_restart_attempts: 0,
                    last_root_restart: 0,
                    windows: Vec::new(),
                })),
            );
        }

        let sessions = Manager::list();
        assert!(sessions.iter().all(|session| session.name != dead_name));
        assert!(sessions.iter().any(|session| session.name == alive_name));

        let state = global_state().lock();
        assert!(!state.sessions.contains_key(dead_name));
        drop(state);
        let _ = Manager::kill(alive_name);
    }

    #[test]
    fn test_rename_noops_when_names_match() {
        let _guard = crate::test_support::lock_mux_test_environment();
        assert!(Manager::rename("tenex-test-same", "tenex-test-same").is_ok());
    }

    #[test]
    fn test_rename_session_with_empty_windows_updates_state() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let old_name = "tenex-test-rename-empty-windows-old";
        let new_name = "tenex-test-rename-empty-windows-new";

        let _ = Manager::kill(old_name);
        let _ = Manager::kill(new_name);

        let session_ref =
            std::sync::Arc::new(parking_lot::Mutex::new(crate::mux::backend::MuxSession {
                name: old_name.to_string(),
                created: unix_timestamp(),
                root_restart_attempts: 0,
                last_root_restart: 0,
                windows: Vec::new(),
            }));
        {
            let mut state = global_state().lock();
            state
                .sessions
                .insert(old_name.to_string(), session_ref.clone());
        }

        Manager::rename(old_name, new_name).unwrap();

        {
            let state = global_state().lock();
            assert!(!state.sessions.contains_key(old_name));
            assert!(state.sessions.contains_key(new_name));
            drop(state);
        }
        assert_eq!(session_ref.lock().name, new_name);

        Manager::kill(new_name).unwrap();
    }

    #[test]
    fn test_kill_window_errors_for_unknown_index() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-kill-window-invalid-index";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let result = Manager::kill_window(session_name, 42);
        assert!(result.is_err());

        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_send_input_propagates_write_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-send-input-write-error";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        let window = {
            let guard = session_ref.lock();
            let window = guard.windows.first().cloned().unwrap();
            drop(guard);
            window
        };
        {
            let mut window = window.lock();
            window.writer = Box::new(StubWriter {
                write_ok: false,
                flush_ok: true,
            });
        }

        let err = Manager::send_input(session_name, b"ignored").unwrap_err();
        assert!(format!("{err:?}").contains("Failed to write to PTY"));

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_send_input_propagates_flush_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-send-input-flush-error";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        let window = {
            let guard = session_ref.lock();
            let window = guard.windows.first().cloned().unwrap();
            drop(guard);
            window
        };
        {
            let mut window = window.lock();
            window.writer = Box::new(StubWriter {
                write_ok: true,
                flush_ok: false,
            });
        }

        let err = Manager::send_input(session_name, b"ignored").unwrap_err();
        assert!(format!("{err:?}").contains("Failed to flush PTY writer"));

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_resize_window_propagates_resize_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-resize-window-error";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        let window = {
            let guard = session_ref.lock();
            let window = guard.windows.first().cloned().unwrap();
            drop(guard);
            window
        };
        {
            let mut window = window.lock();
            window.master = Box::new(FailingMasterPty);
        }

        let err = Manager::resize_window(session_name, 80, 24).unwrap_err();
        assert!(format!("{err:?}").contains("Failed to resize PTY"));

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_exists_returns_false_when_session_has_no_windows() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-exists-no-windows";

        let _ = Manager::kill(session_name);
        {
            let mut state = global_state().lock();
            state.sessions.insert(
                session_name.to_string(),
                std::sync::Arc::new(parking_lot::Mutex::new(crate::mux::backend::MuxSession {
                    name: session_name.to_string(),
                    created: unix_timestamp(),
                    root_restart_attempts: 0,
                    last_root_restart: 0,
                    windows: Vec::new(),
                })),
            );
        }

        assert!(!Manager::exists(session_name));
        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_root_restart_happens_when_root_dead_and_child_alive() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-root-restart-with-child";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_exit_command())).unwrap();
        Manager::create_window(session_name, "child", &tmp, Some(&test_long_command())).unwrap();

        std::thread::sleep(Duration::from_millis(200));

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        {
            let mut guard = session_ref.lock();
            guard.root_restart_attempts = 0;
            guard.last_root_restart = 0;
        }

        assert!(Manager::exists(session_name));
        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_exists_returns_false_when_root_dead_and_restart_disallowed() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-root-dead-no-restart";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_exit_command())).unwrap();
        std::thread::sleep(Duration::from_millis(200));

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        {
            let mut guard = session_ref.lock();
            guard.root_restart_attempts = 3;
            guard.last_root_restart = unix_timestamp();
        }

        assert!(!Manager::exists(session_name));
        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_restart_root_window_errors_when_session_unregistered() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-restart-unregistered";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let mut state = global_state().lock();
            state.sessions.remove(session_name).unwrap()
        };

        let ok = restart_root_window(&session_ref, session_name);
        assert!(!ok);

        let windows = { session_ref.lock().windows.clone() };
        for window in windows {
            let _ = kill_window_handle(&window);
        }
    }

    #[test]
    fn test_restart_root_window_returns_false_without_root() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-restart-empty-windows";

        let _ = Manager::kill(session_name);
        let session_ref =
            std::sync::Arc::new(parking_lot::Mutex::new(crate::mux::backend::MuxSession {
                name: session_name.to_string(),
                created: unix_timestamp(),
                root_restart_attempts: 0,
                last_root_restart: 0,
                windows: Vec::new(),
            }));
        {
            let mut state = global_state().lock();
            state
                .sessions
                .insert(session_name.to_string(), session_ref.clone());
        }

        assert!(!restart_root_window(&session_ref, session_name));
        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_restart_root_window_uses_default_command_when_missing() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-restart-empty-command";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, None).unwrap();

        let state = global_state().lock();
        let session_ref = state.sessions.get(session_name).cloned().unwrap();
        drop(state);

        {
            let windows = { session_ref.lock().windows.clone() };
            for window in windows {
                let _ = kill_window_handle(&window);
            }
        }

        assert!(restart_root_window(&session_ref, session_name));
        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_restart_root_window_warns_when_spawn_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-restart-spawn-fails";
        let tmp = TempDir::new().unwrap();

        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _tracing_guard = tracing::subscriber::set_default(subscriber);

        let _ = Manager::kill(session_name);
        Manager::create(session_name, tmp.path(), Some(&test_exit_command())).unwrap();
        std::thread::sleep(Duration::from_millis(200));

        let state = global_state().lock();
        let session_ref = state.sessions.get(session_name).cloned().unwrap();
        drop(state);

        {
            let root = { session_ref.lock().windows.first().cloned().unwrap() };
            root.lock().command = vec!["/tenex-test-missing-binary".to_string()];
        }

        assert!(!restart_root_window(&session_ref, session_name));
        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_kill_window_warns_when_child_kill_fails() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-kill-window-kill-fails";
        let tmp = std::env::temp_dir();

        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _tracing_guard = tracing::subscriber::set_default(subscriber);

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();
        let w1 = Manager::create_window(session_name, "w1", &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        let window = {
            let guard = session_ref.lock();
            let window = guard.windows.get(w1 as usize).cloned().unwrap();
            drop(guard);
            window
        };
        {
            let mut window = window.lock();
            let _ = window.child.kill();
            window.child = Box::new(StubChild {
                try_wait: TryWaitBehavior::Running,
                kill: KillBehavior::Error,
                pid: None,
            });
        }

        Manager::kill_window(session_name, w1).unwrap();
        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_apply_root_restart_returns_false_when_session_removed() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-apply-root-restart-removed";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            state.sessions.get(session_name).cloned().unwrap()
        };

        let new_root = spawn_window(
            0,
            session_name,
            &tmp,
            Some(&test_command()),
            default_pty_size(),
        )
        .unwrap();

        {
            let mut state = global_state().lock();
            state.sessions.remove(session_name);
        }

        assert!(!apply_root_restart(
            &session_ref,
            session_name,
            new_root,
            unix_timestamp()
        ));
        let windows = { session_ref.lock().windows.clone() };
        for window in windows {
            let _ = kill_window_handle(&window);
        }
    }

    #[test]
    fn test_apply_root_restart_returns_false_when_session_has_no_windows() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-apply-root-restart-empty";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        let session_ref =
            std::sync::Arc::new(parking_lot::Mutex::new(crate::mux::backend::MuxSession {
                name: session_name.to_string(),
                created: unix_timestamp(),
                root_restart_attempts: 0,
                last_root_restart: 0,
                windows: Vec::new(),
            }));
        {
            let mut state = global_state().lock();
            state
                .sessions
                .insert(session_name.to_string(), session_ref.clone());
        }

        let new_root = spawn_window(
            0,
            session_name,
            &tmp,
            Some(&test_command()),
            default_pty_size(),
        )
        .unwrap();
        assert!(!apply_root_restart(
            &session_ref,
            session_name,
            new_root,
            unix_timestamp()
        ));

        Manager::kill(session_name).unwrap();
    }

    #[test]
    fn test_apply_root_restart_emits_info_when_tracing_enabled() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-apply-root-restart-tracing";
        let tmp = std::env::temp_dir();

        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .finish();
        let _tracing_guard = tracing::subscriber::set_default(subscriber);

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            state.sessions.get(session_name).cloned().unwrap()
        };

        let new_root = spawn_window(
            0,
            session_name,
            &tmp,
            Some(&test_command()),
            default_pty_size(),
        )
        .unwrap();
        assert!(apply_root_restart(
            &session_ref,
            session_name,
            new_root,
            unix_timestamp()
        ));

        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_stub_child_methods_cover_branches() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let mut child = StubChild {
            try_wait: TryWaitBehavior::Running,
            kill: KillBehavior::Ok,
            pid: Some(42),
        };

        assert_eq!(child.process_id(), Some(42));
        assert!(child.try_wait().unwrap().is_none());
        assert!(child.wait().unwrap().success());

        let mut killer = child.clone_killer();
        killer.kill().unwrap();
    }

    #[test]
    fn test_stub_writer_success_path() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let session_name = "tenex-test-send-input-stub-writer-success";
        let tmp = std::env::temp_dir();

        let _ = Manager::kill(session_name);
        Manager::create(session_name, &tmp, Some(&test_command())).unwrap();

        let session_ref = {
            let state = global_state().lock();
            let session_ref = state.sessions.get(session_name).cloned().unwrap();
            drop(state);
            session_ref
        };
        let window = {
            let guard = session_ref.lock();
            let window = guard.windows.first().cloned().unwrap();
            drop(guard);
            window
        };
        {
            let mut window = window.lock();
            window.writer = Box::new(StubWriter {
                write_ok: true,
                flush_ok: true,
            });
        }

        Manager::send_input(session_name, b"ignored").unwrap();
        let _ = Manager::kill(session_name);
    }

    #[test]
    fn test_failing_master_pty_methods_cover_all_branches() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let master = FailingMasterPty;

        assert_eq!(master.get_size().unwrap(), portable_pty::PtySize::default());
        assert!(master.try_clone_reader().is_err());
        assert!(master.take_writer().is_err());

        #[cfg(unix)]
        {
            assert!(master.process_group_leader().is_none());
            assert!(master.as_raw_fd().is_none());
            assert!(master.tty_name().is_none());
        }
    }

    #[test]
    fn test_window_is_alive_false_when_child_exited() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let tmp = std::env::temp_dir();
        let window = spawn_window(
            0,
            "tenex-test-window-alive-exited",
            &tmp,
            Some(&test_exit_command()),
            default_pty_size(),
        )
        .unwrap();

        {
            let mut guard = window.lock();
            let _ = guard.child.kill();
            guard.child = Box::new(StubChild {
                try_wait: TryWaitBehavior::Exited,
                kill: KillBehavior::Ok,
                pid: None,
            });
        }

        assert!(!window_is_alive(&window));
        kill_window_handle(&window).unwrap();
    }

    #[test]
    fn test_window_is_alive_true_when_try_wait_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let tmp = std::env::temp_dir();
        let window = spawn_window(
            0,
            "tenex-test-window-alive-error",
            &tmp,
            Some(&test_command()),
            default_pty_size(),
        )
        .unwrap();

        {
            let mut guard = window.lock();
            let _ = guard.child.kill();
            guard.child = Box::new(StubChild {
                try_wait: TryWaitBehavior::Error,
                kill: KillBehavior::Ok,
                pid: None,
            });
        }

        assert!(window_is_alive(&window));
        kill_window_handle(&window).unwrap();
    }
}
