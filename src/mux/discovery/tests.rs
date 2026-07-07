use super::*;
use tempfile::TempDir;

#[cfg(target_os = "linux")]
#[test]
fn test_cmdline_contains_muxd() {
    assert!(cmdline_contains_muxd(b"tenex\0muxd\0"));
    assert!(!cmdline_contains_muxd(b"tenex\0\0"));
}

#[cfg(target_os = "linux")]
#[test]
fn test_parse_environ() {
    let env = b"A=1\0TENEX_MUX_SOCKET=test\0B=2\0";
    let parsed = parse_environ(env);
    assert_eq!(parsed.get("A"), Some(&"1".to_string()));
    assert_eq!(parsed.get("TENEX_MUX_SOCKET"), Some(&"test".to_string()));
    assert_eq!(parsed.get("B"), Some(&"2".to_string()));
}

#[cfg(target_os = "linux")]
#[test]
fn test_parse_environ_skips_entries_without_equals_sign() {
    let parsed = parse_environ(b"NOEQUALS\0");
    assert!(parsed.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_parse_environ_skips_invalid_utf8_entries() {
    let parsed = parse_environ(b"A=\xff\0\xff=1\0");
    assert!(parsed.is_empty());
}

#[test]
fn test_discover_socket_for_sessions_returns_none_for_empty_input() {
    let wanted_sessions: HashSet<String> = HashSet::new();
    assert!(discover_socket_for_sessions(&wanted_sessions, None).is_none());
}

#[test]
fn test_discover_socket_for_sessions_finds_matching_session() {
    let session_manager = crate::mux::SessionManager::new();
    let session_name = format!("tenex-test-discovery-{}", uuid::Uuid::new_v4());
    let workdir = TempDir::new().expect("temp dir");
    session_manager
        .create(&session_name, workdir.path(), None)
        .expect("create session");

    let socket = crate::mux::socket_display().expect("socket display");

    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert(session_name.clone());

    let discovered = discover_socket_for_sessions(&wanted_sessions, Some(&socket));

    let _ = session_manager.kill(&session_name);

    assert_eq!(discovered.as_deref(), Some(socket.as_str()));
}

#[test]
fn test_discover_socket_for_session_candidates_returns_none_when_candidates_empty() {
    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());

    let discovered =
        discover_socket_for_session_candidates(&wanted_sessions, Vec::new(), |_, _| Some(1));
    assert!(discovered.is_none());
}

#[test]
fn test_discover_socket_for_session_candidates_picks_best_match_and_dedupes() {
    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());

    let mut probed = Vec::new();
    let discovered = discover_socket_for_session_candidates(
        &wanted_sessions,
        vec![
            "worse".to_string(),
            "best".to_string(),
            "zero".to_string(),
            "unreachable".to_string(),
            "best".to_string(),
            "tie".to_string(),
            "other".to_string(),
        ],
        |candidate, _| {
            probed.push(candidate.to_string());
            if candidate == "worse" {
                Some(1)
            } else if candidate == "best" || candidate == "tie" {
                Some(2)
            } else if candidate == "zero" {
                Some(0)
            } else if candidate == "unreachable" {
                None
            } else {
                Some(0)
            }
        },
    );

    assert_eq!(discovered.as_deref(), Some("best"));
    assert_eq!(
        probed,
        vec!["worse", "best", "zero", "unreachable", "tie", "other"]
    );
}

