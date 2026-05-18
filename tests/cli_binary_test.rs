//! Binary integration tests for CLI commands
//!
//! These tests run the actual tenex binary to exercise the CLI code paths.

use std::fs;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
#[cfg(unix)]
use tempfile::TempDir;

fn tenex_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tenex"))
}

fn write_state_with_one_agent(
    state_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut storage = tenex::agent::Storage::with_path(state_path.to_path_buf());
    storage.instance_id = Some("deadbeef".to_string());

    let mut agent = tenex::Agent::new(
        "Test Agent".to_string(),
        "echo".to_string(),
        "tenex/test-branch".to_string(),
        std::env::temp_dir().join("tenex-test-worktree"),
    );
    agent.mux_session = format!("{}{}", storage.instance_session_prefix(), agent.short_id());
    storage.add(agent);
    storage.save()?;
    Ok(())
}

fn write_empty_state(state_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut storage = tenex::agent::Storage::with_path(state_path.to_path_buf());
    storage.save()?;
    Ok(())
}

#[cfg(unix)]
fn write_fake_docker_script(
    dir: &std::path::Path,
    body: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let script = dir.join("docker");
    fs::write(&script, body)?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms)?;
    Ok(script)
}

#[test]
fn test_cli_help() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("--help").output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Terminal multiplexer"));
    Ok(())
}

#[test]
fn test_cli_version() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("--version").output()?;
    assert!(output.status.success());
    Ok(())
}

#[test]
fn test_cli_version_with_debug_logging_enabled() -> Result<(), Box<dyn std::error::Error>> {
    for level in ["1", "2", "3"] {
        let output = tenex_bin().arg("--version").env("DEBUG", level).output()?;
        assert!(
            output.status.success(),
            "tenex --version failed with DEBUG={level}:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    Ok(())
}

#[test]
fn test_cli_invalid_argument_shows_help() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("--invalid-flag").output()?;

    // Should fail with non-zero exit code
    assert!(!output.status.success());

    // Should show error message on stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));

    // Should show help text on stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    Ok(())
}

#[test]
fn test_cli_invalid_argument_warns_when_print_help_fails() -> Result<(), Box<dyn std::error::Error>>
{
    let mut cmd = tenex_bin();
    cmd.arg("--invalid-flag");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    drop(child.stdout.take());
    let output = child.wait_with_output()?;

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Warning: Failed to print help"));
    Ok(())
}

#[test]
fn test_cli_reset_force() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::NamedTempFile;

    // Use isolated state file to avoid affecting real agents
    let temp_state = NamedTempFile::new()?;
    fs::write(
        temp_state.path(),
        r#"{
  "agents": [],
  "mux_socket": "tenex-mux-stale.sock"
}
"#,
    )?;

    // reset with --force should succeed (even if no agents)
    let output = tenex_bin()
        .args(["reset", "--force"])
        .env("TENEX_STATE_PATH", temp_state.path())
        .env(
            "TENEX_MUX_SOCKET",
            format!("tenex-mux-test-reset-{}", std::process::id()),
        )
        .output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // "No agents to reset", or lists agents/orphaned sessions
    assert!(
        stdout.contains("No agents")
            || stdout.contains("Reset complete")
            || stdout.contains("Agents to kill")
            || stdout.contains("Orphaned")
    );

    let storage = tenex::agent::Storage::load_from(temp_state.path())?;
    assert!(storage.mux_socket.is_none());
    Ok(())
}

