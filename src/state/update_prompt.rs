//! Update prompt mode state type (new architecture).

use crate::update::UpdateInfo;

/// Update prompt mode - asking user to update Tenex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePromptMode {
    /// Details about the available update.
    pub info: UpdateInfo,
}
