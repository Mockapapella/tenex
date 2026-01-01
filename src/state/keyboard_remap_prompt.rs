//! Keyboard remap prompt mode state type (new architecture).

/// Keyboard remap prompt mode - asking to remap Ctrl+M due to terminal incompatibility.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardRemapPromptMode;
