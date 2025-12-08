//! Application state and logic

mod event;
mod handler;
mod state;

pub use event::{Event, Handler};
pub use handler::Actions;
pub use state::{App, BranchInfo, ConfirmAction, InputMode, Mode, Tab, WorktreeConflictInfo};
