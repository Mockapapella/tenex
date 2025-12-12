//! Application state and logic

mod event;
mod handlers;
mod settings;
mod state;

pub use event::{Event, Handler};
pub use handlers::Actions;
pub use settings::Settings;
pub use state::{App, BranchInfo, ConfirmAction, InputMode, Mode, Tab, WorktreeConflictInfo};
