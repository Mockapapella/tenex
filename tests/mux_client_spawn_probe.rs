//! Exercise mux client startup paths from a non-test build.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

struct CleanupGuard {
    socket: String,
    instance_root: PathBuf,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        cleanup_muxd_for_socket(&self.socket, &self.instance_root);
    }
}

fn is_pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn kill_pid(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn apply_llvm_profile_file_for_child(cmd: &mut Command) {
    if std::env::var_os("LLVM_PROFILE_FILE").is_none() {
        return;
    }

    let _ = fs::create_dir_all("target/llvm-cov-target");
    cmd.env(
        "LLVM_PROFILE_FILE",
        "target/llvm-cov-target/tenex-%p-%m.profraw",
    );
}

#[cfg(target_os = "linux")]
fn muxd_pids_for_socket(socket: &str) -> std::collections::HashSet<u32> {
    let wanted = format!("TENEX_MUX_SOCKET={socket}");
    let mut pids = std::collections::HashSet::new();

    let Ok(entries) = fs::read_dir("/proc") else {
        return pids;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid_str) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };

        let base = entry.path();
        let Ok(cmdline) = fs::read(base.join("cmdline")) else {
            continue;
        };
        if !cmdline
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .any(|part| part == b"muxd")
        {
            continue;
        }

        let Ok(environ) = fs::read(base.join("environ")) else {
            continue;
        };
        if !environ
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .any(|part| part == wanted.as_bytes())
        {
            continue;
        }

        pids.insert(pid);
    }

    pids
}

#[cfg(not(target_os = "linux"))]
fn muxd_pids_for_socket(socket: &str, instance_root: &Path) -> std::collections::HashSet<u32> {
    let mut pids = std::collections::HashSet::new();
    let Ok(entries) = fs::read_dir(instance_root) else {
        return pids;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let has_pid_extension = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pid"));
        if !name.starts_with("tenex-muxd-") || !has_pid_extension {
            continue;
        }

        let Ok(raw) = fs::read(&path) else {
            continue;
        };
        let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        let pid = payload.get("pid").and_then(serde_json::Value::as_u64);
        let reported_socket = payload.get("socket").and_then(serde_json::Value::as_str);
        let Some(pid) = pid else {
            continue;
        };
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        if reported_socket.map(str::trim) != Some(socket.trim()) {
            continue;
        }
        pids.insert(pid);
    }

    pids
}

fn cleanup_muxd_for_socket(socket: &str, instance_root: &Path) {
    let _ = instance_root;
    #[cfg(target_os = "linux")]
    let mut pids = muxd_pids_for_socket(socket);
    #[cfg(not(target_os = "linux"))]
    let mut pids = muxd_pids_for_socket(socket, instance_root);

    if pids.is_empty() {
        return;
    }

    for pid in &pids {
        kill_pid(*pid, "-TERM");
    }

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        pids.retain(|pid| is_pid_alive(*pid));
        if pids.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    for pid in &pids {
        kill_pid(*pid, "-KILL");
    }
}

