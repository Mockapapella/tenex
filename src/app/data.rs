//! Shared application data/context for action execution.

use super::{Actions, App};

/// Shared mutable access to the application state plus handler context.
///
/// This is intentionally lightweight for Milestone 1: it wraps the existing `App`
/// rather than extracting a new persistent data struct.
#[derive(Debug)]
pub struct AppData<'a> {
    /// Mutable access to the legacy `App` state.
    pub app: &'a mut App,
    /// Handler context used by certain actions (e.g., spawning terminals).
    pub actions: Actions,
}

impl<'a> AppData<'a> {
    /// Create a new `AppData` wrapper around an `App` plus handler context.
    #[must_use]
    pub const fn new(app: &'a mut App, actions: Actions) -> Self {
        Self { app, actions }
    }
}

impl std::ops::Deref for AppData<'_> {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        self.app
    }
}

impl std::ops::DerefMut for AppData<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.app
    }
}
