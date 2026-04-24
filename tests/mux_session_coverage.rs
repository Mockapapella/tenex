//! Coverage tests for mux session code paths in non-unit-test builds.
#![cfg(feature = "test-support")]

use interprocess::local_socket::traits::ListenerExt as _;
use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tempfile::TempDir;
use tenex::agent::{Agent, AgentRuntime};
use tenex::mux::{MuxRequest, MuxResponse, SessionManager, read_json, set_socket_override, write_json};

static RUN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static MOCK_MUX: OnceLock<MockMux> = OnceLock::new();

fn run_lock() -> &'static Mutex<()> {
    RUN_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Default)]
struct ServerState {
    pane_cmd: String,
    inputs: Vec<Vec<u8>>,
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
                    let Ok(request) = read_json::<_, MuxRequest>(&mut stream) else {
                        break;
                    };

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
                        MuxRequest::CreateSession { .. } => MuxResponse::Ok,
                        _ => MuxResponse::Ok,
                    };

                    let _ = write_json(&mut stream, &response);
                }
            }
        });

        MockMux { state }
    })
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
