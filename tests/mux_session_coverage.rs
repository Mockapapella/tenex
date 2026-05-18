//! Coverage tests for mux session code paths in non-unit-test builds.
#![cfg(feature = "test-support")]
#![expect(
    clippy::expect_used,
    reason = "coverage tests assert fixture setup directly"
)]
#![expect(
    clippy::significant_drop_tightening,
    reason = "mock server state locks stay local to short assertions"
)]

use interprocess::local_socket::traits::ListenerExt as _;
use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tempfile::TempDir;
use tenex::agent::{Agent, AgentRuntime};
#[cfg(coverage)]
use tenex::mux::exercise_len_prefixed_payload_length_for_tests;
use tenex::mux::{
    MuxRequest, MuxResponse, SessionManager, read_json, set_socket_override, write_json,
};

static RUN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static MOCK_MUX: OnceLock<MockMux> = OnceLock::new();

fn run_lock() -> &'static Mutex<()> {
    RUN_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Default)]
struct ServerState {
    pane_cmd: String,
    inputs: Vec<Vec<u8>>,
    response_override: Option<MuxResponse>,
}

struct MockMux {
    state: Arc<Mutex<ServerState>>,
}

fn mock_mux() -> &'static MockMux {
    MOCK_MUX.get_or_init(|| {
        let pid = std::process::id();
        let socket_path = PathBuf::from("/tmp")
            .join(format!("tx-mux-session-cov-{pid}-{}.sock", uuid::Uuid::new_v4()));
        let socket_display = socket_path.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&socket_path);

        let socket_name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .expect("Expected filesystem socket name")
            .into_owned();

        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .expect("Expected mock listener");

        set_socket_override(&socket_display).expect("Expected socket override");

        let state: Arc<Mutex<ServerState>> = Arc::new(Mutex::new(ServerState::default()));
        let state_server = Arc::clone(&state);

        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                loop {
                    let Ok(request) = read_json::<MuxRequest>(&mut stream) else {
                        break;
                    };

                    let response_override = state_server
                        .lock()
                        .expect("lock")
                        .response_override
                        .take();
                    if let Some(response) = response_override {
                        let _ = write_json(&mut stream, &response);
                        continue;
                    }

                    let response = match request {
                        MuxRequest::PaneCurrentCommand { .. } => MuxResponse::Text {
                            text: state_server.lock().expect("lock").pane_cmd.clone(),
                        },
                        MuxRequest::ListSessions => serde_json::from_str(
                            r#"{"Sessions":{"sessions":[{"name":"cov-session","created":0,"attached":false}]}}"#,
                        )
                        .expect("Expected sessions response"),
                        MuxRequest::SendInput { data, .. } => {
                            state_server.lock().expect("lock").inputs.push(data);
                            MuxResponse::Ok
                        }
                        MuxRequest::CreateWindow { .. } => MuxResponse::WindowCreated { index: 7 },
                        MuxRequest::ListWindows { .. } => serde_json::from_str(
                            r#"{"Windows":{"windows":[{"index":7,"name":"cov-window"}]}}"#,
                        )
                        .expect("Expected windows response"),
                        MuxRequest::ListPanePids { .. } => MuxResponse::Pids {
                            pids: vec![11, 22],
                        },
                        _ => MuxResponse::Ok,
                    };

                    let _ = write_json(&mut stream, &response);
                }
            }
        });

        MockMux { state }
    })
}

fn set_next_response(mock: &MockMux, response: MuxResponse) {
    mock.state.lock().expect("lock").response_override = Some(response);
}

#[test]
fn test_session_manager_create_covers_absolute_and_relative_workdirs() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let _ = mock_mux();
    let manager = SessionManager::new();
    let tmp = TempDir::new().expect("Expected temp dir");
    manager
        .create("cov-session-abs", tmp.path(), None)
        .expect("Expected create");

    // Use a missing relative path so canonicalize fails and the join path is preserved.
    let relative = format!("missing-{}", uuid::Uuid::new_v4());
    manager
        .create("cov-session-rel", std::path::Path::new(&relative), None)
        .expect("Expected create");
}

#[test]
fn test_session_manager_attach_command_is_callable_in_non_test_build() {
    let cmd = SessionManager::attach_command("cov-session");
    assert_eq!(cmd, "tenex attach --session cov-session");
}

