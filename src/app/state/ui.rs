//! UI-related state: scroll positions, preview content, dimensions

use uuid::Uuid;

use std::path::PathBuf;

/// UI-related state for the application
#[derive(Debug, Default)]
pub struct UiState {
    /// Scroll offset for the agent list (index of first visible agent)
    pub agent_list_scroll: usize,

    /// Scroll position in preview pane
    pub preview_scroll: usize,

    /// Scroll position in diff pane
    pub diff_scroll: usize,

    /// Cursor position (selected line index) in diff pane
    pub diff_cursor: usize,

    /// Scroll position in help overlay
    pub help_scroll: usize,

    /// Whether preview should auto-scroll to bottom on content updates
    /// Set to false when user manually scrolls up, true when they scroll to bottom
    pub preview_follow: bool,

    /// Cached preview content
    pub preview_content: String,

    /// Cached cursor position in the selected pane (x, y), 0-based, and whether it is hidden.
    pub preview_cursor_position: Option<(u16, u16, bool)>,

    /// Cached pane size for the selected pane (cols, rows).
    pub preview_pane_size: Option<(u16, u16)>,

    /// Cached diff content
    pub diff_content: String,

    /// Cached byte ranges for each diff line (matches `diff_content.lines()`)
    pub diff_line_ranges: Vec<(usize, usize)>,

    /// Cached metadata for each diff line (matches `diff_content.lines()`)
    pub diff_line_meta: Vec<DiffLineMeta>,

    /// Current structured diff model for interactive operations
    pub diff_model: Option<crate::git::DiffModel>,

    /// Folded file paths in the diff view
    pub diff_folded_files: Vec<PathBuf>,

    /// Folded hunks in the diff view
    pub diff_folded_hunks: Vec<DiffHunkKey>,

    /// Undo stack for diff edits
    pub diff_undo: Vec<DiffEdit>,

    /// Redo stack for diff edits
    pub diff_redo: Vec<DiffEdit>,

    /// Current diff hash (0 when no changes)
    pub diff_hash: u64,

    /// Diff hash the user last saw in the diff tab per agent (0 if never viewed)
    pub diff_last_seen_hash_by_agent: Vec<(Uuid, u64)>,

    /// Whether the diff has unseen changes since last view
    pub diff_has_unseen_changes: bool,

