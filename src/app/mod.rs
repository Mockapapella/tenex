//! Application state and logic

mod event;
mod handlers;
mod state;

pub use event::{Event, Handler};
pub use handlers::Actions;
pub use state::{App, BranchInfo, ConfirmAction, InputMode, Mode, Tab, WorktreeConflictInfo};