#[test]
fn test_session_manager_attach_returns_error_in_non_test_build() {
    let err = SessionManager::new()
        .attach("cov-session")
        .expect_err("Expected attach to error");
    assert!(err.to_string().contains("Attach is not supported"));
}

#[cfg(coverage)]
#[test]
fn test_ipc_length_boundary_rejects_oversized_payload_in_non_test_build() {
    let err = exercise_len_prefixed_payload_length_for_tests(u32::MAX as usize + 1)
        .expect_err("oversized payload length should fail");
    assert!(err.to_string().contains("Message too large"));
}

#[test]
fn test_session_manager_list_maps_sessions_response_in_non_test_build() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let _ = mock_mux();
    let sessions = SessionManager::new()
        .list()
        .expect("Expected list sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].name, "cov-session");
}

#[test]
fn test_session_manager_boundary_responses_in_non_test_build() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    let manager = SessionManager::new();
    let tmp = TempDir::new().expect("Expected temp dir");

    set_next_response(
        mock,
        MuxResponse::Err {
            message: "create failed".to_string(),
        },
    );
    let err = manager
        .create("cov-session-error", tmp.path(), None)
        .expect_err("Expected create error response");
    assert!(err.to_string().contains("create failed"));

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .create("cov-session-unexpected", tmp.path(), None)
        .expect_err("Expected create unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .kill("cov-session")
        .expect_err("Expected kill unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    set_next_response(
        mock,
        MuxResponse::Err {
            message: "exists failed".to_string(),
        },
    );
    let err = manager
        .try_exists("cov-session")
        .expect_err("Expected exists error response");
    assert!(err.to_string().contains("exists failed"));

    set_next_response(mock, MuxResponse::Ok);
    let err = manager
        .try_exists("cov-session")
        .expect_err("Expected exists unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    set_next_response(
        mock,
        MuxResponse::Err {
            message: "list failed".to_string(),
        },
    );
    let err = manager.list().expect_err("Expected list error response");
    assert!(err.to_string().contains("list failed"));

    set_next_response(mock, MuxResponse::Ok);
    let err = manager
        .list()
        .expect_err("Expected list unexpected response");
    assert!(err.to_string().contains("Unexpected response"));
}

