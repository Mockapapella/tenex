//! PTY-backed output capture (server-side).

use anyhow::{Result, bail};
use vt100::Color;

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
            render_screen_rows(guard.parser.screen())?.join("\n")
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
            capture_lines(&mut guard.parser, lines)?
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
            capture_lines(&mut guard.parser, usize::MAX)?
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

fn capture_lines(
    parser: &mut vt100::Parser,
    requested: usize,
) -> Result<String> {
    let (rows, _cols) = parser.screen().size();
    let height = usize::from(rows);

    if height == 0 {
        bail!("Invalid screen size");
    }

    let original_offset = parser.screen().scrollback();
    let result = (|| -> Result<String> {
        parser.screen_mut().set_scrollback(usize::MAX);
        let scrollback_len = parser.screen().scrollback();
        let total_lines = scrollback_len.saturating_add(height);

        let requested = if requested == usize::MAX {
            total_lines
        } else {
            requested.min(total_lines)
        };

        let start_line = total_lines.saturating_sub(requested);

        let mut collected: Vec<String> = Vec::with_capacity(requested);

        if start_line >= scrollback_len {
            parser.screen_mut().set_scrollback(0);
            let rows = render_screen_rows(parser.screen())?;
            let screen_start = start_line.saturating_sub(scrollback_len).min(height);
            collected.extend(rows.into_iter().skip(screen_start));
            return Ok(collected.join("\n"));
        }

        let scrollback_start = start_line;
        let first_page_start = scrollback_start.saturating_sub(scrollback_start % height);
        let mut page_start = first_page_start;
        let mut skip_within_page = scrollback_start.saturating_sub(first_page_start);

        while page_start < scrollback_len {
            let offset = scrollback_len.saturating_sub(page_start);
            parser.screen_mut().set_scrollback(offset);
            let rows = render_screen_rows(parser.screen())?;
            let available = scrollback_len.saturating_sub(page_start).min(height);
            let skip = skip_within_page.min(available);

            collected.extend(rows.into_iter().take(available).skip(skip));

            page_start = page_start.saturating_add(height);
            skip_within_page = 0;
        }

        parser.screen_mut().set_scrollback(0);
        let rows = render_screen_rows(parser.screen())?;
        collected.extend(rows);

        Ok(collected.join("\n"))
    })();

    parser.screen_mut().set_scrollback(original_offset);
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    pub fg: Color,
    pub bg: Color,
    pub modifiers: u8,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            modifiers: 0,
        }
    }
}

impl CellStyle {
    fn from_cell(cell: &vt100::Cell) -> Self {
        let mut modifiers = 0;
        if cell.bold() {
            modifiers |= MOD_BOLD;
        }
        if cell.italic() {
            modifiers |= MOD_ITALIC;
        }
        if cell.underline() {
            modifiers |= MOD_UNDERLINE;
        }
        if cell.inverse() {
            modifiers |= MOD_INVERSE;
        }

        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            modifiers,
        }
    }
}

fn render_screen_rows(screen: &vt100::Screen) -> Result<Vec<String>> {
    let (rows, cols) = screen.size();
    let height = usize::from(rows);
    let mut lines = Vec::with_capacity(height);

    if rows == 0 {
        bail!("Invalid screen size");
    }

    for row in 0..rows {
        lines.push(render_row(screen, row, cols));
    }

    Ok(lines)
}

fn render_row(screen: &vt100::Screen, row: u16, cols: u16) -> String {
    let mut out = String::new();
    let mut current_style: Option<CellStyle> = None;

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            if current_style != Some(CellStyle::default()) {
                write_sgr(&mut out, CellStyle::default());
                current_style = Some(CellStyle::default());
            }
            out.push(' ');
            continue;
        };

        if cell.is_wide_continuation() {
            continue;
        }

        let style = CellStyle::from_cell(cell);
        if current_style != Some(style) {
            write_sgr(&mut out, style);
            current_style = Some(style);
        }

        if cell.has_contents() {
            out.push_str(&cell.contents());
        } else {
            out.push(' ');
        }
    }

    // Reset attributes at the end of each row to avoid style bleeding.
    out.push_str("\x1b[0m");
    out
}

