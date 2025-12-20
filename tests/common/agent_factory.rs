//! Factory functions for creating test agents

use tenex::agent::{Agent, ChildConfig};

/// Helper to create a child agent with specified parent and window index
pub fn create_child_agent(parent: &Agent, title: &str, window_index: u32) -> Agent {
    Agent::new_child(
        title.to_string(),
        "echo".to_string(),
        parent.branch.clone(),
        parent.worktree_path.clone(),
        None,
        ChildConfig {
            parent_id: parent.id,
            mux_session: parent.mux_session.clone(),
            window_index,
        },
    )
}
