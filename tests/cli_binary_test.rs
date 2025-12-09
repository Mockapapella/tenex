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

#[test]
fn test_log_file_cleared_on_startup() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::Path;

    let log_path = Path::new("/tmp/tenex.log");

    // Write some content to the log file to simulate previous session logs
    fs::write(log_path, "previous session log content\nmore log lines\n")?;

    // Verify the file has content
    let content_before = fs::read_to_string(log_path)?;
    assert!(
        !content_before.is_empty(),
        "Log file should have content before test"
    );

    // Run tenex with --help (a quick command that exits immediately)
    let output = tenex_bin().arg("--help").output()?;
    assert!(output.status.success());

    // Verify the log file was cleared
    let content_after = fs::read_to_string(log_path)?;
    assert!(
        content_after.is_empty(),
        "Log file should be empty after tenex startup, but contained: {content_after}"
    );

    Ok(())
}
