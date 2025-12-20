//! Tenex - Terminal multiplexer for AI coding agents
//!
//! Tenex allows you to run multiple AI agents in parallel, each in isolated
//! git worktrees, with a TUI for managing and monitoring them.

mod command;

pub mod agent;
pub mod app;
pub mod config;
pub mod git;
pub mod mux;
pub mod paths;
pub mod prompts;
pub mod ui;
pub mod update;

pub use agent::{Agent, Status};
pub use app::{App, Mode, Tab};
pub use config::Config;
