//! Agent instance definition

use super::Status;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// A single agent instance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Unique identifier for the agent
    pub id: Uuid,

    /// Human-readable title/description
    pub title: String,

    /// Program being run (e.g., "claude", "aider")
    pub program: String,

    /// Current status of the agent
    pub status: Status,

    /// Git branch name for this agent's work
    pub branch: String,

    /// Path to the git worktree
    pub worktree_path: PathBuf,

    /// Initial prompt sent to the agent (if any)
    pub initial_prompt: Option<String>,

    /// Tmux session name
    pub tmux_session: String,

    /// When the agent was created
    pub created_at: DateTime<Utc>,

    /// When the agent was last updated
    pub updated_at: DateTime<Utc>,

    /// Parent agent ID (None for root agents)
    #[serde(default)]
    pub parent_id: Option<Uuid>,

    /// Tmux window index within the root ancestor's session (None for root agents)
    #[serde(default)]
    pub window_index: Option<u32>,

    /// Whether children are collapsed in the UI (default: true)
    #[serde(default = "default_collapsed")]
    pub collapsed: bool,
}

/// Default value for collapsed field
const fn default_collapsed() -> bool {
    true
}

/// Configuration for creating a child agent
#[derive(Debug, Clone)]
pub struct ChildConfig {
    /// Parent agent ID
    pub parent_id: Uuid,
    /// Tmux session name (from root ancestor)
    pub tmux_session: String,
    /// Window index in the session
    pub window_index: u32,
}

impl Agent {
    /// Create a new agent with the given parameters
    #[must_use]
    pub fn new(
        title: String,
        program: String,
        branch: String,
        worktree_path: PathBuf,
        initial_prompt: Option<String>,
    ) -> Self {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let tmux_session = format!("tenex-{}", &id.to_string()[..8]);

        Self {
            id,
            title,
            program,
            status: Status::Starting,
            branch,
            worktree_path,
            initial_prompt,
            tmux_session,
            created_at: now,
            updated_at: now,
            parent_id: None,
            window_index: None,
            collapsed: true,
        }
    }

    /// Create a new child agent under a parent
    #[must_use]
    pub fn new_child(
        title: String,
        program: String,
        branch: String,
        worktree_path: PathBuf,
        initial_prompt: Option<String>,
        config: ChildConfig,
    ) -> Self {
        let id = Uuid::new_v4();
        let now = Utc::now();

        Self {
            id,
            title,
            program,
            status: Status::Starting,
            branch,
            worktree_path,
            initial_prompt,
            tmux_session: config.tmux_session,
            created_at: now,
            updated_at: now,
            parent_id: Some(config.parent_id),
            window_index: Some(config.window_index),
            collapsed: true,
        }
    }

    /// Check if this agent is a root agent (no parent)
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    /// Check if this agent is a child agent (has a parent)
    #[must_use]
    pub const fn is_child(&self) -> bool {
        self.parent_id.is_some()
    }

    /// Get a short display ID (first 8 chars of UUID)
    #[must_use]
    pub fn short_id(&self) -> String {
        self.id.to_string()[..8].to_string()
    }

