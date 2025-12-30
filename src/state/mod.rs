//! Compile-time state types (new architecture).

mod normal;

pub use normal::NormalMode;

use crate::app::{App, Mode};

/// A transitional "next state" wrapper used during migration.
///
/// For Milestone 1, we keep using the existing runtime `Mode` enum for all
/// non-normal modes, while introducing a dedicated `NormalMode` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeUnion {
    /// Remain in (or return to) the new, dedicated `NormalMode`.
    Normal(NormalMode),
    /// Transition to a legacy runtime `Mode`.
    Legacy(Mode),
}

impl ModeUnion {
    /// Convenience constructor for `ModeUnion::Normal`.
    #[must_use]
    pub const fn normal() -> Self {
        Self::Normal(NormalMode)
    }

    /// Apply the mode transition to the legacy `App` state.
    pub fn apply(self, app: &mut App) {
        match self {
            Self::Normal(_) => {}
            Self::Legacy(mode) => match mode {
                Mode::CommandPalette => app.start_command_palette(),
                Mode::ErrorModal(message) => app.set_error(message),
                Mode::SuccessModal(message) => app.show_success(message),
                other => app.enter_mode(other),
            },
        }
    }
}

impl From<Mode> for ModeUnion {
    fn from(mode: Mode) -> Self {
        Self::Legacy(mode)
    }
}

impl From<NormalMode> for ModeUnion {
    fn from(_: NormalMode) -> Self {
        Self::Normal(NormalMode)
    }
}
