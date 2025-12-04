//! Preview pane widget

use ratatui::{
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Widget for displaying terminal preview
pub struct Widget {
    content: String,
    scroll: usize,
    title: String,
}

impl Widget {
    /// Create a new preview widget
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            scroll: 0,
            title: " Terminal Output ".to_string(),
        }
    }

    /// Set the scroll position
    #[must_use]
    pub const fn scroll(mut self, scroll: usize) -> Self {
        self.scroll = scroll;
        self
    }

    /// Set a custom title
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Convert to a Paragraph widget
    #[must_use]
    pub fn to_paragraph(&self) -> Paragraph<'_> {
        let lines: Vec<Line> = self.content.lines().map(Line::from).collect();

        let scroll_pos = u16::try_from(self.scroll).unwrap_or(u16::MAX);
        Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(self.title.clone())
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::White)),
            )
            .scroll((scroll_pos, 0))
            .wrap(Wrap { trim: false })
    }

    /// Get the number of lines in the content
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.content.lines().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_widget_new() {
        let widget = Widget::new("Hello\nWorld");
        assert_eq!(widget.line_count(), 2);
    }

    #[test]
    fn test_preview_widget_scroll() {
        let widget = Widget::new("test").scroll(10);
        assert_eq!(widget.scroll, 10);
    }

    #[test]
    fn test_preview_widget_title() {
        let widget = Widget::new("test").title("Custom");
        assert_eq!(widget.title, "Custom");
    }

    #[test]
    fn test_to_paragraph() {
        let widget = Widget::new("test content");
        let _paragraph = widget.to_paragraph();
    }

    #[test]
    fn test_line_count_empty() {
        let widget = Widget::new("");
        assert_eq!(widget.line_count(), 0);
    }

    #[test]
    fn test_line_count_multiline() {
        let widget = Widget::new("a\nb\nc\nd\ne");
        assert_eq!(widget.line_count(), 5);
    }
}
