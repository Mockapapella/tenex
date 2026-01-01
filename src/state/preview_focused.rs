//! Preview focused mode state type (new architecture).

/// Preview focused mode - keystrokes are forwarded to the mux backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreviewFocusedMode;
