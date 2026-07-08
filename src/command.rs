//! Command line parsing helpers.

use anyhow::{Context, Result, bail};

/// Split a command line into an argv vector.
///
/// This uses Unix shell-style quoting rules. Callers should treat the returned
/// vector as an executable + arguments (not as a shell script).
pub fn parse_command_line(command_line: &str) -> Result<Vec<String>> {
    let trimmed = command_line.trim();
    if trimmed.is_empty() {
        bail!("Command line is empty");
    }

    let argv = shell_words::split(trimmed).context("Failed to parse command line")?;
    if argv.is_empty() {
        bail!("Command line produced no argv items");
    }

    Ok(argv)
}
