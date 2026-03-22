//! Tenex - Terminal multiplexer for AI coding agents
//!
//! Tenex allows you to run multiple AI agents in parallel, each in isolated
//! git worktrees, with a TUI for managing and monitoring them.

#[cfg(not(any(unix, windows)))]
compile_error!("Tenex supports Linux, macOS, and Windows.");

mod command;
pub(crate) mod conversation;

pub mod action;
pub mod agent;
pub mod app;
pub mod config;
pub mod git;
pub mod migration;
pub mod mux;
pub mod paths;
pub mod prompts;
pub mod release_notes;
pub(crate) mod runtime;
pub mod state;
#[cfg(test)]
pub(crate) mod test_support;
pub mod tui;
pub mod update;

pub use agent::{Agent, Status};
pub use app::{App, Tab};
pub use config::Config;
pub use state::AppMode;

/// Best-effort cleanup for runtime resources owned by this agent.
///
/// # Errors
///
/// Returns an error if the agent's runtime resources could not be removed.
pub fn cleanup_agent_runtime(agent: &Agent) -> anyhow::Result<()> {
    runtime::cleanup_runtime(agent)
}
