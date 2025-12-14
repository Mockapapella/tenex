//! Slash command palette state

/// State for the `/` command palette
#[derive(Debug, Default, Clone, Copy)]
pub struct CommandPaletteState {
    /// Currently selected index in filtered list
    pub selected: usize,
}

impl CommandPaletteState {
    /// Create a new command palette state
    #[must_use]
    pub const fn new() -> Self {
        Self { selected: 0 }
    }

    /// Reset the palette selection
    pub const fn reset(&mut self) {
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let state = CommandPaletteState::new();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_default() {
        let state = CommandPaletteState::default();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_reset() {
        let mut state = CommandPaletteState::new();
        state.selected = 5;
        state.reset();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn test_clone() {
        let mut state = CommandPaletteState::new();
        state.selected = 3;
        let cloned = state;
        assert_eq!(cloned.selected, 3);
    }

    #[test]
    fn test_debug() {
        let state = CommandPaletteState::new();
        let debug_str = format!("{state:?}");
        assert!(debug_str.contains("CommandPaletteState"));
        assert!(debug_str.contains("selected"));
    }
}
