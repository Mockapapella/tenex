//! Mux output capture (client-side).

use super::protocol::{CaptureKind, MuxRequest, MuxResponse};
use anyhow::{Result, bail};

/// Capture output from mux sessions.
#[derive(Debug, Clone, Copy, Default)]
pub struct Capture;

impl Capture {
    /// Create a new output capture instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Capture the visible pane content with ANSI color codes.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn capture_pane(&self, target: &str) -> Result<String> {
        self.capture(target, CaptureKind::Visible)
    }

    /// Capture pane with scroll-back history and ANSI color codes.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn capture_pane_with_history(&self, target: &str, lines: u32) -> Result<String> {
        self.capture(target, CaptureKind::History { lines })
    }

    /// Capture entire scroll-back buffer with ANSI color codes.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn capture_full_history(&self, target: &str) -> Result<String> {
        self.capture(target, CaptureKind::FullHistory)
    }

    /// Get the current pane size.
    ///
    /// # Errors
    ///
    /// Returns an error if the size cannot be retrieved.
    pub fn pane_size(&self, target: &str) -> Result<(u16, u16)> {
        match super::client::request(&MuxRequest::PaneSize {
            target: target.to_string(),
        })? {
            MuxResponse::Size { cols, rows } => Ok((cols, rows)),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Get the cursor position in the pane.
    ///
    /// # Errors
    ///
    /// Returns an error if the position cannot be retrieved.
    pub fn cursor_position(&self, target: &str) -> Result<(u16, u16)> {
        match super::client::request(&MuxRequest::CursorPosition {
            target: target.to_string(),
        })? {
            MuxResponse::Position { x, y } => Ok((x, y)),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Return the current command for a pane.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be retrieved.
    pub fn pane_current_command(&self, target: &str) -> Result<String> {
        match super::client::request(&MuxRequest::PaneCurrentCommand {
            target: target.to_string(),
        })? {
            MuxResponse::Text { text } => Ok(text),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    /// Get the last N non-empty lines from the pane.
    ///
    /// # Errors
    ///
    /// Returns an error if capture fails.
    pub fn tail(&self, target: &str, lines: usize) -> Result<Vec<String>> {
        let lines_u32 = u32::try_from(lines).map_or(u32::MAX, |value| value);

        match super::client::request(&MuxRequest::Tail {
            target: target.to_string(),
            lines: lines_u32,
        })? {
            MuxResponse::Text { text } => Ok(text.lines().map(String::from).collect()),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }

    fn capture(self, target: &str, kind: CaptureKind) -> Result<String> {
        let _ = self;
        match super::client::request(&MuxRequest::Capture {
            target: target.to_string(),
            kind,
        })? {
            MuxResponse::Text { text } => Ok(text),
            MuxResponse::Err { message } => bail!("{message}"),
            other => bail!("Unexpected response: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_capture_new() {
        let capture = Capture::new();
        assert!(!format!("{capture:?}").is_empty());
    }
}
