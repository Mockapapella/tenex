//! Integration tests for CLI commands
//!
//! These tests require:
//! - the Tenex mux daemon (auto-started)
//! - git to be available
//! - A writable temp directory
//!
//! IMPORTANT: Run with `--test-threads=1` to avoid race conditions from
//! parallel tests calling `std::env::set_current_dir`.

mod common;

mod integration {
    pub mod actions;
    pub mod agent;
    pub mod auto_connect;
    pub mod git;
    pub mod hierarchy;
    pub mod mux;
    pub mod performance;
    pub mod persistence;
    pub mod review;
    pub mod synthesis;
    pub mod workflow;
    pub mod worktree_conflict;
}
