//! UI-related state: scroll positions, preview content, dimensions

/// UI-related state for the application
#[derive(Debug, Default)]
pub struct UiState {
    /// Scroll offset for the agent list (index of first visible agent)
    pub agent_list_scroll: usize,

    /// Scroll position in preview pane
    pub preview_scroll: usize,

    /// Scroll position in diff pane
    pub diff_scroll: usize,

    /// Scroll position in help overlay
    pub help_scroll: usize,

    /// Whether preview should auto-scroll to bottom on content updates
    /// Set to false when user manually scrolls up, true when they scroll to bottom
    pub preview_follow: bool,

    /// Cached preview content
    pub preview_content: String,

    /// Cached cursor position in the selected pane (x, y), 0-based.
    pub preview_cursor_position: Option<(u16, u16)>,

    /// Cached pane size for the selected pane (cols, rows).
    pub preview_pane_size: Option<(u16, u16)>,

    /// Cached diff content
    pub diff_content: String,

    /// Cached byte ranges for each diff line (matches `diff_content.lines()`)
    pub diff_line_ranges: Vec<(usize, usize)>,

    /// Cached preview pane dimensions (width, height) for mux window sizing
    pub preview_dimensions: Option<(u16, u16)>,

    /// Last error message (if any)
    pub last_error: Option<String>,

    /// Status message to display
    pub status_message: Option<String>,
}

impl UiState {
    /// Create a new UI state with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            agent_list_scroll: 0,
            preview_scroll: 0,
            diff_scroll: 0,
            help_scroll: 0,
            preview_follow: true,
            preview_content: String::new(),
            preview_cursor_position: None,
            preview_pane_size: None,
            diff_content: String::new(),
            diff_line_ranges: Vec::new(),
            preview_dimensions: None,
            last_error: None,
            status_message: None,
        }
    }

    /// Set diff content and refresh cached line ranges
    pub fn set_diff_content(&mut self, content: impl Into<String>) {
        let content = content.into();
        self.diff_line_ranges = compute_line_ranges(&content);
        self.diff_content = content;
        self.normalize_diff_scroll();
    }

    /// Reset scroll positions for both panes
    /// Preview is pinned to bottom (with follow enabled), Diff is pinned to top
    pub const fn reset_scroll(&mut self) {
        // Preview: set to max so render functions clamp to bottom of content
        self.preview_scroll = usize::MAX;
        self.preview_follow = true;
        // Diff: set to 0 to show from top
        self.diff_scroll = 0;
    }

    /// Scroll up in the preview pane by the given amount
    pub fn scroll_preview_up(&mut self, amount: usize) {
        self.normalize_preview_scroll();
        self.preview_scroll = self.preview_scroll.saturating_sub(amount);
        // Disable auto-follow when user scrolls up
        self.preview_follow = false;
    }

    /// Scroll down in the preview pane by the given amount
    pub fn scroll_preview_down(&mut self, amount: usize) {
        self.normalize_preview_scroll();
        self.preview_scroll = self.preview_scroll.saturating_add(amount);
        // Re-enable auto-follow if we've scrolled to the bottom
        self.check_preview_follow();
    }

    /// Scroll up in the diff pane by the given amount
    pub fn scroll_diff_up(&mut self, amount: usize) {
        self.normalize_diff_scroll();
        self.diff_scroll = self.diff_scroll.saturating_sub(amount);
    }

    /// Scroll down in the diff pane by the given amount
    pub fn scroll_diff_down(&mut self, amount: usize) {
        self.normalize_diff_scroll();
        self.diff_scroll = self.diff_scroll.saturating_add(amount);
    }

    /// Check if preview scroll is at bottom and re-enable follow mode if so
    fn check_preview_follow(&mut self) {
        let preview_lines = self.preview_content.lines().count();
        let visible_height = self.preview_dimensions.map_or(20, |(_, h)| usize::from(h));
        let preview_max = preview_lines.saturating_sub(visible_height);

        if self.preview_scroll >= preview_max {
            self.preview_follow = true;
        }
    }

    /// Normalize preview scroll position to be within valid range
    fn normalize_preview_scroll(&mut self) {
        let preview_lines = self.preview_content.lines().count();
        let visible_height = self.preview_dimensions.map_or(20, |(_, h)| usize::from(h));
        let preview_max = preview_lines.saturating_sub(visible_height);

        if self.preview_scroll > preview_max {
            self.preview_scroll = preview_max;
        }
    }

    /// Normalize diff scroll position to be within valid range
    fn normalize_diff_scroll(&mut self) {
        let diff_lines = self.diff_line_ranges.len();
        let visible_height = self.preview_dimensions.map_or(20, |(_, h)| usize::from(h));
        let diff_max = diff_lines.saturating_sub(visible_height);

        if self.diff_scroll > diff_max {
            self.diff_scroll = diff_max;
        }
    }

    /// Scroll preview to the top
    pub const fn preview_to_top(&mut self) {
        self.preview_scroll = 0;
        self.preview_follow = false;
    }

    /// Scroll diff to the top
    pub const fn diff_to_top(&mut self) {
        self.diff_scroll = 0;
    }

    /// Scroll preview to the bottom
    pub const fn preview_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        self.preview_scroll = content_lines.saturating_sub(visible_lines);
        self.preview_follow = true;
    }

    /// Scroll diff to the bottom
    pub const fn diff_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        self.diff_scroll = content_lines.saturating_sub(visible_lines);
    }

    /// Set the preview pane dimensions for mux window sizing
    pub const fn set_preview_dimensions(&mut self, width: u16, height: u16) {
        self.preview_dimensions = Some((width, height));
    }

    /// Set an error message
    pub fn set_error(&mut self, message: impl Into<String>) {
        let msg = message.into();
        tracing::warn!(error = %msg, "Application error");
        self.last_error = Some(msg);
    }

    /// Clear the current error message
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    /// Set a status message to display
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
    }

    /// Clear the current status message
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }
}