    /// Request an immediate diff refresh after an edit action
    pub diff_force_refresh: bool,

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
            diff_cursor: 0,
            help_scroll: 0,
            preview_follow: true,
            preview_content: String::new(),
            preview_cursor_position: None,
            preview_pane_size: None,
            diff_content: String::new(),
            diff_line_ranges: Vec::new(),
            diff_line_meta: Vec::new(),
            diff_model: None,
            diff_folded_files: Vec::new(),
            diff_folded_hunks: Vec::new(),
            diff_undo: Vec::new(),
            diff_redo: Vec::new(),
            diff_hash: 0,
            diff_last_seen_hash_by_agent: Vec::new(),
            diff_has_unseen_changes: false,
            diff_force_refresh: false,
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
        self.diff_line_meta = vec![DiffLineMeta::Unknown; self.diff_line_ranges.len()];
        self.normalize_diff_scroll();
        self.normalize_diff_cursor();
    }

    /// Set diff content, line metadata, and refresh cached line ranges.
    pub fn set_diff_view(&mut self, content: impl Into<String>, meta: Vec<DiffLineMeta>) {
        let content = content.into();
        self.diff_line_ranges = compute_line_ranges(&content);
        self.diff_content = content;
        self.diff_line_meta = if meta.len() == self.diff_line_ranges.len() {
            meta
        } else {
            tracing::warn!(
                meta_len = meta.len(),
                ranges_len = self.diff_line_ranges.len(),
                "diff line metadata length mismatch; falling back to Unknown"
            );
            vec![DiffLineMeta::Unknown; self.diff_line_ranges.len()]
        };
        self.normalize_diff_scroll();
        self.normalize_diff_cursor();
    }

    /// Reset scroll positions for both panes
    /// Preview is pinned to bottom (with follow enabled), Diff is pinned to top
    pub const fn reset_scroll(&mut self) {
        // Preview: set to max so render functions clamp to bottom of content
        self.preview_scroll = usize::MAX;
        self.preview_follow = true;
        // Diff: set to 0 to show from top
        self.diff_scroll = 0;
        self.diff_cursor = 0;
    }

    /// Reset interactive diff state when switching agents/worktrees.
    pub fn reset_diff_interaction(&mut self) {
        self.diff_cursor = 0;
        self.diff_model = None;
        self.diff_folded_files.clear();
        self.diff_folded_hunks.clear();
        self.diff_undo.clear();
        self.diff_redo.clear();
        self.diff_hash = 0;
        self.diff_has_unseen_changes = false;
        self.diff_force_refresh = false;
        self.diff_line_meta.clear();
    }

    #[must_use]
    pub fn diff_last_seen_hash_for_agent(&self, agent_id: Uuid) -> u64 {
        self.diff_last_seen_hash_by_agent
            .iter()
            .find(|(id, _)| *id == agent_id)
            .map_or(0, |(_, hash)| *hash)
    }

    pub fn set_diff_last_seen_hash_for_agent(&mut self, agent_id: Uuid, hash: u64) {
        if let Some((_, existing)) = self
            .diff_last_seen_hash_by_agent
            .iter_mut()
            .find(|(id, _)| *id == agent_id)
        {
            *existing = hash;
            return;
        }

        self.diff_last_seen_hash_by_agent.push((agent_id, hash));
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
        self.diff_cursor = self.diff_cursor.saturating_sub(amount);
        self.normalize_diff_cursor();
    }

    /// Scroll down in the diff pane by the given amount
    pub fn scroll_diff_down(&mut self, amount: usize) {
        self.normalize_diff_scroll();
        self.diff_scroll = self.diff_scroll.saturating_add(amount);
        self.diff_cursor = self.diff_cursor.saturating_add(amount);
        self.normalize_diff_cursor();
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

    fn normalize_diff_cursor(&mut self) {
        let diff_lines = self.diff_line_ranges.len();
        if diff_lines == 0 {
            self.diff_cursor = 0;
            return;
        }

        let max = diff_lines.saturating_sub(1);
        if self.diff_cursor > max {
            self.diff_cursor = max;
        }

        self.ensure_diff_cursor_visible();
    }

    fn ensure_diff_cursor_visible(&mut self) {
        let visible_height = self.preview_dimensions.map_or(20, |(_, h)| usize::from(h));
        if visible_height == 0 {
            return;
        }

        if self.diff_cursor < self.diff_scroll {
            self.diff_scroll = self.diff_cursor;
        } else if self.diff_cursor >= self.diff_scroll.saturating_add(visible_height) {
            self.diff_scroll = self
                .diff_cursor
                .saturating_sub(visible_height.saturating_sub(1));
        }

        self.normalize_diff_scroll();
    }

    /// Scroll preview to the top
    pub const fn preview_to_top(&mut self) {
        self.preview_scroll = 0;
        self.preview_follow = false;
    }

    /// Scroll diff to the top
    pub const fn diff_to_top(&mut self) {
        self.diff_scroll = 0;
        self.diff_cursor = 0;
    }

    /// Scroll preview to the bottom
    pub const fn preview_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        self.preview_scroll = content_lines.saturating_sub(visible_lines);
        self.preview_follow = true;
    }

    /// Scroll diff to the bottom
    pub const fn diff_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        self.diff_scroll = content_lines.saturating_sub(visible_lines);
        self.diff_cursor = content_lines.saturating_sub(1);
    }

    /// Move the diff cursor up by the given amount.
    pub fn diff_cursor_up(&mut self, amount: usize) {
        self.diff_cursor = self.diff_cursor.saturating_sub(amount);
        self.normalize_diff_cursor();
    }

    /// Move the diff cursor down by the given amount.
    pub fn diff_cursor_down(&mut self, amount: usize) {
        self.diff_cursor = self.diff_cursor.saturating_add(amount);
        self.normalize_diff_cursor();
    }

    /// Build the diff view content and metadata from a structured diff model.
    #[must_use]
    pub fn build_diff_view(&self, model: &crate::git::DiffModel) -> (String, Vec<DiffLineMeta>) {
        let mut lines: Vec<String> = Vec::new();
        let mut meta: Vec<DiffLineMeta> = Vec::new();

        let undo_len = self.diff_undo.len();
        let redo_len = self.diff_redo.len();
        lines.push(format!(
            "{} | edits: {undo_len} undo / {redo_len} redo",
            model.summary
        ));
        meta.push(DiffLineMeta::Info);

        lines.push(
            "Enter: focus diff  (focused: Esc/Ctrl+q: exit | ↑/↓: move | x: delete line/hunk | Ctrl+z: undo | Ctrl+y: redo | Space: fold)"
                .to_string(),
        );
        meta.push(DiffLineMeta::Info);

        if model.files.is_empty() {
            lines.push("(No changes)".to_string());
            meta.push(DiffLineMeta::Info);
        }

        for (file_idx, file) in model.files.iter().enumerate() {
            let is_file_folded = self.diff_folded_files.iter().any(|p| p == &file.path);
            let file_indicator = if is_file_folded { "▶" } else { "▼" };
            lines.push(format!(
                "{file_indicator} [{}] {} (+{} -{})",
                file.status,
                file.path.display(),
                file.additions,
                file.deletions
            ));
            meta.push(DiffLineMeta::File { file_idx });

            if is_file_folded {
                continue;
            }

            for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
                let key = DiffHunkKey {
                    file_path: file.path.clone(),
                    old_start: hunk.old_start,
                    new_start: hunk.new_start,
                };
                let is_hunk_folded = self.diff_folded_hunks.iter().any(|k| k == &key);
                let hunk_indicator = if is_hunk_folded { "▶" } else { "▼" };
                lines.push(format!("  {hunk_indicator} {}", hunk.header));
                meta.push(DiffLineMeta::Hunk { file_idx, hunk_idx });

                if is_hunk_folded {
                    continue;
                }

                for (line_idx, hline) in hunk.lines.iter().enumerate() {
                    let raw = match hline.origin {
                        '+' | '-' | ' ' => format!("{}{}", hline.origin, hline.content),
                        '\\' => format!("\\{}", hline.content),
                        _ => hline.content.clone(),
                    };
                    lines.push(format!("    {raw}"));
                    meta.push(DiffLineMeta::Line {
                        file_idx,
                        hunk_idx,
                        line_idx,
                    });
                }
            }
        }

        (lines.join("\n"), meta)
    }

    /// Toggle fold state in the diff view at the current cursor.
    ///
    /// Returns `true` if a foldable diff element was toggled.
    pub fn toggle_diff_fold_at_cursor(&mut self) -> bool {
        let Some(model) = self.diff_model.take() else {
            return false;
        };

        let Some(meta) = self.diff_line_meta.get(self.diff_cursor).copied() else {
            self.diff_model = Some(model);
            return false;
        };

        let Some((file_idx, hunk_idx)) = (match meta {
            DiffLineMeta::File { file_idx } => Some((file_idx, None)),
            DiffLineMeta::Hunk { file_idx, hunk_idx }
            | DiffLineMeta::Line {
                file_idx, hunk_idx, ..
            } => Some((file_idx, Some(hunk_idx))),
            _ => None,
        }) else {
            self.diff_model = Some(model);
            return false;
        };

        let mut handled = false;
        if let Some(file) = model.files.get(file_idx) {
            if let Some(hunk_idx) = hunk_idx {
                if let Some(hunk) = file.hunks.get(hunk_idx) {
                    let key = DiffHunkKey {
                        file_path: file.path.clone(),
                        old_start: hunk.old_start,
                        new_start: hunk.new_start,
                    };
                    if let Some(pos) = self.diff_folded_hunks.iter().position(|k| k == &key) {
                        self.diff_folded_hunks.remove(pos);
                    } else {
                        self.diff_folded_hunks.push(key);
                    }
                    handled = true;
                }
            } else if let Some(pos) = self.diff_folded_files.iter().position(|p| p == &file.path) {
                self.diff_folded_files.remove(pos);
                handled = true;
            } else {
                self.diff_folded_files.push(file.path.clone());
                handled = true;
            }
        }

        if handled {
            let (content, meta) = self.build_diff_view(&model);
            self.set_diff_view(content, meta);
        }

        self.diff_model = Some(model);
        handled
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

/// Identifies a foldable hunk in the diff view.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffHunkKey {
    /// File path for the hunk.
    pub file_path: PathBuf,
    /// Old start line number (from the diff header).
    pub old_start: u32,
    /// New start line number (from the diff header).
    pub new_start: u32,
}

/// Metadata for a displayed diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineMeta {
    /// Non-diff informational line.
    Info,
    /// A file header line (fold/unfold).
    File {
        /// File index in `diff_model.files`.
        file_idx: usize,
    },
    /// A hunk header line (fold/unfold).
    Hunk {
        /// File index in `diff_model.files`.
        file_idx: usize,
        /// Hunk index in `diff_model.files[file_idx].hunks`.
        hunk_idx: usize,
    },
    /// A line within a hunk.
    Line {
        /// File index in `diff_model.files`.
        file_idx: usize,
        /// Hunk index in `diff_model.files[file_idx].hunks`.
        hunk_idx: usize,
        /// Line index in `diff_model.files[file_idx].hunks[hunk_idx].lines`.
        line_idx: usize,
    },
    /// Unknown line type (fallback).
    Unknown,
}

/// One reversible edit applied from the diff view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEdit {
    /// The patch text to apply via `git apply`.
    pub patch: String,
    /// Whether the patch was applied with `-R` when the edit was first executed.
    pub applied_reverse: bool,
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
        // With 5 lines and height 3, max scroll is 2 (cursor clamping keeps it in range)
        assert_eq!(ui.diff_scroll, 2);
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
