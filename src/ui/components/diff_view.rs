//! Diff view widget

use ratatui::{
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Widget for displaying git diffs
pub struct Widget {
    content: String,
    scroll: usize,
    title: String,
}

impl Widget {
    /// Create a new diff view widget
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            scroll: 0,
            title: " Git Diff ".to_string(),
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

    /// Convert to a Paragraph widget with syntax highlighting
    #[must_use]
    pub fn to_paragraph(&self) -> Paragraph<'_> {
        let lines: Vec<Line> = self
            .content
            .lines()
            .map(|line| {
                let color = line_color(line);
                Line::styled(line, Style::default().fg(color))
            })
            .collect();

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

/// Determine the color for a diff line
#[must_use]
pub fn line_color(line: &str) -> Color {
    if line.starts_with('+') && !line.starts_with("+++") {
        Color::Green
    } else if line.starts_with('-') && !line.starts_with("---") {
        Color::Red
    } else if line.starts_with("@@") {
        Color::Cyan
    } else if line.starts_with("diff ") || line.starts_with("index ") {
        Color::Yellow
    } else if line.starts_with("+++") || line.starts_with("---") {
        Color::Blue
    } else {
        Color::White
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_view_widget_new() {
        let widget = Widget::new("+added\n-removed");
        assert_eq!(widget.line_count(), 2);
    }

    #[test]
    fn test_diff_view_widget_scroll() {
        let widget = Widget::new("test").scroll(5);
        assert_eq!(widget.scroll, 5);
    }

    #[test]
    fn test_diff_view_widget_title() {
        let widget = Widget::new("test").title("Changes");
        assert_eq!(widget.title, "Changes");
    }

    #[test]
    fn test_line_color_added() {
        assert_eq!(line_color("+new line"), Color::Green);
    }

    #[test]
    fn test_line_color_removed() {
        assert_eq!(line_color("-old line"), Color::Red);
    }

    #[test]
    fn test_line_color_hunk_header() {
        assert_eq!(line_color("@@ -1,3 +1,4 @@"), Color::Cyan);
    }

    #[test]
    fn test_line_color_diff_header() {
        assert_eq!(line_color("diff --git a/file b/file"), Color::Yellow);
    }

    #[test]
    fn test_line_color_file_markers() {
        assert_eq!(line_color("--- a/file"), Color::Blue);
        assert_eq!(line_color("+++ b/file"), Color::Blue);
    }

    #[test]
    fn test_line_color_context() {
        assert_eq!(line_color(" context line"), Color::White);
        assert_eq!(line_color("regular text"), Color::White);
    }

    #[test]
    fn test_to_paragraph() {
        let widget = Widget::new("+added\n-removed\n context");
        let _paragraph = widget.to_paragraph();
    }
}
