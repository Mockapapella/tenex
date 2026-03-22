//! Preparing Docker modal state.

/// Preparing Docker mode. Input is ignored while Tenex prepares the worker image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparingDockerMode {
    /// Message shown while Tenex prepares Docker.
    pub message: String,
}