fn write_sgr(out: &mut String, style: CellStyle) {
    out.push('\x1b');
    out.push('[');
    out.push('0');

    if style.modifiers & MOD_BOLD != 0 {
        out.push_str(";1");
    }
    if style.modifiers & MOD_ITALIC != 0 {
        out.push_str(";3");
    }
    if style.modifiers & MOD_UNDERLINE != 0 {
        out.push_str(";4");
    }
    if style.modifiers & MOD_INVERSE != 0 {
        out.push_str(";7");
    }

    write_color(out, style.fg, true);
    write_color(out, style.bg, false);

    out.push('m');
}

const MOD_BOLD: u8 = 0b0000_0001;
const MOD_ITALIC: u8 = 0b0000_0010;
const MOD_UNDERLINE: u8 = 0b0000_0100;
const MOD_INVERSE: u8 = 0b0000_1000;

fn write_color(out: &mut String, color: Color, foreground: bool) {
    match color {
        Color::Default => {}
        Color::Idx(index) => {
            if foreground {
                out.push_str(";38;5;");
            } else {
                out.push_str(";48;5;");
            }
            push_u8_decimal(out, index);
        }
        Color::Rgb(r, g, b) => {
            if foreground {
                out.push_str(";38;2;");
            } else {
                out.push_str(";48;2;");
            }
            push_u8_decimal(out, r);
            out.push(';');
            push_u8_decimal(out, g);
            out.push(';');
            push_u8_decimal(out, b);
        }
    }
}

fn push_u8_decimal(out: &mut String, value: u8) {
    if value >= 100 {
        out.push((b'0' + (value / 100)) as char);
        let remainder = value % 100;
        out.push((b'0' + (remainder / 10)) as char);
        out.push((b'0' + (remainder % 10)) as char);
        return;
    }

    if value >= 10 {
        out.push((b'0' + (value / 10)) as char);
        out.push((b'0' + (value % 10)) as char);
        return;
    }

    out.push((b'0' + value) as char);
}

#[cfg(test)]
mod tests {
    use super::super::SessionManager;
    use super::*;

    fn test_command() -> Vec<String> {
        #[cfg(windows)]
        {
            return vec![
                "powershell.exe".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 2".to_string(),
            ];
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

    #[test]
    fn test_push_u8_decimal_formats_values() {
        let mut out = String::new();
        push_u8_decimal(&mut out, 7);
        assert_eq!(out, "7");

        out.clear();
        push_u8_decimal(&mut out, 42);
        assert_eq!(out, "42");

        out.clear();
        push_u8_decimal(&mut out, 123);
        assert_eq!(out, "123");
    }

    #[test]
    fn test_render_screen_rows_basic() -> Result<()> {
        let mut parser = vt100::Parser::new(2, 3, 0);
        parser.process(b"\x1b[31mA\x1b[0m");
        let lines = render_screen_rows(parser.screen())?;
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains('A'));
        assert!(lines[0].ends_with("\x1b[0m"));
        Ok(())
    }

    #[test]
    fn test_capture_lines_includes_tail_lines() -> Result<()> {
        let mut parser = vt100::Parser::new(5, 20, 1_000);

        let mut output = String::new();
        for i in 0..29 {
            output.push_str(&format!("L{i}\r\n"));
        }
        output.push_str("L29");
        parser.process(output.as_bytes());

        let captured = capture_lines(&mut parser, usize::MAX)?;
        let lines: Vec<&str> = captured.lines().collect();
        if lines.len() < 10 {
            bail!("Expected at least 10 lines, got {}", lines.len());
        }

        let tail_start = lines.len().saturating_sub(4);
        let Some(tail) = lines.get(tail_start..) else {
            bail!("Failed to slice captured tail");
        };

        assert!(
            lines.first().is_some_and(|line| line.contains("L0")),
            "Expected captured output to start with L0"
        );

        assert!(
            lines.last().is_some_and(|line| line.contains("L29")),
            "Expected captured output to end with L29"
        );

        assert!(tail[0].contains("L26"));
        assert!(tail[1].contains("L27"));
        assert!(tail[2].contains("L28"));
        assert!(tail[3].contains("L29"));
        Ok(())
    }
}
