//! Tenex - Terminal multiplexer for AI coding agents
//!
//! Tenex allows you to run multiple AI agents in parallel, each in isolated
//! git worktrees, with a TUI for managing and monitoring them.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
#![cfg_attr(
    test,
    expect(clippy::unwrap_used, reason = "unit tests use unwrap for assertions")
)]
#![cfg_attr(
    test,
    expect(clippy::expect_used, reason = "unit tests use expect for assertions")
)]
#![cfg_attr(
    test,
    expect(
        clippy::large_stack_arrays,
        reason = "Unit tests may allocate large scratch buffers; production builds forbid them."
    )
)]
#![cfg_attr(
    test,
    expect(
        clippy::significant_drop_tightening,
        reason = "Work around a clippy ICE in nightly-2025-11-07."
    )
)]

#[cfg(not(any(unix, windows)))]
compile_error!("Tenex supports Linux, macOS, and Windows.");

mod command;
pub mod conversation;

pub mod action;
pub mod agent;
pub mod app;
pub mod cli;
pub mod config;
pub mod git;
pub mod migration;
pub mod mux;
pub mod paths;
pub mod prompts;
pub mod release_notes;
pub(crate) mod runtime;
pub mod state;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
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