#[test]
fn test_mux_client_spawn_probe_covers_is_tenex_resolution_in_non_test_build()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::TempDir::new()?;

    let probe_path = PathBuf::from(env!("CARGO_BIN_EXE_mux_client_spawn_probe"));
    let tenex_path = temp_dir
        .path()
        .join(format!("tenex{}", std::env::consts::EXE_SUFFIX));
    fs::copy(&probe_path, &tenex_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tenex_path, fs::Permissions::from_mode(0o755))?;
    }

    let workdir = temp_dir.path().join("workdir");
    fs::create_dir_all(&workdir)?;

    let socket_path = temp_dir.path().join("mux.sock");
    let state_path = temp_dir.path().join("state.json");
    let socket_display = socket_path.to_string_lossy().to_string();
    let instance_root = temp_dir.path().to_path_buf();

    let _cleanup = CleanupGuard {
        socket: socket_display,
        instance_root,
    };

    let session_name = format!("tenex-test-mux-client-probe-{}", uuid::Uuid::new_v4());
    let mut cmd = Command::new(&tenex_path);
    cmd.env("TENEX_MUX_SOCKET", &socket_path)
        .env("TENEX_STATE_PATH", &state_path)
        .env("TENEX_TEST_MUX_SESSION_NAME", &session_name)
        .env("TENEX_TEST_MUX_WORKDIR", &workdir);

    apply_llvm_profile_file_for_child(&mut cmd);

    let status = cmd.status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_errors_when_session_name_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"))
        .env_remove("TENEX_TEST_MUX_SESSION_NAME")
        .env_remove("TENEX_TEST_MUX_WORKDIR")
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("TENEX_TEST_MUX_SESSION_NAME is required")
    );
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_errors_when_session_name_blank()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"))
        .env("TENEX_TEST_MUX_SESSION_NAME", " ")
        .env_remove("TENEX_TEST_MUX_WORKDIR")
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("TENEX_TEST_MUX_SESSION_NAME is required")
    );
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_errors_when_workdir_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"))
        .env("TENEX_TEST_MUX_SESSION_NAME", "tenex-test-mux-client-probe")
        .env_remove("TENEX_TEST_MUX_WORKDIR")
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("TENEX_TEST_MUX_WORKDIR is required"));
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_errors_when_workdir_blank() -> Result<(), Box<dyn std::error::Error>>
{
    let output = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"))
        .env("TENEX_TEST_MUX_SESSION_NAME", "tenex-test-mux-client-probe")
        .env("TENEX_TEST_MUX_WORKDIR", " ")
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("TENEX_TEST_MUX_WORKDIR is required"));
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_errors_when_session_create_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::TempDir::new()?;
    let parent_as_file = temp_dir.path().join("not-a-dir");
    fs::write(&parent_as_file, b"nope")?;

    let socket_path = parent_as_file.join("mux.sock");
    let workdir = temp_dir.path().join("workdir");
    fs::create_dir_all(&workdir)?;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"));
    cmd.env(
        "TENEX_MUX_SOCKET",
        socket_path.to_string_lossy().to_string(),
    )
    .env("TENEX_STATE_PATH", temp_dir.path().join("state.json"))
    .env("TENEX_TEST_MUX_SESSION_NAME", "tenex-test-mux-client-probe")
    .env("TENEX_TEST_MUX_WORKDIR", &workdir);
    apply_llvm_profile_file_for_child(&mut cmd);

    let output = cmd.output()?;
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_muxd_returns_success_when_existing_daemon_responds()
-> Result<(), Box<dyn std::error::Error>> {
    use interprocess::local_socket::ListenerOptions;
    use interprocess::local_socket::{GenericFilePath, prelude::*};
    use std::io::{Read, Write};
    use std::sync::mpsc;

    let temp_dir = tempfile::TempDir::new()?;
    let socket_path = temp_dir.path().join("mux.sock");
    let socket_display = socket_path.to_string_lossy().to_string();
    let name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()?
        .into_owned();

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel::<std::io::Result<()>>();

    std::thread::spawn(move || {
        let listener = ListenerOptions::new().name(name).create_sync();
        let listener = match listener {
            Ok(listener) => listener,
            Err(err) => {
                let _ = done_tx.send(Err(err));
                return;
            }
        };

        let _ = ready_tx.send(());

        let result = (|| -> std::io::Result<()> {
            for mut stream in listener.incoming().flatten().take(1) {
                let mut len_bytes = [0u8; 4];
                stream.read_exact(&mut len_bytes)?;
                let len = u32::from_le_bytes(len_bytes) as usize;
                let mut payload = vec![0u8; len];
                stream.read_exact(&mut payload)?;

                let response = br#""Ok""#;
                let response_len = u32::try_from(response.len())
                    .map_err(|_| std::io::Error::other("mock response too large"))?;
                stream.write_all(&response_len.to_le_bytes())?;
                stream.write_all(response)?;
                stream.flush()?;
            }
            Ok(())
        })();

        let _ = done_tx.send(result);
    });

    ready_rx.recv_timeout(Duration::from_secs(1))?;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"));
    cmd.arg("muxd").env("TENEX_MUX_SOCKET", socket_display);
    apply_llvm_profile_file_for_child(&mut cmd);
    let status = cmd.status()?;
    assert!(status.success());

    let server_result = done_rx.recv_timeout(Duration::from_secs(1))?;
    server_result?;
    Ok(())
}

#[test]
fn test_mux_client_spawn_probe_muxd_exits_nonzero_when_daemon_start_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::TempDir::new()?;
    let parent_as_file = temp_dir.path().join("not-a-dir");
    fs::write(&parent_as_file, b"nope")?;

    let socket_path = parent_as_file.join("mux.sock");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mux_client_spawn_probe"));
    cmd.arg("muxd")
        .env(
            "TENEX_MUX_SOCKET",
            socket_path.to_string_lossy().to_string(),
        )
        .env("TENEX_STATE_PATH", temp_dir.path().join("state.json"));
    apply_llvm_profile_file_for_child(&mut cmd);

    let output = cmd.output()?;
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
    Ok(())
}