#[test]
fn test_mux_daemon_pids_for_socket_returns_empty_for_empty_socket() {
    assert!(mux_daemon_pids_for_socket("").is_empty());
    assert!(mux_daemon_pids_for_socket("   ").is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_pid_is_alive_in_proc_root_reports_missing_dir_false() {
    let proc_root = TempDir::new().expect("temp proc root");
    assert!(!pid_is_alive_in_proc_root(proc_root.path(), 1234));
}

#[cfg(target_os = "linux")]
#[test]
fn test_pid_is_alive_in_proc_root_returns_true_when_stat_missing_but_dir_exists() {
    let proc_root = TempDir::new().expect("temp proc root");
    std::fs::create_dir_all(proc_root.path().join("1234")).expect("create pid dir");
    assert!(pid_is_alive_in_proc_root(proc_root.path(), 1234));
}

#[cfg(target_os = "linux")]
#[test]
fn test_pid_is_alive_in_proc_root_returns_true_when_stat_missing_separator() {
    let proc_root = TempDir::new().expect("temp proc root");
    let pid_dir = proc_root.path().join("1234");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("stat"), "invalid").expect("write stat");
    assert!(pid_is_alive_in_proc_root(proc_root.path(), 1234));
}

#[cfg(target_os = "linux")]
#[test]
fn test_pid_is_alive_in_proc_root_detects_zombies() {
    let proc_root = TempDir::new().expect("temp proc root");
    let pid_dir = proc_root.path().join("1234");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("stat"), "1 (tenex) Z 0").expect("write stat");
    assert!(!pid_is_alive_in_proc_root(proc_root.path(), 1234));
}

#[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
#[test]
fn test_pid_is_alive_returns_true_for_current_pid() {
    assert!(pid_is_alive(std::process::id()));
}

#[cfg(target_os = "linux")]
#[test]
fn test_mux_daemon_pids_for_socket_handles_missing_cmdline_files() {
    let proc_root = TempDir::new().expect("temp proc root");

    // Missing cmdline file should be skipped (covers cmdline read failure path).
    std::fs::create_dir_all(proc_root.path().join("1000")).expect("create pid dir");

    let wanted_socket = "tenex-test-socket";
    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    std::fs::write(
        pid_dir.join("environ"),
        format!("TENEX_MUX_SOCKET={wanted_socket}\0").as_bytes(),
    )
    .expect("Expected to write environ");

    let pids = mux_daemon_pids_for_socket_in_proc_root(proc_root.path(), wanted_socket);
    assert_eq!(pids, vec![2000]);
}

