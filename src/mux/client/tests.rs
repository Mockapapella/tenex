use super::*;
use interprocess::local_socket::traits::ListenerExt as _;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

#[test]
fn test_resolve_tenex_executable_from_current_exe_returns_original_when_already_tenex() {
    let exe_name = format!("{}{}", env!("CARGO_PKG_NAME"), std::env::consts::EXE_SUFFIX);
    let exe = PathBuf::from(exe_name);

    let resolved =
        resolve_tenex_executable_from_current_exe(&exe).expect("Resolve tenex executable");

    assert_eq!(resolved, exe);
}

struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn is_pong(response: &MuxResponse) -> bool {
    matches!(response, MuxResponse::Pong { .. })
}

#[test]
fn test_is_pong_reports_false_for_error() {
    let response = MuxResponse::Err {
        message: "nope".to_string(),
    };
    assert!(!is_pong(&response));
}

fn connect_with_timeout(endpoint: &SocketEndpoint, timeout: Duration) -> Result<Stream> {
    let deadline = Instant::now() + timeout;
    loop {
        match Stream::connect(endpoint.name.clone()) {
            Ok(stream) => return Ok(stream),
            Err(err) => {
                if Instant::now() > deadline {
                    return Err(anyhow::anyhow!("connect timeout: {err}"));
                }

                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[test]
fn test_connect_with_timeout_errors_when_deadline_exceeded() {
    use interprocess::local_socket::prelude::*;

    #[cfg(windows)]
    let endpoint = {
        use interprocess::local_socket::GenericNamespaced;

        let display = format!("tenex-mux-client-{}", uuid::Uuid::new_v4());
        let name = display
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("Convert namespaced socket name")
            .into_owned();
        SocketEndpoint {
            name,
            cleanup_path: None,
            display,
        }
    };

    #[cfg(not(windows))]
    let endpoint = {
        use interprocess::local_socket::GenericFilePath;

        let temp_dir = tempfile::TempDir::new().expect("Create mux connect timeout temp dir");
        let socket_path = temp_dir.path().join("missing.sock");
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .expect("Convert missing socket path")
            .into_owned();
        SocketEndpoint {
            name,
            cleanup_path: Some(socket_path.clone()),
            display: socket_path.to_string_lossy().into_owned(),
        }
    };

    let err = connect_with_timeout(&endpoint, Duration::from_millis(0))
        .expect_err("Expected connect timeout error");
    assert!(err.to_string().contains("connect timeout"));
}

fn wait_for_child_exit_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("Poll child status") {
            return Ok(status);
        }

        if Instant::now() > deadline {
            return Err(anyhow::anyhow!("muxd did not exit"));
        }

        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn test_wait_for_child_exit_with_timeout_errors_when_deadline_exceeded() {
    let temp_dir = tempfile::TempDir::new().expect("Create mux wait timeout temp dir");
    let socket_path = temp_dir.path().join("mux.sock");

    let current_exe = std::env::current_exe().expect("Resolve current test executable");
    let mut child = ChildGuard(
        Command::new(current_exe)
            .args(["--exact", MUXD_TEST_ENTRY, "--nocapture"])
            .env("TENEX_RUN_MUXD_TEST_ENTRY", "1")
            .env("TENEX_MUX_SOCKET", &socket_path)
            .spawn()
            .expect("Spawn muxd test entry"),
    );

    let err = wait_for_child_exit_with_timeout(&mut child.0, Duration::from_millis(0))
        .expect_err("Expected child timeout error");
    assert!(err.to_string().contains("muxd did not exit"));
}

#[test]
fn __tenex_muxd_test_entry() {
    if std::env::var_os("TENEX_RUN_MUXD_TEST_ENTRY").is_some() {
        let endpoint = super::super::endpoint::socket_endpoint().expect("Resolve mux endpoint");

        let listener = interprocess::local_socket::ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .context("Failed to create mux listener")
            .expect("Create mux listener");

        let mut incoming = listener.incoming();
        let mut stream = incoming
            .next()
            .expect("Expected mux client connection")
            .expect("Mux accept failed");
        let _request: MuxRequest = ipc::read_json(&mut stream).expect("Read mux request");
        ipc::write_json(
            &mut stream,
            &MuxResponse::Pong {
                version: "test".to_string(),
            },
        )
        .expect("Write pong response");
    }
}

#[test]
fn test_muxd_test_entry_serves_one_ping() {
    let temp_dir = tempfile::TempDir::new().expect("Create muxd test temp dir");
    let socket_path = temp_dir.path().join("mux.sock");
    let display = socket_path.to_string_lossy().into_owned();

    let current_exe = std::env::current_exe().expect("Resolve current test executable");

    let mut child = ChildGuard(
        Command::new(current_exe)
            .args(["--exact", MUXD_TEST_ENTRY, "--nocapture"])
            .env("TENEX_RUN_MUXD_TEST_ENTRY", "1")
            .env("TENEX_MUX_SOCKET", &socket_path)
            .spawn()
            .expect("Spawn muxd test entry"),
    );

    let endpoint = super::super::endpoint::socket_endpoint_from_value(&display)
        .expect("Resolve mux endpoint for test");

    let mut stream = connect_with_timeout(&endpoint, Duration::from_secs(2))
        .expect("Connect to muxd test entry");

    ipc::write_json(&mut stream, &MuxRequest::Ping).expect("Write ping request");
    let response: MuxResponse = ipc::read_json(&mut stream).expect("Read pong response");
    assert!(is_pong(&response));
    drop(stream);

    let status = wait_for_child_exit_with_timeout(&mut child.0, Duration::from_secs(2))
        .expect("Wait for muxd test entry to exit");
    assert!(status.success());
}

#[test]
fn test_resolve_tenex_executable_returns_existing_path() {
    let exe = resolve_tenex_executable().expect("Resolve tenex executable path");
    assert!(exe.exists());
}

#[test]
fn __tenex_muxd_fallback_client_entry() {
    let socket_path = match std::env::var_os("TENEX_MUX_CLIENT_TEST_SOCKET") {
        Some(value) => PathBuf::from(value),
        None => return,
    };

    crate::mux::set_socket_override(&socket_path.to_string_lossy())
        .expect("set fallback client socket override");
    let response = request(&MuxRequest::Ping).expect("Fallback request ping");
    assert!(is_pong(&response));
    let _ = fs::remove_file(&socket_path);
}

#[test]
fn __tenex_resolve_exe_is_tenex_entry() {
    if std::env::var_os("TENEX_RUN_RESOLVE_EXE_IS_TENEX").is_none() {
        return;
    }

    let current_exe = std::env::current_exe().expect("Resolve current exe for is_tenex");
    let resolved = resolve_tenex_executable().expect("Resolve tenex executable for is_tenex");
    assert_eq!(resolved, current_exe);
}

#[test]
fn test_test_scope_key_uses_current_thread_name() {
    let thread = std::thread::current();
    let current = thread.name().unwrap_or("unknown");
    assert_eq!(test_scope_key(), current);
}

#[test]
fn test_test_scope_key_falls_back_for_unnamed_thread() {
    let handle = std::thread::spawn(test_scope_key);
    let scope = handle.join().expect("Unnamed client thread panicked");
    assert!(scope.starts_with("ThreadId("));
}

#[test]
fn test_endpoint_is_not_cached_across_named_threads() {
    let first = std::thread::Builder::new()
        .name("client-endpoint-one".to_string())
        .spawn(|| {
            crate::mux::set_socket_override("tenex-mux-client-one")
                .expect("set client endpoint one socket override");
            endpoint().expect("resolve endpoint for client one").display
        })
        .expect("spawn client endpoint one thread")
        .join()
        .expect("join client endpoint one thread");
    let second = std::thread::Builder::new()
        .name("client-endpoint-two".to_string())
        .spawn(|| {
            crate::mux::set_socket_override("tenex-mux-client-two")
                .expect("set client endpoint two socket override");
            endpoint().expect("resolve endpoint for client two").display
        })
        .expect("spawn client endpoint two thread")
        .join()
        .expect("join client endpoint two thread");

    assert_eq!(first, "tenex-mux-client-one");
    assert_eq!(second, "tenex-mux-client-two");
}

fn command_env(cmd: &Command, key: &str) -> Option<std::ffi::OsString> {
    let key = std::ffi::OsStr::new(key);
    cmd.get_envs().find_map(|(name, value)| {
        if name == key {
            value.map(std::ffi::OsStr::to_os_string)
        } else {
            None
        }
    })
}

#[test]
fn test_command_env_returns_none_for_missing_key() {
    let mut cmd = Command::new("tenex-mux-test");
    cmd.env("OTHER_KEY", "value");
    assert!(command_env(&cmd, "TENEX_STATE_PATH").is_none());
}

#[test]
fn test_maybe_inherit_state_path_is_noop_when_missing_or_blank() {
    let mut cmd = Command::new("tenex-mux-test");
    maybe_inherit_state_path(&mut cmd, None);
    assert!(command_env(&cmd, "TENEX_STATE_PATH").is_none());

    let mut cmd = Command::new("tenex-mux-test");
    maybe_inherit_state_path(&mut cmd, Some("   "));
    assert!(command_env(&cmd, "TENEX_STATE_PATH").is_none());
}

#[test]
fn test_maybe_inherit_state_path_sets_trimmed_value() {
    let mut cmd = Command::new("tenex-mux-test");
    maybe_inherit_state_path(&mut cmd, Some("  /tmp/tenex-state.json "));
    assert_eq!(
        command_env(&cmd, "TENEX_STATE_PATH").as_deref(),
        Some(std::ffi::OsStr::new("/tmp/tenex-state.json"))
    );
}

#[test]
fn test_mux_client_reconnects_after_failed_request() {
    use interprocess::local_socket::traits::ListenerExt;
    use interprocess::local_socket::{ListenerOptions, prelude::*};

    #[cfg(windows)]
    let endpoint = {
        use interprocess::local_socket::GenericNamespaced;

        let display = format!("tenex-mux-client-{}", uuid::Uuid::new_v4());
        let name = display
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("Convert namespaced socket name")
            .into_owned();
        SocketEndpoint {
            name,
            cleanup_path: None,
            display,
        }
    };

    #[cfg(not(windows))]
    let endpoint = {
        use interprocess::local_socket::GenericFilePath;

        let socket_name = format!("tx-mux-client-{}.sock", uuid::Uuid::new_v4().simple());
        let socket_path = std::env::temp_dir().join(socket_name);
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .expect("Convert socket path")
            .into_owned();
        SocketEndpoint {
            name,
            cleanup_path: Some(socket_path.clone()),
            display: socket_path.to_string_lossy().into_owned(),
        }
    };

    let listener = ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("Create mux listener");
    let handle = std::thread::spawn(move || {
        let mut incoming = listener.incoming();

        let mut stream = incoming
            .next()
            .expect("Missing initial connection")
            .expect("Accept initial connection");
        let _: MuxRequest = ipc::read_json(&mut stream).expect("Read initial request");
        drop(stream);

        let mut stream = incoming
            .next()
            .expect("Missing reconnect connection")
            .expect("Accept reconnect connection");
        let _: MuxRequest = ipc::read_json(&mut stream).expect("Read reconnect request");
        ipc::write_json(
            &mut stream,
            &MuxResponse::Pong {
                version: "test".to_string(),
            },
        )
        .expect("Write reconnect response");
    });

    let mut client = MuxClient::new(endpoint);
    let response = client.request(&MuxRequest::Ping).expect("Request ping");
    assert!(is_pong(&response));

    handle.join().expect("Server thread panicked");
}

#[test]
fn test_request_replaces_thread_scoped_client_when_endpoint_changes() {
    let handle = std::thread::Builder::new()
        .name(format!(
            "mux-client-endpoint-change-{}",
            uuid::Uuid::new_v4().simple()
        ))
        .spawn(|| {
            let original_endpoint = endpoint().expect("Resolve original endpoint");
            let response = request(&MuxRequest::Ping).expect("Request ping");
            assert!(is_pong(&response));

            let temp_dir = tempfile::TempDir::new().expect("Create endpoint change temp dir");
            let socket_path = temp_dir.path().join("mux.sock");
            crate::mux::set_socket_override(&socket_path.to_string_lossy())
                .expect("Set socket override");

            let response = request(&MuxRequest::Ping).expect("Request ping with new socket");
            assert!(is_pong(&response));

            crate::mux::terminate_mux_daemon_for_socket(&original_endpoint.display)
                .expect("Terminate mux daemon for original socket");
            crate::mux::terminate_mux_daemon_for_socket(&socket_path.to_string_lossy())
                .expect("Terminate mux daemon for override socket");
            let _ = fs::remove_file(&socket_path);
        })
        .expect("Spawn endpoint change thread");

    handle.join().expect("endpoint change thread panicked");
}

#[test]
fn test_request_spawns_daemon_when_socket_is_unreachable() {
    let temp_dir = tempfile::TempDir::new().expect("Create request spawn temp dir");
    let socket_path = temp_dir.path().join("mux.sock");
    crate::mux::set_socket_override(&socket_path.to_string_lossy())
        .expect("Set request spawn socket override");

    let response = request(&MuxRequest::Ping).expect("Request ping");
    assert!(is_pong(&response));

    crate::mux::terminate_mux_daemon_for_socket(&socket_path.to_string_lossy())
        .expect("Terminate mux daemon");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_start_daemon_can_fall_back_to_test_entry_when_binary_is_missing() {
    let temp_dir = tempfile::TempDir::new().expect("Create start daemon temp dir");
    let deps_dir = temp_dir.path().join("deps");
    fs::create_dir_all(&deps_dir).expect("Create deps dir");

    let current_exe = std::env::current_exe().expect("Resolve current exe for fallback");
    let runner_path = deps_dir.join(format!("runner{}", std::env::consts::EXE_SUFFIX));
    fs::copy(&current_exe, &runner_path).expect("Copy runner");
    #[cfg(unix)]
    fs::set_permissions(&runner_path, fs::Permissions::from_mode(0o755))
        .expect("Set runner permissions");

    let socket_path = temp_dir.path().join("mux.sock");
    let status = Command::new(&runner_path)
        .args(["--exact", MUXD_FALLBACK_CLIENT_ENTRY, "--nocapture"])
        .env("TENEX_MUX_CLIENT_TEST_SOCKET", &socket_path)
        .status()
        .expect("Run fallback client entry");

    assert!(status.success());
}

#[test]
fn test_resolve_tenex_executable_returns_current_exe_when_named_tenex() {
    let temp_dir = tempfile::TempDir::new().expect("Create is_tenex temp dir");
    let current_exe = std::env::current_exe().expect("Resolve current exe for is_tenex");
    let tenex_path = temp_dir
        .path()
        .join(format!("tenex{}", std::env::consts::EXE_SUFFIX));
    fs::copy(&current_exe, &tenex_path).expect("Copy tenex binary");
    #[cfg(unix)]
    fs::set_permissions(&tenex_path, fs::Permissions::from_mode(0o755))
        .expect("Set tenex permissions");

    let status = Command::new(&tenex_path)
        .args(["--exact", RESOLVE_EXE_IS_TENEX_ENTRY, "--nocapture"])
        .env("TENEX_RUN_RESOLVE_EXE_IS_TENEX", "1")
        .status()
        .expect("Run resolve exe is_tenex entry");

    assert!(status.success());
}

#[test]
fn test_mux_client_reports_error_after_retrying() {
    use interprocess::local_socket::GenericFilePath;
    use interprocess::local_socket::prelude::*;

    let temp_dir = tempfile::TempDir::new().expect("Create retry error temp dir");
    let socket_path = temp_dir.path().join("mux.sock");
    fs::create_dir_all(&socket_path).expect("Create retry error socket dir");
    let name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()
        .expect("Convert retry error socket path")
        .into_owned();
    let endpoint = SocketEndpoint {
        name,
        cleanup_path: Some(socket_path.clone()),
        display: socket_path.to_string_lossy().into_owned(),
    };

    let mut client = MuxClient::new(endpoint);
    let err = client
        .request(&MuxRequest::Ping)
        .expect_err("Expected connect error");
    let message = format!("{err:#}");
    assert!(message.contains("Failed to connect to mux daemon at"));
}

#[test]
fn test_resolve_tenex_executable_errors_when_current_exe_fails() {
    let err = with_current_exe_override(CurrentExeOverride::Err, resolve_tenex_executable)
        .expect_err("Expected resolve_tenex_executable error");
    assert!(
        err.to_string()
            .contains("Failed to resolve current executable")
    );
}

#[test]
fn test_resolve_tenex_executable_errors_when_build_output_dir_unresolvable() {
    let err = with_current_exe_override(
        CurrentExeOverride::Ok(PathBuf::from("runner")),
        resolve_tenex_executable,
    )
    .expect_err("Expected resolve_tenex_executable error");
    assert!(
        err.to_string()
            .contains("Failed to resolve build output directory")
    );
}

#[test]
fn test_start_daemon_errors_when_current_exe_fails() {
    let endpoint = super::super::endpoint::socket_endpoint_from_value("tenex-mux-test-name")
        .expect("Resolve endpoint for start_daemon error test");
    let err = with_current_exe_override(CurrentExeOverride::Err, || start_daemon(&endpoint))
        .expect_err("Expected start_daemon error");
    assert!(
        err.to_string()
            .contains("Failed to resolve current executable")
    );
}

#[test]
fn test_current_exe_falls_back_when_override_values_are_empty() {
    let exe = with_current_exe_overrides(Vec::new(), current_exe)
        .expect("expected current_exe to succeed");
    assert!(!exe.as_os_str().is_empty());
}

#[test]
fn test_start_daemon_errors_when_current_test_exe_fails() {
    let endpoint = super::super::endpoint::socket_endpoint_from_value("tenex-mux-test-name")
        .expect("Resolve endpoint for start_daemon error test");
    let fake_exe = std::env::temp_dir()
        .join("tenex")
        .join("target")
        .join("debug")
        .join(env!("CARGO_PKG_NAME"));
    let err = with_current_exe_overrides(
        vec![CurrentExeOverride::Ok(fake_exe), CurrentExeOverride::Err],
        || start_daemon(&endpoint),
    )
    .expect_err("Expected start_daemon error");
    assert!(
        err.to_string()
            .contains("Failed to resolve current test executable")
    );
}

#[test]
fn test_start_daemon_errors_when_current_executable_is_unresolvable() {
    let endpoint = super::super::endpoint::socket_endpoint_from_value("tenex-mux-test-name")
        .expect("Resolve endpoint for start_daemon error test");
    let err = with_current_exe_override(CurrentExeOverride::Ok(PathBuf::from("runner")), || {
        start_daemon(&endpoint)
    })
    .expect_err("Expected start_daemon error");
    assert!(
        err.to_string()
            .contains("Failed to resolve build output directory")
    );
}

#[test]
fn test_start_daemon_reports_spawn_errors_with_context() {
    let endpoint = super::super::endpoint::socket_endpoint_from_value("tenex-mux-test-name")
        .expect("Resolve endpoint for start_daemon spawn error test");
    let temp_dir = tempfile::TempDir::new().expect("Create spawn error temp dir");
    let fake_exe = temp_dir.path().join(env!("CARGO_PKG_NAME"));
    let err =
        with_current_exe_override(CurrentExeOverride::Ok(fake_exe), || start_daemon(&endpoint))
            .expect_err("Expected spawn error");
    assert!(err.to_string().contains("Failed to spawn mux daemon"));
}

#[test]
fn test_request_returns_error_when_endpoint_is_invalid() {
    let handle = std::thread::Builder::new()
        .name(format!(
            "mux-client-invalid-endpoint-{}",
            uuid::Uuid::new_v4()
        ))
        .spawn(|| {
            crate::mux::set_socket_override("bad\0socket").expect("Set invalid socket override");
            request(&MuxRequest::Ping).expect_err("Expected request error");
        })
        .expect("Spawn invalid endpoint thread");
    handle
        .join()
        .expect("invalid endpoint thread should not panic");
}

#[test]
fn test_send_request_reports_errors_when_write_fails() {
    struct FailWriteStream;

    impl Read for FailWriteStream {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl Write for FailWriteStream {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write boom"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut stream = FailWriteStream;
    let mut buf = [0u8; 1];
    assert_eq!(stream.read(&mut buf).expect("read"), 0);
    stream.flush().expect("flush");

    let err = send_request(&mut stream, &MuxRequest::Ping).expect_err("Expected write error");
    assert!(err.to_string().contains("Failed to write message length"));
}

#[test]
fn test_mux_client_request_errors_when_reconnect_fails() {
    use interprocess::local_socket::prelude::*;

    #[cfg(windows)]
    let endpoint = {
        use interprocess::local_socket::GenericNamespaced;
        let display = format!("tenex-mux-client-{}", uuid::Uuid::new_v4());
        let name = display
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .expect("Convert namespaced socket name")
            .into_owned();
        SocketEndpoint {
            name,
            cleanup_path: None,
            display,
        }
    };

    #[cfg(not(windows))]
    let endpoint = {
        use interprocess::local_socket::GenericFilePath;

        let socket_name = format!("tx-mux-client-{}.sock", uuid::Uuid::new_v4().simple());
        let socket_path = std::env::temp_dir().join(socket_name);
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .expect("Convert socket path")
            .into_owned();
        SocketEndpoint {
            name,
            cleanup_path: Some(socket_path.clone()),
            display: socket_path.to_string_lossy().into_owned(),
        }
    };

    let listener = interprocess::local_socket::ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("Create mux listener");

    let server = std::thread::spawn(move || {
        let mut incoming = listener.incoming();

        let mut stream = incoming
            .next()
            .expect("Missing initial connection")
            .expect("Accept initial connection");

        let _: MuxRequest = ipc::read_json(&mut stream).expect("Read initial request");
        drop(stream);
    });

    let err = with_current_exe_override(CurrentExeOverride::Err, || {
        let mut client = MuxClient::new(endpoint);
        client.request(&MuxRequest::Ping)
    })
    .expect_err("Expected mux client error");
    assert!(
        err.to_string()
            .contains("Failed to resolve current executable")
    );

    server.join().expect("Server thread panicked");
}
