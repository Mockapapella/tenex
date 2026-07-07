use super::*;
use crate::app::Settings;
use std::path::PathBuf;

fn host_agent() -> Agent {
    Agent::new(
        "Host".to_string(),
        "codex".to_string(),
        "agent/host".to_string(),
        PathBuf::from("/tmp/tenex-runtime-host"),
    )
}

fn docker_agent() -> Agent {
    let mut agent = host_agent();
    agent.runtime = AgentRuntime::Docker;
    agent.mux_session = "tenex-abcd1234-root".to_string();
    agent
}

#[test]
fn test_new_root_runtime_defaults_to_host() {
    assert_eq!(new_root_runtime(&Settings::default()), AgentRuntime::Host);
}

#[test]
fn test_new_root_runtime_uses_docker_toggle() {
    let settings = Settings {
        docker_for_new_roots: true,
        ..Settings::default()
    };

    assert_eq!(new_root_runtime(&settings), AgentRuntime::Docker);
}

#[test]
fn test_build_agent_command_host_passthrough() {
    let agent = host_agent();
    let argv = build_agent_command(
        &agent,
        AgentLaunch::Spawn {
            prompt: Some("fix bug"),
        },
        &Settings::default(),
    )
    .expect("build agent command");

    assert_eq!(argv, vec!["codex".to_string(), "fix bug".to_string()]);
}

#[test]
fn test_build_agent_command_docker_wraps_exec() {
    let agent = docker_agent();
    let argv = build_agent_command(
        &agent,
        AgentLaunch::Spawn {
            prompt: Some("fix bug"),
        },
        &Settings::default(),
    )
    .expect("build agent command");

    assert_eq!(argv[0], "docker");
    assert_eq!(argv[1], "exec");
    assert!(argv.iter().any(|arg| arg == "codex"));
    assert!(argv.iter().any(|arg| arg == "fix bug"));
}

#[test]
fn test_build_terminal_command_host_returns_none() {
    let agent = host_agent();
    assert!(build_terminal_command(&agent, None, &Settings::default()).is_none());
}

#[test]
fn test_build_terminal_command_docker_enters_container() {
    let agent = docker_agent();
    let argv = build_terminal_command(&agent, Some("cargo test"), &Settings::default())
        .unwrap_or_default();

    assert_eq!(argv[0], "docker");
    assert!(argv.iter().any(|arg| arg == "bash"));
    assert!(argv.iter().any(|arg| arg == "cargo test; exec bash -i"));
}

#[test]
fn test_build_terminal_command_docker_starts_shell_when_no_startup_command() {
    let agent = docker_agent();
    let argv = build_terminal_command(&agent, None, &Settings::default()).unwrap_or_default();

    assert_eq!(argv[0], "docker");
    assert!(argv.iter().any(|arg| arg == "bash"));
    assert!(argv.iter().any(|arg| arg == "-i"));
}

#[cfg(unix)]
use std::fs;

#[cfg(unix)]
use tempfile::TempDir;

#[cfg(unix)]
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(unix)]
fn default_worker_dockerfile_hash() -> String {
    let dockerfile = include_str!("../../docker/worker.Dockerfile");
    format!("{:016x}", fnv1a64(dockerfile.as_bytes()))
}

#[cfg(unix)]
fn write_fake_docker_script(temp: &TempDir, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let script = temp.path().join("docker");
    fs::write(&script, body).expect("write docker script");
    let mut perms = fs::metadata(&script)
        .expect("read docker script metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).expect("set docker script permissions");
    script
}

#[cfg(unix)]
#[test]
fn test_inspect_docker_runtime_reports_ready_when_default_image_is_current() {
    let temp = TempDir::new().expect("temp dir");
    let hash = default_worker_dockerfile_hash();
    let script = write_fake_docker_script(
        &temp,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{hash}'\n  exit 0\nfi\nexit 0\n"
        ),
    );

    with_docker_program_override_for_tests(script, || {
        let result = inspect_docker_runtime(&Settings::default(), &["codex"])
            .expect("inspect docker runtime");
        assert_eq!(result, DockerPreparation::Ready);
    });
}

#[cfg(unix)]
#[test]
fn test_prepare_docker_runtime_succeeds_when_default_image_is_current() {
    let temp = TempDir::new().expect("temp dir");
    let hash = default_worker_dockerfile_hash();
    let script = write_fake_docker_script(
        &temp,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '{hash}'\n  exit 0\nfi\nexit 0\n"
        ),
    );

    with_docker_program_override_for_tests(script, || {
        prepare_docker_runtime(&Settings::default(), &["codex"]).expect("prepare docker runtime");
    });
}

#[cfg(unix)]
#[test]
fn test_prepare_docker_runtime_propagates_check_available_failures() {
    let temp = TempDir::new().expect("temp dir");
    let script = write_fake_docker_script(
        &temp,
        "#!/bin/sh\nif [ \"$1\" = \"version\" ]; then\n  echo 'boom' >&2\n  exit 1\nfi\nexit 0\n",
    );

    with_docker_program_override_for_tests(script, || {
        let error = prepare_docker_runtime(&Settings::default(), &["codex"]).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Docker"));
    });
}

#[cfg(unix)]
#[test]
fn test_prepare_docker_runtime_propagates_ensure_image_ready_failures() {
    let temp = TempDir::new().expect("temp dir");
    let script = write_fake_docker_script(&temp, "#!/bin/sh\nexit 0\n");

    with_docker_program_override_for_tests(script, || {
        let error = prepare_docker_runtime(&Settings::default(), &["custom"]).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("custom"));
    });
}

#[test]
fn test_build_agent_command_spawn_propagates_program_parse_errors() {
    let mut agent = host_agent();
    agent.program = "   ".to_string();

    let error = build_agent_command(
        &agent,
        AgentLaunch::Spawn { prompt: None },
        &Settings::default(),
    )
    .unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("Command line is empty"));
}

#[test]
fn test_build_agent_command_resume_propagates_resume_parse_errors() {
    let mut agent = host_agent();
    agent.program = "   ".to_string();
    agent.conversation_id = Some("conversation-id".to_string());

    let error = build_agent_command(&agent, AgentLaunch::Resume, &Settings::default()).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("Command line is empty"));
}

#[test]
fn test_build_agent_command_resume_without_conversation_id_propagates_program_parse_errors() {
    let mut agent = host_agent();
    agent.program = "   ".to_string();
    agent.conversation_id = None;

    let error = build_agent_command(&agent, AgentLaunch::Resume, &Settings::default()).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("Command line is empty"));
}