#[test]
fn test_cli_reset_scope_accepts_all_instances_numeric() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::NamedTempFile;

    let temp_state = NamedTempFile::new()?;
    write_empty_state(temp_state.path())?;

    let mut child = tenex_bin()
        .args(["reset"])
        .env("TENEX_STATE_PATH", temp_state.path())
        .env(
            "TENEX_MUX_SOCKET",
            format!("tenex-mux-test-reset-scope-numeric-{}", std::process::id()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .take()
        .ok_or("Expected child stdin")?
        .write_all(b"2\n")?;

    let output = child.wait_with_output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Reset scope"));
    assert!(stdout.contains("No agents"));
    Ok(())
}

#[test]
fn test_cli_reset_scope_accepts_all_instances_alias() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::NamedTempFile;

    let temp_state = NamedTempFile::new()?;
    write_empty_state(temp_state.path())?;

    let mut child = tenex_bin()
        .args(["reset"])
        .env("TENEX_STATE_PATH", temp_state.path())
        .env(
            "TENEX_MUX_SOCKET",
            format!("tenex-mux-test-reset-scope-alias-{}", std::process::id()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .take()
        .ok_or("Expected child stdin")?
        .write_all(b"all\n")?;

    let output = child.wait_with_output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Reset scope"));
    assert!(stdout.contains("No agents"));
    Ok(())
}

#[test]
fn test_cli_reset_aborts_when_confirmation_declined() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::NamedTempFile;

    let temp_state = NamedTempFile::new()?;
    write_state_with_one_agent(temp_state.path())?;

    let mut child = tenex_bin()
        .args(["reset"])
        .env("TENEX_STATE_PATH", temp_state.path())
        .env(
            "TENEX_MUX_SOCKET",
            format!("tenex-mux-test-reset-abort-{}", std::process::id()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .take()
        .ok_or("Expected child stdin")?
        .write_all(b"\n\n")?;

    let output = child.wait_with_output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Aborted."));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn test_cli_muxd_exits_when_endpoint_is_already_in_use() -> Result<(), Box<dyn std::error::Error>> {
    use interprocess::local_socket::traits::ListenerExt as _;
    use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Name, prelude::*};

    if !GenericNamespaced::is_supported() {
        eprintln!("Skipping test: namespaced local sockets not supported");
        return Ok(());
    }

    let pid = std::process::id();
    let suffix = uuid::Uuid::new_v4();
    let socket_name = format!("tenex-muxd-test-{pid}-{suffix}");
    let endpoint: Name<'static> = socket_name
        .clone()
        .to_ns_name::<GenericNamespaced>()?
        .into_owned();

    let listener = ListenerOptions::new().name(endpoint).create_sync()?;
    let accept_handle = std::thread::spawn(move || {
        // Accept one connection from the Tenex muxd ping attempt and drop it so the ping fails.
        for stream in listener.incoming().take(1).flatten() {
            use std::io::Read as _;

            let mut stream = stream;
            let mut len_bytes = [0u8; 4];
            if stream.read_exact(&mut len_bytes).is_ok() {
                let len = u32::from_le_bytes(len_bytes) as usize;
                if len <= 1024 * 1024 {
                    let mut buf = vec![0u8; len];
                    let _ = stream.read_exact(&mut buf);
                }
            }
        }
    });

    let output = tenex_bin()
        .args(["muxd"])
        .env("TENEX_MUX_SOCKET", &socket_name)
        .output()?;
    let _ = accept_handle.join();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Mux endpoint is already in use")
            || stderr.contains("already in use")
            || stdout.contains("Mux endpoint is already in use")
            || stdout.contains("already in use"),
        "unexpected muxd output\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_cli_reset_force_cleans_up_docker_runtime() -> Result<(), Box<dyn std::error::Error>> {
    let state_dir = TempDir::new()?;
    let state_path = state_dir.path().join("state.json");
    let mut storage = tenex::agent::Storage::with_path(state_path.clone());
    storage.instance_id = Some("deadbeef".to_string());

    let mut agent = tenex::Agent::new(
        "Docker Agent".to_string(),
        "codex".to_string(),
        "tenex/docker-branch".to_string(),
        std::env::temp_dir().join("tenex-test-docker-worktree"),
    );
    agent.runtime = tenex::agent::AgentRuntime::Docker;
    agent.mux_session = format!("{}{}", storage.instance_session_prefix(), agent.short_id());
    let expected_container = format!("tenex-runtime-{}", agent.mux_session).to_lowercase();
    storage.add(agent);
    storage.save()?;

    let docker_dir = TempDir::new()?;
    let log = docker_dir.path().join("docker.log");
    write_fake_docker_script(
        docker_dir.path(),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log.display()
        ),
    )?;

    let path = format!(
        "{}:{}",
        docker_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = tenex_bin()
        .args(["reset", "--force"])
        .env("TENEX_STATE_PATH", &state_path)
        .env(
            "TENEX_MUX_SOCKET",
            format!("tenex-mux-test-reset-docker-{}", uuid::Uuid::new_v4()),
        )
        .env("PATH", path)
        .output()?;
    assert!(
        output.status.success(),
        "tenex reset failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let log_contents = fs::read_to_string(&log)?;
    assert!(log_contents.contains(&format!("rm -f {expected_container}")));
    Ok(())
}

#[test]
fn test_cli_reset_interactive_can_abort() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::TempDir;

    let state_dir = TempDir::new()?;
    let state_path = state_dir.path().join("state.json");
    write_state_with_one_agent(&state_path)?;

    let run_dir = TempDir::new()?;
    let mux_socket = format!("tenex-mux-test-reset-interactive-{}", uuid::Uuid::new_v4());

    let mut child = tenex_bin()
        .arg("reset")
        .env("TENEX_STATE_PATH", &state_path)
        .env("TENEX_MUX_SOCKET", &mux_socket)
        .current_dir(run_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .ok_or("Expected stdin to be piped")?
        // default scope + abort
        .write_all(b"\nn\n")?;

    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "tenex reset failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Aborted."), "stdout:\n{stdout}");

    Ok(())
}

#[test]
fn test_cli_reset_interactive_can_confirm() -> Result<(), Box<dyn std::error::Error>> {
    use tempfile::TempDir;

    let state_dir = TempDir::new()?;
    let state_path = state_dir.path().join("state.json");
    write_state_with_one_agent(&state_path)?;

    let run_dir = TempDir::new()?;
    let mux_socket = format!("tenex-mux-test-reset-interactive-{}", uuid::Uuid::new_v4());

    let mut child = tenex_bin()
        .arg("reset")
        .env("TENEX_STATE_PATH", &state_path)
        .env("TENEX_MUX_SOCKET", &mux_socket)
        .current_dir(run_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child
        .stdin
        .as_mut()
        .ok_or("Expected stdin to be piped")?
        // all instances + confirm
        .write_all(b"all\ny\n")?;

    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "tenex reset failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Reset complete."), "stdout:\n{stdout}");

    Ok(())
}

