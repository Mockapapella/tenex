//! Scrolling mode state type (new architecture).

/// Scrolling mode - keybindings are the same as Normal, but the user is adjusting scroll offset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollingMode;
