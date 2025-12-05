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
fn test_cli_invalid_argument_shows_help() {
    let output = muster_bin().arg("--invalid-flag").output().unwrap();

    // Should fail with non-zero exit code
    assert!(!output.status.success());

    // Should show error message on stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));

    // Should show help text on stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
}

#[test]
fn test_cli_unexpected_argument_shows_help() {
    // Simulates typo like `--set` instead of `--set-agent`
    let output = muster_bin().args(["--set", "codex"]).output().unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));

    // Help on stdout should show the correct flag
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--set-agent"));
}

#[test]
fn test_cli_set_agent() {
    use std::fs;
    use tempfile::TempDir;

    // Create a temp directory for config
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path().join("muster");
    fs::create_dir_all(&config_dir).unwrap();

    // Run with XDG_CONFIG_HOME set to temp directory
    let output = muster_bin()
        .args(["--set-agent", "test-agent"])
        .env("XDG_CONFIG_HOME", temp_dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Default agent set to: test-agent"));

    // Verify config file was created with correct value
    let config_path = config_dir.join("config.json");
    let config_content = fs::read_to_string(&config_path).unwrap();
    assert!(config_content.contains("test-agent"));
}

#[test]
fn test_cli_reset_force() {
    // reset with --force should succeed (even if no agents)
    let output = muster_bin().args(["reset", "--force"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // "No agents to reset", or lists agents/orphaned sessions
    assert!(
        stdout.contains("No agents")
            || stdout.contains("Reset complete")
            || stdout.contains("Agents to kill")
            || stdout.contains("Orphaned")
    );
}
