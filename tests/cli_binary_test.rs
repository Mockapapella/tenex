//! Binary integration tests for CLI commands
//!
//! These tests run the actual muster binary to exercise the CLI code paths.

#![expect(clippy::unwrap_used, reason = "integration test assertions")]

use std::process::Command;

fn muster_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_muster"))
}

#[test]
fn test_cli_help() {
    let output = muster_bin().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Terminal multiplexer"));
}

#[test]
fn test_cli_version() {
    let output = muster_bin().arg("--version").output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_cli_list_empty() {
    let output = muster_bin().arg("list").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Either shows "No agents" or the header
    assert!(stdout.contains("No agents") || stdout.contains("ID"));
}

#[test]
fn test_cli_config_show() {
    let output = muster_bin().arg("config").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("default_program"));
}

#[test]
fn test_cli_config_path() {
    let output = muster_bin().args(["config", "--path"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("muster"));
}

#[test]
fn test_cli_reset_force() {
    // Reset with force should succeed (even if no agents)
    let output = muster_bin().args(["reset", "--force"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_cli_attach_nonexistent() {
    let output = muster_bin()
        .args(["attach", "nonexistent"])
        .output()
        .unwrap();
    // Should fail because agent doesn't exist
    assert!(!output.status.success());
}

#[test]
fn test_cli_kill_nonexistent() {
    let output = muster_bin().args(["kill", "nonexistent"]).output().unwrap();
    // Should fail because agent doesn't exist
    assert!(!output.status.success());
}

#[test]
fn test_cli_pause_nonexistent() {
    let output = muster_bin()
        .args(["pause", "nonexistent"])
        .output()
        .unwrap();
    // Should fail because agent doesn't exist
    assert!(!output.status.success());
}

#[test]
fn test_cli_resume_nonexistent() {
    let output = muster_bin()
        .args(["resume", "nonexistent"])
        .output()
        .unwrap();
    // Should fail because agent doesn't exist
    assert!(!output.status.success());
}

#[test]
fn test_cli_list_running_only() {
    let output = muster_bin().args(["list", "--running"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_cli_with_program_flag() {
    let output = muster_bin()
        .args(["--program", "echo", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn test_cli_with_auto_yes_flag() {
    let output = muster_bin().args(["-y", "list"]).output().unwrap();
    assert!(output.status.success());
}
