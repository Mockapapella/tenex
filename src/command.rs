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
    #[cfg(windows)]
    {
        fn powershell_quote(arg: &str) -> String {
            let escaped = arg.replace('\'', "''");
            format!("'{escaped}'")
        }

        let mut should_wrap = false;

        if let Some(cmd) = argv.first().cloned() {
            let path = std::path::Path::new(&cmd);
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase());
            let mut replacement: Option<String> = None;

            match ext.as_deref() {
                Some("cmd") | Some("bat") => {
                    should_wrap = true;
                }
                Some(_) => {}
                None => {
                    let mut select_in_dir = |dir: &std::path::Path| -> bool {
                        let exe = dir.join(format!("{cmd}.exe"));
                        if exe.is_file() {
                            replacement = Some(exe.to_string_lossy().into_owned());
                            should_wrap = false;
                            return true;
                        }

                        let com = dir.join(format!("{cmd}.com"));
                        if com.is_file() {
                            replacement = Some(com.to_string_lossy().into_owned());
                            should_wrap = false;
                            return true;
                        }

                        if replacement.is_none() {
                            let cmd_path = dir.join(format!("{cmd}.cmd"));
                            if cmd_path.is_file() {
                                replacement = Some(cmd_path.to_string_lossy().into_owned());
                                should_wrap = true;
                            }

                            let bat_path = dir.join(format!("{cmd}.bat"));
                            if bat_path.is_file() {
                                replacement = Some(bat_path.to_string_lossy().into_owned());
                                should_wrap = true;
                            }
                        }

                        false
                    };

                    if path.components().count() > 1 {
                        if let Some(parent) = path.parent() {
                            let _ = select_in_dir(parent);
                        }
                    } else if let Some(paths) = std::env::var_os("PATH") {
                        for dir in std::env::split_paths(&paths) {
                            if select_in_dir(&dir) {
                                break;
                            }
                        }
                    }
                }
            }

            if let Some(new_cmd) = replacement {
                argv[0] = new_cmd;
            }
        }

        if should_wrap {
            let mut command = String::from("& ");
            command.push_str(&powershell_quote(&argv[0]));
            for arg in argv.iter().skip(1) {
                command.push(' ');
                command.push_str(&powershell_quote(arg));
            }

            argv = vec![
                "powershell.exe".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                command,
            ];
        }
    }
    Ok(argv)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn test_build_command_wraps_cmd_on_windows() -> Result<(), Box<dyn std::error::Error>> {
        let argv = build_command_argv("claude.cmd --foo", None)?;
        assert_eq!(argv[0], "powershell.exe");
        assert_eq!(argv[1], "-NoProfile");
        assert_eq!(argv[2], "-Command");
        assert_eq!(argv[3], "& 'claude.cmd' '--foo'");
        Ok(())
    }

    #[test]
    fn test_build_command_keeps_exe_on_windows() -> Result<(), Box<dyn std::error::Error>> {
        let argv = build_command_argv("codex.exe --bar", None)?;
        assert_eq!(argv[0], "codex.exe");
        assert_eq!(argv[1], "--bar");
        Ok(())
    }
}

#[cfg(all(test, not(windows)))]
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
    fn test_build_command_argv_appends_prompt() -> Result<(), Box<dyn std::error::Error>> {
        let argv = build_command_argv("echo hello", Some("prompt"))?;
        assert_eq!(
            argv,
            vec![
                "echo".to_string(),
                "hello".to_string(),
                "prompt".to_string()
            ]
        );
        Ok(())
    }
}
