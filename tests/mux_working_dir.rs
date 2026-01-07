//! Regression test: mux sessions must start in the caller's CWD.

use std::path::Path;
use tenex::mux::{OutputCapture, SessionManager};

struct DirGuard {
    original_dir: std::path::PathBuf,
}

impl DirGuard {
    fn new() -> std::io::Result<Self> {
        Ok(Self {
            original_dir: std::env::current_dir()?,
        })
    }
}

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_dir);
    }
}

#[test]
fn test_mux_create_session_resolves_relative_working_dir_from_client_cwd()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = DirGuard::new()?;

    // Use a unique socket so we can control the mux daemon's starting directory.
    let socket = format!("tenex-mux-cwd-test-{}", std::process::id());
    let _ = tenex::mux::set_socket_override(&socket);

    let daemon_cwd = tempfile::tempdir()?;
    let client_cwd = tempfile::tempdir()?;

    // Start the mux daemon from `daemon_cwd`.
    std::env::set_current_dir(daemon_cwd.path())?;
    let manager = SessionManager::new();
    let _ = manager.exists("nonexistent-session");
    assert!(
        tenex::mux::is_server_running(),
        "Expected mux daemon to be running"
    );

    // Now create a session while our process is in `client_cwd`, but pass a *relative* working
    // directory. The mux should resolve it relative to this process (not the daemon).
    std::env::set_current_dir(client_cwd.path())?;
    let session_name = format!("cwd-session-{}", std::process::id());

    let _ = manager.kill(&session_name);

    let marker = format!("__tenex_cwd_test_{session_name}__");
    let script = format!("echo {marker}; pwd -P; sleep 60");
    let command = vec!["sh".to_string(), "-c".to_string(), script];

    manager.create(&session_name, Path::new("."), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    let capture = OutputCapture::new();
    let output = capture.capture_pane_with_history(&session_name, 200)?;

    manager.kill(&session_name)?;

    let expected = client_cwd
        .path()
        .canonicalize()
        .unwrap_or_else(|_| client_cwd.path().to_path_buf());
    let expected = expected.to_string_lossy();
    assert!(
        output.contains(expected.as_ref()),
        "Expected mux session to start in {expected}, got output: {output:?}"
    );

    Ok(())
}
