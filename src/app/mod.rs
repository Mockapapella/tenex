//! Application state and logic

mod data;
mod event;
mod handlers;
mod settings;
pub(crate) mod sidebar;
mod state;

pub use crate::state::ConfirmAction;
pub use data::AppData;
pub use event::{Event, Handler};
pub use handlers::Actions;
pub use settings::{AgentProgram, AgentRole, Settings};
pub(crate) use sidebar::{SidebarItem, SidebarProject};
pub use state::{
    App, BranchInfo, DiffEdit, DiffLineMeta, InputMode, MuxdVersionMismatchInfo, Tab,
    WorktreeConflictInfo,
};
