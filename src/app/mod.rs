//! Application state and logic

mod event;
mod handlers;
mod settings;
mod state;

pub use event::{Event, Handler};
pub use handlers::Actions;
pub use settings::{AgentProgram, Settings};
pub use state::{
    App, BranchInfo, BranchPickerKind, ConfirmAction, ConfirmKind, CountPickerKind, InputMode,
    Mode, OverlayMode, Tab, TextInputKind, WorktreeConflictInfo,
};
