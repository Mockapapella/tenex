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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command_line_rejects_empty() {
        let result = parse_command_line("   ");
        assert!(result.is_err());
        if let Err(error) = result {
            let message = format!("{error}");
            assert!(message.contains("Command line is empty"));
        }
    }

    #[test]
    fn test_parse_command_line_splits_args() -> Result<(), Box<dyn std::error::Error>> {
        let argv = parse_command_line(r#"echo "hello world""#)?;
        assert_eq!(argv, vec!["echo".to_string(), "hello world".to_string()]);
        Ok(())
    }

    #[test]
    fn test_parse_command_line_comment_is_empty() {
        let result = parse_command_line("# comment only");
        assert!(result.is_err());
        if let Err(error) = result {
            let message = format!("{error}");
            assert!(message.contains("Command line produced no argv items"));
        }
    }

    // `build_spawn_argv` lives in `crate::conversation` and covers Tenex's current spawn needs.
}
