//! Confirming mode state type (new architecture).

use crate::app::ConfirmAction;

/// Confirming mode - yes/no (or special) confirmations for various actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmingMode {
    /// The action being confirmed.
    pub action: ConfirmAction,
}
