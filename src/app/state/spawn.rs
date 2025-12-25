//! Spawn state: child agent spawning configuration

use crate::app::state::WorktreeConflictInfo;

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
    pub const fn start_spawning_under(&mut self, parent_id: uuid::Uuid) {
        self.spawning_under = Some(parent_id);
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = false;
    }

    /// Start spawning a new root agent with children (no plan prompt)
    pub const fn start_spawning_root(&mut self) {
        self.spawning_under = None;
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = false;
    }

    /// Start spawning a new root agent with children (with planning pre-prompt)
    pub const fn start_planning_swarm(&mut self) {
        self.spawning_under = None;
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = true;
    }

    /// Start spawning a planning swarm under an existing agent.
    pub const fn start_planning_swarm_under(&mut self, parent_id: uuid::Uuid) {
        self.spawning_under = Some(parent_id);
        self.child_count = 3; // Reset to default
        self.use_plan_prompt = true;
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

use super::{App, Mode};

impl App {
    /// Increment child count (for `ChildCount` mode)
    pub const fn increment_child_count(&mut self) {
        self.spawn.increment_child_count();
    }

    /// Decrement child count (minimum 1)
    pub const fn decrement_child_count(&mut self) {
        self.spawn.decrement_child_count();
    }

    /// Start spawning children under a specific agent
    pub fn start_spawning_under(&mut self, parent_id: uuid::Uuid) {
        self.spawn.start_spawning_under(parent_id);
        self.enter_mode(Mode::ChildCount);
    }

    /// Start spawning a new root agent with children (no plan prompt)
    pub fn start_spawning_root(&mut self) {
        self.spawn.start_spawning_root();
        self.enter_mode(Mode::ChildCount);
    }

    /// Start spawning a planning swarm under the selected agent
    pub fn start_planning_swarm(&mut self) {
        let Some(agent) = self.selected_agent() else {
            self.set_status("Select an agent first (press 'a')");
            return;
        };

        self.spawn.start_planning_swarm_under(agent.id);
        self.enter_mode(Mode::ChildCount);
    }

    /// Proceed from `ChildCount` to `ChildPrompt` mode
    pub fn proceed_to_child_prompt(&mut self) {
        self.enter_mode(Mode::ChildPrompt);
    }

    /// Get the next terminal name and increment the counter
    pub fn next_terminal_name(&mut self) -> String {
        self.spawn.next_terminal_name()
    }

    /// Start prompting for a terminal startup command
    pub fn start_terminal_prompt(&mut self) {
        self.enter_mode(Mode::TerminalPrompt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_state_new() {
        let state = SpawnState::new();
        assert_eq!(state.child_count, 3);
        assert!(state.spawning_under.is_none());
        assert!(!state.use_plan_prompt);
        assert_eq!(state.terminal_counter, 0);
        assert!(state.worktree_conflict.is_none());
    }

    #[test]
    fn test_increment_child_count() {
        let mut state = SpawnState::new();
        assert_eq!(state.child_count, 3);

        state.increment_child_count();
        assert_eq!(state.child_count, 4);

        state.increment_child_count();
        assert_eq!(state.child_count, 5);
    }

    #[test]
    fn test_decrement_child_count() {
        let mut state = SpawnState::new();
        assert_eq!(state.child_count, 3);

        state.decrement_child_count();
        assert_eq!(state.child_count, 2);

        state.decrement_child_count();
        assert_eq!(state.child_count, 1);

        // Minimum of 1
        state.decrement_child_count();
        assert_eq!(state.child_count, 1);
    }

    #[test]
    fn test_start_spawning_under() {
        let mut state = SpawnState::new();
        state.child_count = 10;
        state.use_plan_prompt = true;

        let parent_id = uuid::Uuid::new_v4();
        state.start_spawning_under(parent_id);

        assert_eq!(state.spawning_under, Some(parent_id));
        assert_eq!(state.child_count, 3); // Reset to default
        assert!(!state.use_plan_prompt);
    }

    #[test]
    fn test_start_spawning_root() {
        let mut state = SpawnState::new();
        state.child_count = 10;
        state.spawning_under = Some(uuid::Uuid::new_v4());
        state.use_plan_prompt = true;

        state.start_spawning_root();

        assert!(state.spawning_under.is_none());
        assert_eq!(state.child_count, 3); // Reset to default
        assert!(!state.use_plan_prompt);
    }

    #[test]
    fn test_start_planning_swarm() {
        let mut state = SpawnState::new();
        state.child_count = 10;
        state.spawning_under = Some(uuid::Uuid::new_v4());

        state.start_planning_swarm();

        assert!(state.spawning_under.is_none());
        assert_eq!(state.child_count, 3); // Reset to default
        assert!(state.use_plan_prompt);
    }

    #[test]
    fn test_start_planning_swarm_under() {
        let mut state = SpawnState::new();
        state.child_count = 10;
        state.spawning_under = None;

        let parent_id = uuid::Uuid::new_v4();
        state.start_planning_swarm_under(parent_id);

        assert_eq!(state.spawning_under, Some(parent_id));
        assert_eq!(state.child_count, 3); // Reset to default
        assert!(state.use_plan_prompt);
    }

    #[test]
    fn test_next_terminal_name() {
        let mut state = SpawnState::new();

        assert_eq!(state.next_terminal_name(), "Terminal 1");
        assert_eq!(state.next_terminal_name(), "Terminal 2");
        assert_eq!(state.next_terminal_name(), "Terminal 3");
    }

    #[test]
    fn test_set_and_take_conflict() {
        let mut state = SpawnState::new();
        let conflict = WorktreeConflictInfo {
            title: "test-agent".to_string(),
            prompt: None,
            branch: "test-branch".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/test"),
            existing_branch: None,
            existing_commit: None,
            current_branch: "main".to_string(),
            current_commit: "abc123".to_string(),
            swarm_child_count: None,
        };

        state.set_conflict(conflict);
        assert!(state.worktree_conflict.is_some());

        let taken = state.take_conflict();
        assert!(taken.is_some());
        assert!(state.worktree_conflict.is_none());
    }
}