#[test]
fn test_session_manager_window_and_input_responses_in_non_test_build() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    let manager = SessionManager::new();
    let tmp = TempDir::new().expect("Expected temp dir");

    let index = manager
        .create_window("cov-session", "cov-window", tmp.path(), None)
        .expect("Expected window create");
    assert_eq!(index, 7);

    set_next_response(mock, MuxResponse::Ok);
    let err = manager
        .create_window("cov-session", "cov-window", tmp.path(), None)
        .expect_err("Expected create window unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    manager
        .kill_window("cov-session", 7)
        .expect("Expected window kill");

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .kill_window("cov-session", 7)
        .expect_err("Expected kill window unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    let windows = manager
        .list_windows("cov-session")
        .expect("Expected list windows");
    assert_eq!(windows[0].name, "cov-window");

    set_next_response(
        mock,
        MuxResponse::Err {
            message: "windows failed".to_string(),
        },
    );
    let err = manager
        .list_windows("cov-session")
        .expect_err("Expected list windows error response");
    assert!(err.to_string().contains("windows failed"));

    set_next_response(mock, MuxResponse::Ok);
    let err = manager
        .list_windows("cov-session")
        .expect_err("Expected list windows unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    let pids = manager
        .list_pane_pids("cov-session")
        .expect("Expected pane PIDs");
    assert_eq!(pids, [11, 22]);

    set_next_response(mock, MuxResponse::Ok);
    let err = manager
        .list_pane_pids("cov-session")
        .expect_err("Expected pane PIDs unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    manager
        .resize_window("cov-session:7", 80, 24)
        .expect("Expected resize");

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .resize_window("cov-session:7", 80, 24)
        .expect_err("Expected resize unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    manager
        .rename_window("cov-session", 7, "renamed-window")
        .expect("Expected window rename");

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .rename_window("cov-session", 7, "renamed-window")
        .expect_err("Expected window rename unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    manager
        .rename("cov-session", "renamed-session")
        .expect("Expected session rename");

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .rename("cov-session", "renamed-session")
        .expect_err("Expected session rename unexpected response");
    assert!(err.to_string().contains("Unexpected response"));

    set_next_response(mock, MuxResponse::Bool { value: true });
    let err = manager
        .send_keys("cov-session:7", "hi")
        .expect_err("Expected send unexpected response");
    assert!(err.to_string().contains("Unexpected response"));
}

#[test]
fn test_session_manager_send_keys_for_program_uses_paste_when_pane_is_codex() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    {
        let mut state = mock.state.lock().expect("lock");
        state.pane_cmd = "codex".to_string();
        state.inputs.clear();
    }

    SessionManager::new()
        .send_keys_and_submit_for_program("session:0", "codex", "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 1);
    assert!(state.inputs[0].starts_with(b"\x1b[200~"));
    assert!(state.inputs[0].ends_with(b"\x1b[201~\r"));
}

#[test]
fn test_session_manager_send_keys_for_program_falls_back_when_pane_is_not_codex() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    {
        let mut state = mock.state.lock().expect("lock");
        state.pane_cmd = "bash".to_string();
        state.inputs.clear();
    }

    SessionManager::new()
        .send_keys_and_submit_for_program("session:0", "codex", "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 1);
    assert_eq!(state.inputs[0], b"hi\r");
}

#[test]
fn test_session_manager_send_keys_for_program_falls_back_when_pane_command_errors() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    {
        let mut state = mock.state.lock().expect("lock");
        state.inputs.clear();
        state.response_override = Some(MuxResponse::Err {
            message: "pane command failed".to_string(),
        });
    }

    SessionManager::new()
        .send_keys_and_submit_for_program("session:0", "codex", "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 1);
    assert_eq!(state.inputs[0], b"hi\r");
}

#[test]
fn test_session_manager_send_keys_for_non_codex_program_uses_normal_submit() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    {
        let mut state = mock.state.lock().expect("lock");
        state.inputs.clear();
    }

    SessionManager::new()
        .send_keys_and_submit_for_program("session:0", "bash", "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 1);
    assert_eq!(state.inputs[0], b"hi\r");
}

#[test]
fn test_session_manager_claude_submit_propagates_first_send_error() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    set_next_response(
        mock,
        MuxResponse::Err {
            message: "send failed".to_string(),
        },
    );

    let err = SessionManager::new()
        .send_keys_and_submit_for_program("session:0", "claude", "hi")
        .expect_err("Expected first send error");

    assert!(err.to_string().contains("send failed"));
}

#[test]
fn test_session_manager_send_keys_for_docker_codex_agent_uses_paste() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    {
        let mut state = mock.state.lock().expect("lock");
        state.inputs.clear();
    }

    let mut agent = Agent::new(
        "Codex".to_string(),
        "codex".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    agent.runtime = AgentRuntime::Docker;

    SessionManager::new()
        .send_keys_and_submit_for_agent("session:0", &agent, "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 1);
    assert!(state.inputs[0].starts_with(b"\x1b[200~"));
    assert!(state.inputs[0].ends_with(b"\x1b[201~\r"));
}

#[test]
fn test_session_manager_send_keys_for_host_codex_agent_uses_program_path() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    {
        let mut state = mock.state.lock().expect("lock");
        state.pane_cmd = "codex".to_string();
        state.inputs.clear();
    }

    let mut agent = Agent::new(
        "Codex".to_string(),
        "codex".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );
    agent.runtime = AgentRuntime::Host;

    SessionManager::new()
        .send_keys_and_submit_for_agent("session:0", &agent, "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 1);
    assert!(state.inputs[0].starts_with(b"\x1b[200~"));
    assert!(state.inputs[0].ends_with(b"\x1b[201~\r"));
}

#[test]
fn test_session_manager_send_keys_for_claude_agent_uses_csi_u_enter() {
    let _guard = run_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mock = mock_mux();
    mock.state.lock().expect("lock").inputs.clear();

    let agent = Agent::new(
        "Claude".to_string(),
        "claude".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp"),
    );

    SessionManager::new()
        .send_keys_and_submit_for_agent("session:0", &agent, "hi")
        .expect("Expected send keys");

    let state = mock.state.lock().expect("lock");
    assert_eq!(state.inputs.len(), 2);
    assert_eq!(state.inputs[0], b"hi");
    assert_eq!(state.inputs[1], b"\x1b[13;1u");
}
