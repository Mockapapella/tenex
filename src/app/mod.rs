//! Application state and logic

mod data;
mod event;
mod handlers;
mod settings;
mod state;

pub use crate::state::ConfirmAction;
pub use data::AppData;
pub use event::{Event, Handler};
pub use handlers::Actions;
pub use settings::{AgentProgram, AgentRole, Settings};
pub use state::{App, BranchInfo, DiffEdit, DiffLineMeta, InputMode, Tab, WorktreeConflictInfo};