#[test]
fn test_log_file_cleared_on_startup() -> Result<(), Box<dyn std::error::Error>> {
    let log_path = tenex::paths::log_path();
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write some content to the log file to simulate previous session logs
    fs::write(&log_path, "previous session log content\nmore log lines\n")?;

    // Verify the file has content
    let content_before = fs::read_to_string(&log_path)?;
    assert!(
        !content_before.is_empty(),
        "Log file should have content before test"
    );

    // Run tenex with --help (a quick command that exits immediately)
    let output = tenex_bin().arg("--help").output()?;
    assert!(output.status.success());

    // Verify the log file was cleared
    let content_after = fs::read_to_string(&log_path)?;
    assert!(
        content_after.is_empty(),
        "Log file should be empty after tenex startup, but contained: {content_after}"
    );

    Ok(())
}

#[test]
fn test_migrate_settings_without_state_moves_to_tenex_dir() -> Result<(), Box<dyn std::error::Error>>
{
    use tempfile::TempDir;

    let home = TempDir::new()?;
    let xdg_data_home = TempDir::new()?;

    let legacy_dir = xdg_data_home.path().join("tenex");
    fs::create_dir_all(&legacy_dir)?;
    fs::write(
        legacy_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    let output = tenex_bin()
        .args(["reset", "--force"])
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", xdg_data_home.path())
        .env_remove("TENEX_STATE_PATH")
        .env(
            "TENEX_MUX_SOCKET",
            format!("tenex-mux-test-migration-{}", std::process::id()),
        )
        .output()?;
    assert!(
        output.status.success(),
        "tenex reset failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let migrated_settings_path = home.path().join(".tenex").join("settings.json");
    assert!(
        migrated_settings_path.exists(),
        "Expected migrated settings at {}",
        migrated_settings_path.display()
    );
    let migrated_settings = fs::read_to_string(&migrated_settings_path)?;
    assert!(
        migrated_settings.contains("codex"),
        "Expected migrated settings to contain codex, got: {migrated_settings}"
    );

    assert!(!legacy_dir.exists(), "Expected legacy dir to be removed");
    Ok(())
}
