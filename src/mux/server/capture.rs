//! PTY-backed output capture (server-side).

use anyhow::Result;

/// Capture output from mux sessions.
#[derive(Debug, Clone, Copy, Default)]
pub struct Capture;

impl Capture {
    /// Capture the visible pane content with ANSI color codes.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn capture_pane(session: &str) -> Result<String> {
        let window = super::super::backend::resolve_window(session)?;
        let result = {
            let guard = window.lock();
            super::super::render::render_screen_rows(guard.parser.screen())?.join("\n")
        };
        Ok(result)
    }

    /// Capture pane with scroll-back history and ANSI color codes.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn capture_pane_with_history(session: &str, lines: u32) -> Result<String> {
        let window = super::super::backend::resolve_window(session)?;
        let lines = usize::try_from(lines).map_or(usize::MAX, |value| value);
        let result = {
            let mut guard = window.lock();
            super::super::render::capture_lines(&mut guard.parser, lines)?
        };
        Ok(result)
    }

    /// Capture entire scroll-back buffer with ANSI color codes.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn capture_full_history(session: &str) -> Result<String> {
        let window = super::super::backend::resolve_window(session)?;
        let result = {
            let mut guard = window.lock();
            super::super::render::capture_lines(&mut guard.parser, usize::MAX)?
        };
        Ok(result)
    }

    /// Get the current pane size.
    ///
    /// # Errors
    ///
    /// Returns an error if the size cannot be retrieved.
    pub fn pane_size(session: &str) -> Result<(u16, u16)> {
        let window = super::super::backend::resolve_window(session)?;
        let (rows, cols) = {
            let guard = window.lock();
            guard.parser.screen().size()
        };
        Ok((cols, rows))
    }

    /// Get the cursor position in the pane.
    ///
    /// # Errors
    ///
    /// Returns an error if the position cannot be retrieved.
    pub fn cursor_position(session: &str) -> Result<(u16, u16, bool)> {
        let window = super::super::backend::resolve_window(session)?;
        let (row, col, hidden) = {
            let guard = window.lock();
            let (row, col) = guard.parser.screen().cursor_position();
            let hidden = guard.parser.screen().hide_cursor();
            drop(guard);
            (row, col, hidden)
        };
        Ok((col, row, hidden))
    }

    /// Check if the pane is running a program.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be retrieved.
    pub fn pane_current_command(session: &str) -> Result<String> {
        let window = super::super::backend::resolve_window(session)?;
        let command = {
            let guard = window.lock();
            guard.command.first().cloned()
        };
        Ok(command.unwrap_or_default())
    }

    /// Get the last N lines from the pane.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn tail(session: &str, lines: usize) -> Result<Vec<String>> {
        let lines_u32 = u32::try_from(lines).map_or(u32::MAX, |value| value);
        let content = Self::capture_pane_with_history(session, lines_u32)?;

        let mut result: Vec<String> = content
            .lines()
            .map(String::from)
            .rev()
            .filter(|line| has_visible_text(line))
            .take(lines)
            .collect();

        result.reverse();
        Ok(result)
    }
}

fn has_visible_text(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i = skip_escape_sequence(bytes, i);
            continue;
        }
        if bytes[i].is_ascii_whitespace() {
            i = i.saturating_add(1);
            continue;
        }
        return true;
    }
    false
}

fn skip_escape_sequence(bytes: &[u8], start: usize) -> usize {
    let mut i = start.saturating_add(1);
    if i >= bytes.len() {
        return i;
    }

    if bytes[i] != b'[' && bytes[i] != b']' {
        return i.saturating_add(1);
    }

    i = i.saturating_add(1);
    while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
        i = i.saturating_add(1);
    }
    i.saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::super::SessionManager;
    use super::*;

    fn test_command() -> Vec<String> {
        #[cfg(windows)]
        {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 2".to_string(),
            ]
        }
        #[cfg(not(windows))]
        {
            vec!["sh".to_string(), "-c".to_string(), "sleep 2".to_string()]
        }
    }

    #[test]
    fn test_output_capture_new() {
        let capture = Capture;
        assert!(!format!("{capture:?}").is_empty());
    }

    #[test]
    fn test_capture_with_real_session() {
        let session_name = "tenex-test-capture";
        let tmp = std::env::temp_dir();

        let _ = SessionManager::kill(session_name);

        let command = test_command();
        let result = SessionManager::create(session_name, &tmp, Some(&command));
        assert!(result.is_ok());

        let _ = Capture::capture_pane(session_name);
        let _ = Capture::capture_pane_with_history(session_name, 10);
        let _ = Capture::capture_full_history(session_name);
        let _ = Capture::pane_size(session_name);
        let _ = Capture::cursor_position(session_name);
        let _ = Capture::pane_current_command(session_name);

        let _ = SessionManager::kill(session_name);
    }

    #[test]
    fn test_visible_text_detection() {
        assert!(!has_visible_text("   \t"));
        assert!(!has_visible_text("\u{1b}[0m   \u{1b}[0m"));
        assert!(has_visible_text(" x "));
    }
}
