//! Application state and logic

mod data;
mod event;
mod handlers;
mod settings;
mod state;

pub use data::AppData;
pub use event::{Event, Handler};
pub use handlers::Actions;
pub use settings::{AgentProgram, Settings};
pub use state::{App, BranchInfo, ConfirmAction, InputMode, Mode, Tab, WorktreeConflictInfo};
