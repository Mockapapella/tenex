//! Tenex - Terminal multiplexer for AI coding agents
//!
//! Tenex allows you to run multiple AI agents in parallel, each in isolated
//! git worktrees, with a TUI for managing and monitoring them.

pub mod agent;
pub mod app;
pub mod config;
pub mod git;
pub mod prompts;
pub mod tmux;
pub mod ui;

pub use agent::{Agent, Status};
pub use app::{App, Mode, Tab};
pub use config::Config;
