//! Binary integration tests for CLI commands
//!
//! These tests run the actual tenex binary to exercise the CLI code paths.

use std::process::Command;

fn tenex_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tenex"))
}

#[test]
fn test_cli_help() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("--help").output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Terminal multiplexer"));
    Ok(())
}

#[test]
fn test_cli_version() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("--version").output()?;
    assert!(output.status.success());
    Ok(())
}

#[test]
fn test_cli_config_show() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("config").output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("default_program"));
    Ok(())
}

#[test]
fn test_cli_config_path() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().args(["config", "--path"]).output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("tenex"));
    Ok(())
}

#[test]
fn test_cli_invalid_argument_shows_help() -> Result<(), Box<dyn std::error::Error>> {
    let output = tenex_bin().arg("--invalid-flag").output()?;

    // Should fail with non-zero exit code
    assert!(!output.status.success());

    // Should show error message on stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));

    // Should show help text on stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    Ok(())
}

#[test]
fn test_cli_unexpected_argument_shows_help() -> Result<(), Box<dyn std::error::Error>> {
    // Simulates typo like `--set` instead of `--set-agent`
    let output = tenex_bin().args(["--set", "codex"]).output()?;

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));

    // Help on stdout should show the correct flag
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--set-agent"));
    Ok(())
}

#[test]
fn test_cli_set_agent() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use tempfile::TempDir;

    // Create a temp directory for config
    let temp_dir = TempDir::new()?;
    let config_dir = temp_dir.path().join("tenex");
    fs::create_dir_all(&config_dir)?;

    // Run with XDG_CONFIG_HOME set to temp directory
    let output = tenex_bin()
        .args(["--set-agent", "test-agent"])
        .env("XDG_CONFIG_HOME", temp_dir.path())
        .output()?;

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Default agent set to: test-agent"));

    // Verify config file was created with correct value
    let config_path = config_dir.join("config.json");
    let config_content = fs::read_to_string(&config_path)?;
    assert!(config_content.contains("test-agent"));
    Ok(())
}

#[test]
fn test_cli_reset_force() -> Result<(), Box<dyn std::error::Error>> {
    // reset with --force should succeed (even if no agents)
    let output = tenex_bin().args(["reset", "--force"]).output()?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // "No agents to reset", or lists agents/orphaned sessions
    assert!(
        stdout.contains("No agents")
            || stdout.contains("Reset complete")
            || stdout.contains("Agents to kill")
            || stdout.contains("Orphaned")
    );
    Ok(())
}