#[cfg(target_os = "linux")]
#[test]
fn test_mux_daemon_pids_for_socket_in_proc_root_returns_empty_when_proc_root_not_directory() {
    let temp = TempDir::new().expect("temp dir");
    let not_dir = temp.path().join("not-a-dir");
    std::fs::write(&not_dir, "nope").expect("write not-dir marker");

    let pids = mux_daemon_pids_for_socket_in_proc_root(&not_dir, "tenex-test-socket");
    assert!(pids.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_mux_daemon_pids_for_socket_in_proc_root_skips_non_utf8_entries() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let proc_root = TempDir::new().expect("temp proc root");
    let bad_name = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
    std::fs::create_dir_all(proc_root.path().join(bad_name)).expect("create bad entry");

    let pids = mux_daemon_pids_for_socket_in_proc_root(proc_root.path(), "tenex-test-socket");
    assert!(pids.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_mux_daemon_pids_for_socket_in_proc_root_skips_missing_environ() {
    let proc_root = TempDir::new().expect("temp proc root");
    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");

    let pids = mux_daemon_pids_for_socket_in_proc_root(proc_root.path(), "tenex-test-socket");
    assert!(pids.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_mux_daemon_pids_for_socket_in_proc_root_skips_missing_socket_env() {
    let proc_root = TempDir::new().expect("temp proc root");
    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    std::fs::write(pid_dir.join("environ"), b"A=1\0").expect("write environ");

    let pids = mux_daemon_pids_for_socket_in_proc_root(proc_root.path(), "tenex-test-socket");
    assert!(pids.is_empty());
}

#[test]
fn test_probe_session_matches_returns_none_when_response_not_sessions() {
    use interprocess::local_socket::traits::ListenerExt as _;

    let temp = TempDir::new().expect("temp dir");
    let socket_path = temp.path().join("probe.sock");
    let socket = socket_path.to_string_lossy().into_owned();
    let endpoint = socket_endpoint_from_value(&socket).expect("socket endpoint");

    let listener = interprocess::local_socket::ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("create listener");
    let server = std::thread::spawn(move || {
        let mut incoming = listener.incoming();
        let mut stream = incoming
            .next()
            .expect("incoming stream")
            .expect("accept stream");
        let request: MuxRequest = ipc::read_json(&mut stream).expect("read request");
        assert_eq!(
            std::mem::discriminant(&request),
            std::mem::discriminant(&MuxRequest::ListSessions)
        );
        ipc::write_json(
            &mut stream,
            &MuxResponse::Pong {
                version: "test".to_string(),
            },
        )
        .expect("Expected to write response");
    });

    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());
    assert!(probe_session_matches(&socket, &wanted_sessions).is_none());

    server.join().expect("server join");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_discover_socket_for_sessions_skips_default_socket_when_socket_display_errors() {
    crate::mux::endpoint::set_socket_override("/tmp/tenex-mux-test\0bad.sock")
        .expect("set mux socket override");

    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());

    assert!(discover_socket_for_sessions(&wanted_sessions, None).is_none());
}

#[test]
fn test_probe_session_matches_returns_none_when_socket_parse_fails() {
    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());

    assert!(probe_session_matches("/tmp/tenex-mux-test\0bad.sock", &wanted_sessions).is_none());
}

#[test]
fn test_probe_session_matches_returns_none_when_connect_fails() {
    let temp = TempDir::new().expect("temp dir");
    let socket_path = temp.path().join("missing.sock");
    let socket = socket_path.to_string_lossy().into_owned();

    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());

    assert!(probe_session_matches(&socket, &wanted_sessions).is_none());
}

#[test]
fn test_probe_session_matches_returns_none_when_write_fails() {
    use interprocess::local_socket::traits::ListenerExt as _;

    let temp = TempDir::new().expect("temp dir");
    let socket_path = temp.path().join("close-on-accept.sock");
    let socket = socket_path.to_string_lossy().into_owned();
    let endpoint = socket_endpoint_from_value(&socket).expect("socket endpoint");

    let listener = interprocess::local_socket::ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("create listener");
    let server = std::thread::spawn(move || {
        // Accept a single connection and then immediately drop it so the client write fails.
        for _ in listener.incoming().take(1) {}
    });

    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());
    with_test_probe_delay_before_write(|| {
        assert!(probe_session_matches(&socket, &wanted_sessions).is_none());
    });

    server.join().expect("server join");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_probe_session_matches_returns_none_when_read_fails() {
    use interprocess::local_socket::traits::ListenerExt as _;

    let temp = TempDir::new().expect("temp dir");
    let socket_path = temp.path().join("close-after-read.sock");
    let socket = socket_path.to_string_lossy().into_owned();
    let endpoint = socket_endpoint_from_value(&socket).expect("socket endpoint");

    let listener = interprocess::local_socket::ListenerOptions::new()
        .name(endpoint.name.clone())
        .create_sync()
        .expect("create listener");
    let server = std::thread::spawn(move || {
        let mut incoming = listener.incoming();
        let mut stream = incoming
            .next()
            .expect("incoming stream")
            .expect("accept stream");
        let _: MuxRequest = ipc::read_json(&mut stream).expect("read request");
        // Intentionally do not respond, so the client read fails.
    });

    let mut wanted_sessions = HashSet::new();
    wanted_sessions.insert("wanted".to_string());
    assert!(probe_session_matches(&socket, &wanted_sessions).is_none());

    server.join().expect("server join");
    let _ = std::fs::remove_file(&socket_path);
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_handles_missing_cmdline_files() {
    let proc_root = TempDir::new().expect("temp proc root");
    std::fs::create_dir_all(proc_root.path().join("1000")).expect("create pid dir");

    let want_path_sockets = crate::mux::socket_display()
        .ok()
        .is_some_and(|display| display.contains('/') || display.contains('\\'));

    let socket_good = if want_path_sockets {
        proc_root
            .path()
            .join("good.sock")
            .to_string_lossy()
            .into_owned()
    } else {
        "tenex-mux-test-good".to_string()
    };

    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");

    let wanted_state_path = std::env::var("TENEX_STATE_PATH").unwrap_or_default();
    let wanted_state_path = wanted_state_path.trim();
    let environ = format!("TENEX_MUX_SOCKET={socket_good}\0TENEX_STATE_PATH={wanted_state_path}\0");
    std::fs::write(pid_dir.join("environ"), environ.as_bytes()).expect("write environ");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert!(sockets.contains(&socket_good));
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_filters_mismatched_state_path() {
    let Some(wanted_state_path) = std::env::var("TENEX_STATE_PATH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        // The state-path filter only applies when TENEX_STATE_PATH is set.
        return;
    };

    let want_path_sockets = crate::mux::socket_display()
        .ok()
        .is_some_and(|display| display.contains('/') || display.contains('\\'));

    let proc_root = TempDir::new().expect("temp proc root");
    let socket_good = if want_path_sockets {
        proc_root
            .path()
            .join("good.sock")
            .to_string_lossy()
            .into_owned()
    } else {
        "tenex-mux-test-good".to_string()
    };
    let socket_bad = if want_path_sockets {
        proc_root
            .path()
            .join("bad.sock")
            .to_string_lossy()
            .into_owned()
    } else {
        "tenex-mux-test-bad".to_string()
    };

    let good_pid = proc_root.path().join("1000");
    std::fs::create_dir_all(&good_pid).expect("create pid dir");
    std::fs::write(good_pid.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    std::fs::write(
        good_pid.join("environ"),
        format!("TENEX_MUX_SOCKET={socket_good}\0TENEX_STATE_PATH={wanted_state_path}\0")
            .as_bytes(),
    )
    .expect("Expected to write environ");

    let bad_pid = proc_root.path().join("2000");
    std::fs::create_dir_all(&bad_pid).expect("create pid dir");
    std::fs::write(bad_pid.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    std::fs::write(
        bad_pid.join("environ"),
        format!("TENEX_MUX_SOCKET={socket_bad}\0TENEX_STATE_PATH={wanted_state_path}.mismatch\0")
            .as_bytes(),
    )
    .expect("Expected to write environ");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert!(sockets.contains(&socket_good));
    assert!(!sockets.contains(&socket_bad));
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_in_proc_root_returns_empty_when_proc_root_not_directory() {
    let temp = TempDir::new().expect("temp dir");
    let not_dir = temp.path().join("not-a-dir");
    std::fs::write(&not_dir, "nope").expect("write not-dir marker");

    let sockets = running_mux_sockets_in_proc_root(&not_dir);
    assert!(sockets.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_in_proc_root_skips_non_utf8_entries() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let proc_root = TempDir::new().expect("temp proc root");
    let bad_name = OsString::from_vec(vec![0xff, 0xfe, 0xfd]);
    std::fs::create_dir_all(proc_root.path().join(bad_name)).expect("create bad entry");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert!(sockets.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_in_proc_root_skips_missing_environ() {
    let proc_root = TempDir::new().expect("temp proc root");
    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert!(sockets.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_in_proc_root_skips_empty_socket_env() {
    let proc_root = TempDir::new().expect("Create temp proc root");
    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("Create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("Write cmdline");

    let wanted_state_path = std::env::var("TENEX_STATE_PATH").unwrap_or_default();
    let wanted_state_path = wanted_state_path.trim();
    let environ = format!("TENEX_MUX_SOCKET= \0TENEX_STATE_PATH={wanted_state_path}\0");
    std::fs::write(pid_dir.join("environ"), environ.as_bytes()).expect("Write environ");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert!(sockets.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_in_proc_root_skips_missing_socket_env() {
    let proc_root = TempDir::new().expect("temp proc root");
    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    let wanted_state_path = std::env::var("TENEX_STATE_PATH").unwrap_or_default();
    let wanted_state_path = wanted_state_path.trim();
    std::fs::write(
        pid_dir.join("environ"),
        format!("TENEX_STATE_PATH={wanted_state_path}\0A=1\0").as_bytes(),
    )
    .expect("Write test process environ");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert!(sockets.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_in_proc_root_filters_mismatched_socket_kind() {
    let proc_root = TempDir::new().expect("temp proc root");
    let socket_path = proc_root
        .path()
        .join("good.sock")
        .to_string_lossy()
        .into_owned();
    let socket_namespaced = "tenex-mux-test-good".to_string();

    let wanted_state_path = std::env::var("TENEX_STATE_PATH").unwrap_or_default();
    let wanted_state_path = wanted_state_path.trim();

    let pid_dir = proc_root.path().join("2000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    std::fs::write(
        pid_dir.join("environ"),
        format!("TENEX_MUX_SOCKET={socket_path}\0TENEX_STATE_PATH={wanted_state_path}\0")
            .as_bytes(),
    )
    .expect("Write test process environ");

    let pid_dir = proc_root.path().join("3000");
    std::fs::create_dir_all(&pid_dir).expect("create pid dir");
    std::fs::write(pid_dir.join("cmdline"), b"tenex\0muxd\0").expect("write cmdline");
    std::fs::write(
        pid_dir.join("environ"),
        format!("TENEX_MUX_SOCKET={socket_namespaced}\0TENEX_STATE_PATH={wanted_state_path}\0")
            .as_bytes(),
    )
    .expect("Write test process environ");

    let sockets = running_mux_sockets_in_proc_root(proc_root.path());
    assert_eq!(sockets.len(), 1);
    let expected = HashSet::from([socket_path, socket_namespaced]);
    assert!(expected.contains(&sockets[0]));
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_handles_namespaced_socket_display() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_handles_missing_cmdline_files",
            "--nocapture",
        ])
        .env("TENEX_MUX_SOCKET", "tenex-mux-test-good")
        .status()
        .expect("run test");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_filters_namespaced_socket_display() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_filters_mismatched_state_path",
            "--nocapture",
        ])
        .env("TENEX_MUX_SOCKET", "tenex-mux-test-good")
        .env("TENEX_STATE_PATH", "tenex-test-state")
        .status()
        .expect("run test");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_handles_path_socket_display() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_handles_missing_cmdline_files",
            "--nocapture",
        ])
        .env("TENEX_MUX_SOCKET", "/tmp/tenex-mux-test-good.sock")
        .env("TENEX_STATE_PATH", "tenex-test-state")
        .status()
        .expect("run test");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_filters_path_socket_display() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_filters_mismatched_state_path",
            "--nocapture",
        ])
        .env("TENEX_MUX_SOCKET", "/tmp/tenex-mux-test-good.sock")
        .env("TENEX_STATE_PATH", "tenex-test-state")
        .status()
        .expect("run test");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_filters_state_path_is_optional() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_handles_missing_cmdline_files",
            "--nocapture",
        ])
        .env("TENEX_MUX_SOCKET", "tenex-mux-test-good")
        .env_remove("TENEX_STATE_PATH")
        .status()
        .expect("run test");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_filters_mismatched_state_path_noops_without_state_path_env() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_filters_mismatched_state_path",
            "--nocapture",
        ])
        .env("TENEX_MUX_SOCKET", "tenex-mux-test-good")
        .env_remove("TENEX_STATE_PATH")
        .status()
        .expect("run test");
    assert!(status.success());
}

#[cfg(target_os = "linux")]
#[test]
fn test_running_mux_sockets_skips_empty_socket_env_with_state_path_set() {
    use std::process::Command;

    let current_exe = std::env::current_exe().expect("current exe");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "mux::discovery::tests::test_running_mux_sockets_in_proc_root_skips_empty_socket_env",
            "--nocapture",
        ])
        .env("TENEX_STATE_PATH", "tenex-test-state")
        .status()
        .expect("run test");
    assert!(status.success());
}