/// Compute per-line byte ranges for fast slicing.
/// Treats both `\n` and `\r\n` as line endings (like `str::lines()`).
fn compute_line_ranges(s: &str) -> Vec<(usize, usize)> {
    let bytes = s.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0usize;

    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            let mut end = i;
            if end > start && bytes[end - 1] == b'\r' {
                end = end.saturating_sub(1);
            }
            ranges.push((start, end));
            start = i + 1;
        }
    }

    if start < bytes.len() {
        let mut end = bytes.len();
        if end > start && bytes[end - 1] == b'\r' {
            end = end.saturating_sub(1);
        }
        ranges.push((start, end));
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_state_new() {
        let ui = UiState::new();
        assert_eq!(ui.agent_list_scroll, 0);
        assert_eq!(ui.preview_scroll, 0);
        assert_eq!(ui.diff_scroll, 0);
        assert_eq!(ui.help_scroll, 0);
        assert!(ui.preview_follow);
        assert!(ui.preview_content.is_empty());
        assert!(ui.diff_content.is_empty());
        assert!(ui.diff_line_ranges.is_empty());
        assert!(ui.preview_dimensions.is_none());
        assert!(ui.last_error.is_none());
        assert!(ui.status_message.is_none());
    }

    #[test]
    fn test_reset_scroll() {
        let mut ui = UiState::new();
        ui.preview_scroll = 100;
        ui.diff_scroll = 50;
        ui.preview_follow = false;
        ui.help_scroll = 25;

        ui.reset_scroll();

        assert_eq!(ui.preview_scroll, usize::MAX);
        assert!(ui.preview_follow);
        assert_eq!(ui.diff_scroll, 0);
        assert_eq!(ui.help_scroll, 25);
    }

    #[test]
    fn test_scroll_preview_up() {
        let mut ui = UiState::new();
        ui.preview_scroll = 10;
        ui.preview_content = "line1\nline2\nline3\nline4\nline5".to_string();
        ui.preview_dimensions = Some((80, 3));

        ui.scroll_preview_up(3);
        assert_eq!(ui.preview_scroll, 0);
        assert!(!ui.preview_follow);
    }

    #[test]
    fn test_scroll_preview_down() {
        let mut ui = UiState::new();
        ui.preview_scroll = 0;
        ui.preview_content = "line1\nline2\nline3\nline4\nline5".to_string();
        ui.preview_dimensions = Some((80, 3));

        ui.scroll_preview_down(2);
        assert_eq!(ui.preview_scroll, 2);
        assert!(ui.preview_follow); // At max scroll, follow is re-enabled
    }

    #[test]
    fn test_scroll_diff_up() {
        let mut ui = UiState::new();
        ui.diff_scroll = 10;
        ui.preview_dimensions = Some((80, 3));
        ui.set_diff_content("line1\nline2\nline3\nline4\nline5");

        ui.scroll_diff_up(3);
        assert_eq!(ui.diff_scroll, 0);
    }

    #[test]
    fn test_scroll_diff_down() {
        let mut ui = UiState::new();
        ui.diff_scroll = 0;
        ui.preview_dimensions = Some((80, 3));
        ui.set_diff_content("line1\nline2\nline3\nline4\nline5");

        ui.scroll_diff_down(5);
        // With 5 lines and height 3, max scroll is 2, but normalization happens on next scroll
        assert_eq!(ui.diff_scroll, 5);
    }

    #[test]
    fn test_preview_to_top() {
        let mut ui = UiState::new();
        ui.preview_scroll = 100;
        ui.preview_follow = true;

        ui.preview_to_top();

        assert_eq!(ui.preview_scroll, 0);
        assert!(!ui.preview_follow);
    }

    #[test]
    fn test_diff_to_top() {
        let mut ui = UiState::new();
        ui.diff_scroll = 100;

        ui.diff_to_top();

        assert_eq!(ui.diff_scroll, 0);
    }

    #[test]
    fn test_preview_to_bottom() {
        let mut ui = UiState::new();

        ui.preview_to_bottom(100, 20);

        assert_eq!(ui.preview_scroll, 80);
        assert!(ui.preview_follow);
    }

    #[test]
    fn test_diff_to_bottom() {
        let mut ui = UiState::new();

        ui.diff_to_bottom(100, 20);

        assert_eq!(ui.diff_scroll, 80);
    }

    #[test]
    fn test_set_preview_dimensions() {
        let mut ui = UiState::new();

        ui.set_preview_dimensions(80, 24);

        assert_eq!(ui.preview_dimensions, Some((80, 24)));
    }

    #[test]
    fn test_set_and_clear_error() {
        let mut ui = UiState::new();

        ui.set_error("Test error");
        assert_eq!(ui.last_error, Some("Test error".to_string()));

        ui.clear_error();
        assert!(ui.last_error.is_none());
    }

    #[test]
    fn test_set_and_clear_status() {
        let mut ui = UiState::new();

        ui.set_status("Test status");
        assert_eq!(ui.status_message, Some("Test status".to_string()));

        ui.clear_status();
        assert!(ui.status_message.is_none());
    }

    #[test]
    fn test_compute_line_ranges_empty() {
        let ranges = compute_line_ranges("");
        assert!(ranges.is_empty());
    }

    #[test]
    fn test_compute_line_ranges_single_line() {
        let s = "hello";
        let ranges = compute_line_ranges(s);
        assert_eq!(ranges, vec![(0, 5)]);
        assert_eq!(&s[ranges[0].0..ranges[0].1], "hello");
    }

    #[test]
    fn test_compute_line_ranges_multiple_lines() {
        let s = "line1\nline2\nline3";
        let ranges = compute_line_ranges(s);
        assert_eq!(ranges.len(), 3);
        assert_eq!(&s[ranges[0].0..ranges[0].1], "line1");
        assert_eq!(&s[ranges[1].0..ranges[1].1], "line2");
        assert_eq!(&s[ranges[2].0..ranges[2].1], "line3");
    }

    #[test]
    fn test_compute_line_ranges_crlf() {
        let s = "line1\r\nline2\r\nline3";
        let ranges = compute_line_ranges(s);
        assert_eq!(ranges.len(), 3);
        assert_eq!(&s[ranges[0].0..ranges[0].1], "line1");
        assert_eq!(&s[ranges[1].0..ranges[1].1], "line2");
        assert_eq!(&s[ranges[2].0..ranges[2].1], "line3");
    }

    #[test]
    fn test_compute_line_ranges_trailing_newline() {
        let s = "line1\nline2\n";
        let ranges = compute_line_ranges(s);
        // Trailing newline creates an empty implicit line only if there's content after it
        // Since there's no content after the final \n, we get 2 lines (matches str::lines())
        assert_eq!(ranges.len(), 2);
        assert_eq!(&s[ranges[0].0..ranges[0].1], "line1");
        assert_eq!(&s[ranges[1].0..ranges[1].1], "line2");
    }

    #[test]
    fn test_set_diff_content_updates_line_ranges() {
        let mut ui = UiState::new();
        ui.set_diff_content("line1\nline2\nline3");
        assert_eq!(ui.diff_line_ranges.len(), 3);
        assert_eq!(
            &ui.diff_content[ui.diff_line_ranges[0].0..ui.diff_line_ranges[0].1],
            "line1"
        );
    }
}
