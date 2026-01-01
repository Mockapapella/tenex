//! Success modal state type (new architecture).

/// Success modal mode - displaying a success message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessModalMode {
    /// Success message shown in the modal.
    pub message: String,
}
