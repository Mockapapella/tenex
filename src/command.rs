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

/// Build an argv vector from a configured program string and an optional prompt.
pub fn build_command_argv(program: &str, prompt: Option<&str>) -> Result<Vec<String>> {
    let mut argv = parse_command_line(program)?;
    if let Some(prompt) = prompt {
        argv.push(prompt.to_string());
    }
    Ok(argv)
}
