//! Status bar widget

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Widget for displaying the status bar
#[derive(Debug)]
pub struct Widget {
    content: StatusContent,
}

/// Content type for the status bar
#[derive(Debug)]
pub enum StatusContent {
    /// Normal status showing running count and keybindings
    Normal { running_count: usize },
    /// Error message
    Error(String),
    /// Status message
    Status(String),
}

impl Widget {
    /// Create a new status bar with normal content
    #[must_use]
    pub const fn normal(running_count: usize) -> Self {
        Self {
            content: StatusContent::Normal { running_count },
        }
    }

    /// Create a new status bar with an error message
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: StatusContent::Error(message.into()),
        }
    }

    /// Create a new status bar with a status message
    #[must_use]
    pub fn status(message: impl Into<String>) -> Self {
        Self {
            content: StatusContent::Status(message.into()),
        }
    }

    /// Convert to a Paragraph widget
    #[must_use]
    pub fn to_paragraph(&self) -> Paragraph<'_> {
        let span = match &self.content {
            StatusContent::Error(msg) => Span::styled(
                format!(" Error: {msg} "),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            StatusContent::Status(msg) => {
                Span::styled(format!(" {msg} "), Style::default().fg(Color::Green))
            }
            StatusContent::Normal { running_count } => Span::styled(
                format!(" {running_count} running | [n]ew [d]el [Tab]switch [?]help [q]uit "),
                Style::default().fg(Color::Gray),
            ),
        };

        Paragraph::new(Line::from(span)).style(Style::default().bg(Color::DarkGray))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_bar_normal() {
        let widget = Widget::normal(3);
        match widget.content {
            StatusContent::Normal { running_count } => assert_eq!(running_count, 3),
            _ => panic!("Expected Normal content"),
        }
    }

    #[test]
    fn test_status_bar_error() {
        let widget = Widget::error("Something went wrong");
        match widget.content {
            StatusContent::Error(msg) => assert_eq!(msg, "Something went wrong"),
            _ => panic!("Expected Error content"),
        }
    }

    #[test]
    fn test_status_bar_status() {
        let widget = Widget::status("Agent created");
        match widget.content {
            StatusContent::Status(msg) => assert_eq!(msg, "Agent created"),
            _ => panic!("Expected Status content"),
        }
    }

    #[test]
    fn test_to_paragraph() {
        let widget = Widget::normal(0);
        let _paragraph = widget.to_paragraph();
    }

    #[test]
    fn test_to_paragraph_error() {
        let widget = Widget::error("test error");
        let _paragraph = widget.to_paragraph();
    }

    #[test]
    fn test_to_paragraph_status() {
        let widget = Widget::status("test status");
        let _paragraph = widget.to_paragraph();
    }
}
