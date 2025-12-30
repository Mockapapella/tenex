//! Input state: text input buffer and cursor management

/// State for text input in various modes
#[derive(Debug, Default)]
pub struct InputState {
    /// Input buffer for text input modes
    pub buffer: String,

    /// Cursor position within `buffer` (byte offset)
    pub cursor: usize,

    /// Scroll position in input modal (for multiline text)
    pub scroll: u16,
}

impl InputState {
    /// Create a new input state with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            scroll: 0,
        }
    }

    /// Clear the input buffer and reset cursor
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Clear the current input buffer.
    ///
    /// This matches typical line-editor behavior for "clear line" in Tenex's
    /// text input modals.
    pub fn clear_line(&mut self) {
        self.clear();
    }

    /// Delete the previous word (like many shell/readline editors).
    ///
    /// This removes any whitespace immediately before the cursor, then removes
    /// the contiguous non-whitespace "word" segment.
    pub fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let mut start = self.cursor;
        let mut found_non_whitespace = false;

        for (index, ch) in self.buffer[..self.cursor].char_indices().rev() {
            if !found_non_whitespace {
                if ch.is_whitespace() {
                    start = index;
                    continue;
                }
                found_non_whitespace = true;
                start = index;
                continue;
            }

            if ch.is_whitespace() {
                start = index.saturating_add(ch.len_utf8());
                break;
            }

            start = index;
        }

        self.buffer.drain(start..self.cursor);
        self.cursor = start;
    }

    /// Set the input buffer content and move cursor to end
    pub fn set(&mut self, content: String) {
        self.cursor = content.len();
        self.buffer = content;
    }

    /// Insert a character at the cursor position
    pub fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete the character before the cursor (backspace)
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous character boundary
            let prev_char_boundary = self.buffer[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            self.buffer.remove(prev_char_boundary);
            self.cursor = prev_char_boundary;
        }
    }

    /// Delete the character at the cursor (delete key)
    pub fn delete(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    /// Move cursor left by one character
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            // Find previous character boundary
            self.cursor = self.buffer[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    /// Move cursor right by one character
    pub fn cursor_right(&mut self) {
        if self.cursor < self.buffer.len() {
            // Find next character boundary
            self.cursor = self.buffer[self.cursor..]
                .char_indices()
                .nth(1)
                .map_or(self.buffer.len(), |(i, _)| self.cursor + i);
        }
    }

    /// Move cursor up one line (for multiline input)
    pub fn cursor_up(&mut self) {
        let text = &self.buffer[..self.cursor];
        // Find current line start and column
        let current_line_start = text.rfind('\n').map_or(0, |i| i + 1);
        let column = self.cursor - current_line_start;

        if current_line_start > 0 {
            // Find previous line
            let prev_text = &self.buffer[..current_line_start - 1];
            let prev_line_start = prev_text.rfind('\n').map_or(0, |i| i + 1);
            let prev_line_len = current_line_start - 1 - prev_line_start;

            // Move to same column or end of previous line
            self.cursor = prev_line_start + column.min(prev_line_len);
        }
    }

    /// Move cursor down one line (for multiline input)
    pub fn cursor_down(&mut self) {
        let text = &self.buffer;
        // Find current line start and column
        let before_cursor = &text[..self.cursor];
        let current_line_start = before_cursor.rfind('\n').map_or(0, |i| i + 1);
        let column = self.cursor - current_line_start;

        // Find next line
        if let Some(next_newline) = text[self.cursor..].find('\n') {
            let next_line_start = self.cursor + next_newline + 1;
            let next_line_end = text[next_line_start..]
                .find('\n')
                .map_or(text.len(), |i| next_line_start + i);
            let next_line_len = next_line_end - next_line_start;

            // Move to same column or end of next line
            self.cursor = next_line_start + column.min(next_line_len);
        }
    }

    /// Move cursor to start of current line
    pub fn cursor_home(&mut self) {
        let text = &self.buffer[..self.cursor];
        self.cursor = text.rfind('\n').map_or(0, |i| i + 1);
    }

    /// Move cursor to end of current line
    pub fn cursor_end(&mut self) {
        let text = &self.buffer[self.cursor..];
        self.cursor += text.find('\n').unwrap_or(text.len());
    }

    /// Get the trimmed content of the buffer
    #[must_use]
    pub fn trimmed(&self) -> &str {
        self.buffer.trim()
    }

    /// Check if the buffer is empty (after trimming)
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_state_new() {
        let input = InputState::new();
        assert!(input.buffer.is_empty());
        assert_eq!(input.cursor, 0);
        assert_eq!(input.scroll, 0);
    }

    #[test]
    fn test_clear() {
        let mut input = InputState::new();
        input.buffer = "test".to_string();
        input.cursor = 3;
        input.scroll = 1;

        input.clear();

        assert!(input.buffer.is_empty());
        assert_eq!(input.cursor, 0);
        assert_eq!(input.scroll, 0);
    }

    #[test]
    fn test_set() {
        let mut input = InputState::new();
        input.set("hello".to_string());

        assert_eq!(input.buffer, "hello");
        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn test_insert_char() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 5;

        input.insert_char('!');

        assert_eq!(input.buffer, "hello!");
        assert_eq!(input.cursor, 6);
    }

    #[test]
    fn test_insert_char_middle() {
        let mut input = InputState::new();
        input.buffer = "hllo".to_string();
        input.cursor = 1;

        input.insert_char('e');

        assert_eq!(input.buffer, "hello");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn test_backspace() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 5;

        input.backspace();

        assert_eq!(input.buffer, "hell");
        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn test_backspace_at_start() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 0;

        input.backspace();

        assert_eq!(input.buffer, "hello");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_delete() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 0;

        input.delete();

        assert_eq!(input.buffer, "ello");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_delete_at_end() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 5;

        input.delete();

        assert_eq!(input.buffer, "hello");
        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn test_cursor_left() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 3;

        input.cursor_left();

        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn test_cursor_left_at_start() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 0;

        input.cursor_left();

        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_cursor_right() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 3;

        input.cursor_right();

        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn test_cursor_right_at_end() {
        let mut input = InputState::new();
        input.buffer = "hello".to_string();
        input.cursor = 5;

        input.cursor_right();

        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn test_cursor_up() {
        let mut input = InputState::new();
        input.buffer = "line1\nline2".to_string();
        input.cursor = 8; // Middle of line2

        input.cursor_up();

        assert_eq!(input.cursor, 2); // Corresponding position in line1
    }

    #[test]
    fn test_cursor_up_on_first_line() {
        let mut input = InputState::new();
        input.buffer = "line1\nline2".to_string();
        input.cursor = 2;

        input.cursor_up();

        assert_eq!(input.cursor, 2); // Stays on first line
    }

    #[test]
    fn test_cursor_down() {
        let mut input = InputState::new();
        input.buffer = "line1\nline2".to_string();
        input.cursor = 2; // In line1

        input.cursor_down();

        assert_eq!(input.cursor, 8); // Corresponding position in line2
    }

    #[test]
    fn test_cursor_down_on_last_line() {
        let mut input = InputState::new();
        input.buffer = "line1\nline2".to_string();
        input.cursor = 8;

        input.cursor_down();

        assert_eq!(input.cursor, 8); // Stays on last line
    }

    #[test]
    fn test_cursor_home() {
        let mut input = InputState::new();
        input.buffer = "line1\nline2".to_string();
        input.cursor = 8; // Middle of line2

        input.cursor_home();

        assert_eq!(input.cursor, 6); // Start of line2
    }

    #[test]
    fn test_cursor_end() {
        let mut input = InputState::new();
        input.buffer = "line1\nline2".to_string();
        input.cursor = 7; // In line2

        input.cursor_end();

        assert_eq!(input.cursor, 11); // End of line2
    }

    #[test]
    fn test_trimmed() {
        let mut input = InputState::new();
        input.buffer = "  hello  ".to_string();

        assert_eq!(input.trimmed(), "hello");
    }

    #[test]
    fn test_is_empty() {
        let mut input = InputState::new();
        assert!(input.is_empty());

        input.buffer = "  ".to_string();
        assert!(input.is_empty());

        input.buffer = "hello".to_string();
        assert!(!input.is_empty());
    }
}
