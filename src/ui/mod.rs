//! UI components for the TUI
//!
//! This module contains reusable UI components and rendering utilities.
//! The main TUI implementation is in the binary crate's `tui` module.

mod components;

pub use components::{
    agent_list::Widget as AgentListWidget,
    diff_view::Widget as DiffViewWidget,
    preview::Widget as PreviewWidget,
    status_bar::Widget as StatusBarWidget,
};
