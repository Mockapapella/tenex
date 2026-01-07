//! Tenex - Terminal multiplexer for AI coding agents
//!
//! Tenex allows you to run multiple AI agents in parallel, each in isolated
//! git worktrees, with a TUI for managing and monitoring them.

#[cfg(not(target_os = "linux"))]
compile_error!("Tenex currently supports Linux only.");

mod command;

pub mod action;
pub mod agent;
pub mod app;
pub mod config;
pub mod git;
pub mod migration;
pub mod mux;
pub mod paths;
pub mod prompts;
pub mod state;
pub mod tui;
pub mod update;

pub use agent::{Agent, Status};
pub use app::{App, Tab};
pub use config::Config;
pub use state::AppMode;
