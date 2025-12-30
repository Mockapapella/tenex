//! Error modal state type (new architecture).

/// Error modal mode - displaying an error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorModalMode {
    /// Error message shown in the modal.
    pub message: String,
}
