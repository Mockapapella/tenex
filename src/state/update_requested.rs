//! Update requested mode state type (new architecture).

use crate::update::UpdateInfo;

/// Update requested mode - update in progress and input is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRequestedMode {
    /// Details about the update being installed.
    pub info: UpdateInfo,
}