    /// Update the agent's status
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    /// Check if this agent is still active (not stopped)
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.status != Status::Stopped
    }

    /// Get the age of the agent as a human-readable string
    #[must_use]
    pub fn age_string(&self) -> String {
        let duration = Utc::now().signed_duration_since(self.created_at);

        if duration.num_days() > 0 {
            format!("{}d", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{}h", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{}m", duration.num_minutes())
        } else {
            format!("{}s", duration.num_seconds().max(0))
        }
    }

    /// Get a one-line summary of the agent
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "[{}] {} ({}) - {}",
            self.short_id(),
            self.title,
            self.program,
            self.status
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    fn create_test_agent() -> Agent {
        Agent::new(
            "Test Agent".to_string(),
            "claude".to_string(),
            "tenex/test-agent".to_string(),
            PathBuf::from("/tmp/worktree"),
            None,
        )
    }

    #[test]
    fn test_new_agent() {
        let agent = create_test_agent();

        assert_eq!(agent.title, "Test Agent");
        assert_eq!(agent.program, "claude");
        assert_eq!(agent.status, Status::Starting);
        assert_eq!(agent.branch, "tenex/test-agent");
        assert!(agent.tmux_session.starts_with("tenex-"));
        assert!(agent.initial_prompt.is_none());
    }

    #[test]
    fn test_agent_with_prompt() {
        let agent = Agent::new(
            "Fix Bug".to_string(),
            "aider".to_string(),
            "tenex/fix-bug".to_string(),
            PathBuf::from("/tmp/worktree"),
            Some("Fix the authentication bug".to_string()),
        );

        assert_eq!(
            agent.initial_prompt,
            Some("Fix the authentication bug".to_string())
        );
    }

    #[test]
    fn test_short_id() {
        let agent = create_test_agent();
        let short_id = agent.short_id();

        assert_eq!(short_id.len(), 8);
        assert!(short_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn test_set_status() {
        let mut agent = create_test_agent();
        let original_updated = agent.updated_at;

        sleep(Duration::from_millis(10));

        agent.set_status(Status::Running);

        assert_eq!(agent.status, Status::Running);
        assert!(agent.updated_at > original_updated);
    }

    #[test]
    fn test_is_alive() {
        let mut agent = create_test_agent();

        assert!(agent.is_alive());

        agent.set_status(Status::Running);
        assert!(agent.is_alive());

        agent.set_status(Status::Paused);
        assert!(agent.is_alive());

        agent.set_status(Status::Stopped);
        assert!(!agent.is_alive());
    }

    #[test]
    fn test_age_string() {
        let agent = create_test_agent();
        let age = agent.age_string();

        assert!(age.ends_with('s') || age.ends_with('m'));
    }

    #[test]
    fn test_summary() {
        let agent = create_test_agent();
        let summary = agent.summary();

        assert!(summary.contains("Test Agent"));
        assert!(summary.contains("claude"));
        assert!(summary.contains("Starting"));
    }

    #[test]
    fn test_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let agent = create_test_agent();
        let json = serde_json::to_string(&agent)?;
        let parsed: Agent = serde_json::from_str(&json)?;

        assert_eq!(agent.id, parsed.id);
        assert_eq!(agent.title, parsed.title);
        assert_eq!(agent.program, parsed.program);
        assert_eq!(agent.status, parsed.status);
        assert_eq!(agent.branch, parsed.branch);
        Ok(())
    }

    #[test]
    fn test_unique_ids() {
        let agent1 = create_test_agent();
        let agent2 = create_test_agent();

        assert_ne!(agent1.id, agent2.id);
        assert_ne!(agent1.tmux_session, agent2.tmux_session);
    }

    #[test]
    fn test_new_agent_is_root() {
        let agent = create_test_agent();
        assert!(agent.is_root());
        assert!(!agent.is_child());
        assert!(agent.parent_id.is_none());
        assert!(agent.window_index.is_none());
        assert!(agent.collapsed);
    }

    #[test]
    fn test_new_child_agent() {
        let parent = create_test_agent();
        let child = Agent::new_child(
            "Child Agent".to_string(),
            "claude".to_string(),
            parent.branch.clone(),
            parent.worktree_path.clone(),
            Some("Research task".to_string()),
            ChildConfig {
                parent_id: parent.id,
                tmux_session: parent.tmux_session.clone(),
                window_index: 2,
            },
        );

        assert!(!child.is_root());
        assert!(child.is_child());
        assert_eq!(child.parent_id, Some(parent.id));
        assert_eq!(child.window_index, Some(2));
        assert_eq!(child.tmux_session, parent.tmux_session);
        assert!(child.collapsed);
    }

    #[test]
    fn test_serde_roundtrip_with_hierarchy() -> Result<(), Box<dyn std::error::Error>> {
        let parent = create_test_agent();
        let child = Agent::new_child(
            "Child".to_string(),
            "claude".to_string(),
            parent.branch.clone(),
            parent.worktree_path.clone(),
            None,
            ChildConfig {
                parent_id: parent.id,
                tmux_session: parent.tmux_session,
                window_index: 2,
            },
        );

        let json = serde_json::to_string(&child)?;
        let parsed: Agent = serde_json::from_str(&json)?;

        assert_eq!(child.parent_id, parsed.parent_id);
        assert_eq!(child.window_index, parsed.window_index);
        assert_eq!(child.collapsed, parsed.collapsed);
        Ok(())
    }

    #[test]
    fn test_serde_backwards_compatibility() -> Result<(), Box<dyn std::error::Error>> {
        // Test that old JSON without hierarchy fields deserializes correctly
        let old_json = r#"{
            "id": "12345678-1234-1234-1234-123456789012",
            "title": "Old Agent",
            "program": "claude",
            "status": "running",
            "branch": "tenex/old",
            "worktree_path": "/tmp/old",
            "initial_prompt": null,
            "tmux_session": "tenex-12345678",
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let agent: Agent = serde_json::from_str(old_json)?;
        assert!(agent.parent_id.is_none());
        assert!(agent.window_index.is_none());
        assert!(agent.collapsed); // default value
        Ok(())
    }
}
