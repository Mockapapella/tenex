//! Runtime-specific command construction and setup.

mod docker;

use crate::agent::{Agent, AgentRuntime};
use crate::app::Settings;
use anyhow::Result;

/// How Tenex is about to launch or relaunch an agent program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLaunch<'a> {
    /// Start a fresh agent session, optionally with an initial prompt.
    Spawn { prompt: Option<&'a str> },
    /// Resume an existing conversation when possible.
    Resume,
}

/// What Tenex still needs to do before Docker mode is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerPreparation {
    /// Docker is available and the shipped worker image is already present.
    Ready,
    /// Docker is available, but the shipped worker image still needs to be built.
    NeedsImageBuild,
}

/// Runtime for newly created root agents.
#[must_use]
pub const fn new_root_runtime(settings: &Settings) -> AgentRuntime {
    if settings.docker_for_new_roots {
        AgentRuntime::Docker
    } else {
        AgentRuntime::Host
    }
}

/// Check whether Docker mode can be enabled immediately or needs a first-use image build.
///
/// # Errors
///
/// Returns an error if Docker is missing, the daemon is unavailable, or the configured programs
/// are not supported by the shipped worker image.
pub fn inspect_docker_runtime(settings: &Settings, programs: &[&str]) -> Result<DockerPreparation> {
    docker::check_available()?;

    let mut needs_image_build = false;
    for program in programs {
        needs_image_build |= docker::image_build_required(settings, program)?;
    }

    Ok(if needs_image_build {
        DockerPreparation::NeedsImageBuild
    } else {
        DockerPreparation::Ready
    })
}

/// Check whether Docker is available and the needed Docker image can be used.
///
/// # Errors
///
/// Returns an error if Docker is missing, the daemon is unavailable, or the needed image cannot
/// be prepared for the provided programs.
pub fn prepare_docker_runtime(settings: &Settings, programs: &[&str]) -> Result<()> {
    docker::check_available()?;
    for program in programs {
        docker::ensure_image_ready(settings, program)?;
    }
    Ok(())
}

/// Build the command argv for launching an agent in its configured runtime.
///
/// # Errors
///
/// Returns an error if the configured program or runtime cannot produce a valid argv.
pub fn build_agent_command(
    agent: &Agent,
    launch: AgentLaunch<'_>,
    settings: &Settings,
) -> Result<Vec<String>> {
    let base = match launch {
        AgentLaunch::Spawn { prompt } => crate::conversation::build_spawn_argv(
            &agent.program,
            prompt,
            agent.conversation_id.as_deref(),
        )?,
        AgentLaunch::Resume => {
            if let Some(conversation_id) = agent.conversation_id.as_deref() {
                crate::conversation::build_resume_argv(&agent.program, conversation_id)?
            } else {
                crate::conversation::build_spawn_argv(&agent.program, None, None)?
            }
        }
    };

    match agent.runtime {
        AgentRuntime::Host => Ok(base),
        AgentRuntime::Docker => Ok(docker::wrap_exec(agent, settings, &base)),
    }
}

/// Build the command argv for launching a terminal in the configured runtime.
///
/// Host terminals keep using the mux daemon's default shell. Docker terminals must explicitly
/// enter the container and start a shell there.
///
pub fn build_terminal_command(
    agent: &Agent,
    startup_command: Option<&str>,
    settings: &Settings,
) -> Option<Vec<String>> {
    match agent.runtime {
        AgentRuntime::Host => None,
        AgentRuntime::Docker => {
            let shell = startup_command.map_or_else(
                || vec!["bash".to_string(), "-i".to_string()],
                |startup_command| {
                    vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        format!("{startup_command}; exec bash -i"),
                    ]
                },
            );
            Some(docker::wrap_exec(agent, settings, &shell))
        }
    }
}

/// Ensure the runtime backing this agent is ready before Tenex tries to launch it.
///
/// # Errors
///
/// Returns an error if the Docker container cannot be inspected, started, or created.
pub fn ensure_runtime_ready(agent: &Agent, settings: &Settings) -> Result<()> {
    match agent.runtime {
        AgentRuntime::Host => Ok(()),
        AgentRuntime::Docker => docker::ensure_container(agent, settings),
    }
}

/// Best-effort cleanup for runtime resources owned by this agent tree.
///
/// # Errors
///
/// Returns an error if the Docker container could not be removed.
pub fn cleanup_runtime(agent: &Agent) -> Result<()> {
    match agent.runtime {
        AgentRuntime::Host => Ok(()),
        AgentRuntime::Docker => docker::remove_container(agent),
    }
}

/// Filesystem path Codex records for this agent's session metadata.
#[must_use]
pub fn codex_session_workdir(agent: &Agent) -> std::path::PathBuf {
    match agent.runtime {
        AgentRuntime::Host => agent.worktree_path.clone(),
        AgentRuntime::Docker => docker::session_workdir(agent),
    }
}

#[cfg(test)]
pub fn with_docker_program_override_for_tests<T>(
    program: std::path::PathBuf,
    f: impl FnOnce() -> T,
) -> T {
    docker::with_docker_program_override_for_tests(program, f)
}

#[cfg(test)]
mod tests {
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
    fn test_build_agent_command_host_passthrough() -> Result<()> {
        let agent = host_agent();
        let argv = build_agent_command(
            &agent,
            AgentLaunch::Spawn {
                prompt: Some("fix bug"),
            },
            &Settings::default(),
        )?;

        assert_eq!(argv, vec!["codex".to_string(), "fix bug".to_string()]);
        Ok(())
    }

    #[test]
    fn test_build_agent_command_docker_wraps_exec() -> Result<()> {
        let agent = docker_agent();
        let argv = build_agent_command(
            &agent,
            AgentLaunch::Spawn {
                prompt: Some("fix bug"),
            },
            &Settings::default(),
        )?;

        assert_eq!(argv[0], "docker");
        assert_eq!(argv[1], "exec");
        assert!(argv.iter().any(|arg| arg == "codex"));
        assert!(argv.iter().any(|arg| arg == "fix bug"));
        Ok(())
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
}
