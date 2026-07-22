//! Spawn state: child agent spawning configuration

/// Information about an existing worktree that conflicts with a new agent.
#[derive(Debug, Clone)]
pub struct WorktreeConflictInfo {
    /// The title the user entered for the new agent.
    pub title: String,
    /// Optional prompt for the new agent.
    pub prompt: Option<String>,
    /// The generated branch name.
    pub branch: String,
    /// The path to the existing worktree.
    pub worktree_path: std::path::PathBuf,
    /// The repository/workspace root where the conflicting worktree lives.
    pub repo_root: std::path::PathBuf,
    /// The branch the existing worktree is based on (if available).
    pub existing_branch: Option<String>,
    /// The commit hash of the existing worktree's HEAD (short form).
    pub existing_commit: Option<String>,
    /// The current HEAD branch that would be used for a new worktree.
    pub current_branch: String,
    /// The current HEAD commit hash (short form).
    pub current_commit: String,
    /// If this is a swarm creation, the number of children to spawn.
    pub swarm_child_count: Option<usize>,
}

/// State for spawning child agents
#[derive(Debug, Default)]
pub struct SpawnState {
    /// Number of child agents to spawn
    pub child_count: usize,

    /// Agent ID to spawn children under (None = create new root with children)
    pub spawning_under: Option<uuid::Uuid>,

    /// Whether to use the planning pre-prompt when spawning children
    pub use_plan_prompt: bool,

    /// Number of terminals spawned so far (for naming "Terminal 1", "Terminal 2", etc.)
    pub terminal_counter: usize,

    /// Information about a worktree conflict (when creating an agent with existing worktree)
    pub worktree_conflict: Option<WorktreeConflictInfo>,

    /// Repository/workspace root to use when spawning a new root swarm.
    pub root_repo_path: Option<std::path::PathBuf>,
}

impl SpawnState {
    /// Create a new spawn state with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            child_count: 3,
            spawning_under: None,
            use_plan_prompt: false,
            terminal_counter: 0,
            worktree_conflict: None,
            root_repo_path: None,
        }
    }

    /// Increment child count
    pub const fn increment_child_count(&mut self) {
        self.child_count = self.child_count.saturating_add(1);
    }

    /// Decrement child count (minimum 1)
    pub const fn decrement_child_count(&mut self) {
        if self.child_count > 1 {
            self.child_count -= 1;
        }
    }

    /// Start spawning children under a specific agent
    pub fn start_spawning_under(&mut self, parent_id: uuid::Uuid) {
        self.spawning_under = Some(parent_id);
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = false;
        self.root_repo_path = None;
    }

    /// Start spawning a new root agent with children (no plan prompt)
    pub fn start_spawning_root(&mut self) {
        self.spawning_under = None;
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = false;
        self.root_repo_path = None;
    }

    /// Start spawning a new root agent with children (with planning pre-prompt)
    pub fn start_planning_swarm(&mut self) {
        self.spawning_under = None;
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = true;
        self.root_repo_path = None;
    }

    /// Start spawning a planning swarm under an existing agent.
    pub fn start_planning_swarm_under(&mut self, parent_id: uuid::Uuid) {
        self.spawning_under = Some(parent_id);
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = true;
        self.root_repo_path = None;
    }

    /// Get the next terminal name and increment the counter
    pub fn next_terminal_name(&mut self) -> String {
        self.terminal_counter += 1;
        format!("Terminal {}", self.terminal_counter)
    }

    /// Set worktree conflict info
    pub fn set_conflict(&mut self, conflict: WorktreeConflictInfo) {
        self.worktree_conflict = Some(conflict);
    }

    /// Take and clear the worktree conflict info
    pub const fn take_conflict(&mut self) -> Option<WorktreeConflictInfo> {
        self.worktree_conflict.take()
    }
}

use super::App;
use crate::state::{ChildCountMode, ChildPromptMode, TerminalPromptMode};

impl App {
    /// Increment child count (for `ChildCount` mode)
    pub const fn increment_child_count(&mut self) {
        self.data.spawn.increment_child_count();
    }

    /// Decrement child count (minimum 1)
    pub const fn decrement_child_count(&mut self) {
        self.data.spawn.decrement_child_count();
    }

    /// Start spawning children under a specific agent
    pub fn start_spawning_under(&mut self, parent_id: uuid::Uuid) {
        self.data.spawn.start_spawning_under(parent_id);
        self.apply_mode(ChildCountMode.into());
    }

    /// Start spawning a new root agent with children (no plan prompt)
    pub fn start_spawning_root(&mut self) {
        self.data.spawn.start_spawning_root();
        self.data.spawn.root_repo_path = self.data.selected_project_root();
        self.apply_mode(ChildCountMode.into());
    }

    /// Start spawning a planning swarm under the selected agent
    pub fn start_planning_swarm(&mut self) {
        let Some(agent) = self.data.selected_agent() else {
            self.set_status("Select an agent first (press 'a')");
            return;
        };

        if agent.is_terminal_agent() {
            self.set_status("Select a non-terminal agent first (press 'a')");
            return;
        }

        self.data.spawn.start_planning_swarm_under(agent.id);
        self.apply_mode(ChildCountMode.into());
    }

    /// Proceed from `ChildCount` to `ChildPrompt` mode
    pub fn proceed_to_child_prompt(&mut self) {
        self.apply_mode(ChildPromptMode.into());
    }

    /// Get the next terminal name and increment the counter
    pub fn next_terminal_name(&mut self) -> String {
        self.data.spawn.next_terminal_name()
    }

    /// Start prompting for a terminal startup command
    pub fn start_terminal_prompt(&mut self) {
        self.apply_mode(TerminalPromptMode.into());
    }
}
