//! Agent instance definition

use super::Status;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// What kind of workspace an agent runs in.
///
/// Most agents run in a git worktree managed by Tenex, but Tenex also supports starting in a
/// regular directory that is not a git repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    /// A git worktree and branch managed by Tenex.
    #[default]
    GitWorktree,
    /// A regular directory (no worktree isolation / git operations).
    PlainDir,
}

/// Where Tenex should run the agent process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntime {
    /// Run the agent directly on the host.
    #[default]
    Host,
    /// Run the agent inside a Docker container scoped to the root session.
    Docker,
}

/// Persisted identity, process, workspace, and hierarchy state for one agent.
#[expect(
    missing_docs,
    reason = "self-describing fields omit restatements; persistence exceptions remain documented"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,

    pub title: String,

    /// Program being run (e.g., "claude", "aider")
    pub program: String,

    /// Conversation/session id for resuming an agent after a reboot/crash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,

    pub status: Status,

    pub branch: String,

    pub worktree_path: PathBuf,

    /// Root directory of the repository/workspace this agent belongs to.
    ///
    /// For git worktrees this is the main repository root (not the worktree path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<PathBuf>,

    #[serde(default)]
    pub workspace_kind: WorkspaceKind,

    #[serde(default)]
    pub runtime: AgentRuntime,

    /// Stable identifier for runtime resources shared by a root agent tree.
    ///
    /// Docker roots use this instead of the mux session name so renaming the session does not
    /// silently create a second container. Older state files omit it and fall back to
    /// `mux_session`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runtime_scope: String,

    #[serde(alias = "tmux_session")]
    pub mux_session: String,

    pub created_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,

    #[serde(default)]
    pub parent_id: Option<Uuid>,

    #[serde(default)]
    pub window_index: Option<u32>,

    /// Whether children are collapsed in this client (default: true).
    #[serde(skip, default = "default_collapsed")]
    pub collapsed: bool,

    /// Whether this is a terminal (not a Claude agent) - excluded from broadcast
    #[serde(default)]
    pub is_terminal: bool,
}

const fn default_collapsed() -> bool {
    true
}

/// Parent session coordinates inherited by a new child.
#[expect(
    missing_docs,
    reason = "the coordinate field names and types are the complete contract"
)]
#[derive(Debug, Clone)]
pub struct ChildConfig {
    pub parent_id: Uuid,
    pub mux_session: String,
    pub window_index: u32,
    /// Repository/workspace root for the agent.
    pub repo_root: Option<PathBuf>,
}

#[expect(
    missing_docs,
    reason = "self-describing constructors and predicates omit restatements"
)]
impl Agent {
    #[must_use]
    pub fn new(title: String, program: String, branch: String, worktree_path: PathBuf) -> Self {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let mux_session = format!("tenex-{}", &id.to_string()[..8]);

        Self {
            id,
            title,
            program,
            conversation_id: None,
            status: Status::Starting,
            branch,
            worktree_path,
            repo_root: None,
            workspace_kind: WorkspaceKind::GitWorktree,
            runtime: AgentRuntime::Host,
            runtime_scope: String::new(),
            mux_session,
            created_at: now,
            updated_at: now,
            parent_id: None,
            window_index: None,
            collapsed: true,
            is_terminal: false,
        }
    }

    #[must_use]
    pub fn new_child(
        title: String,
        program: String,
        branch: String,
        worktree_path: PathBuf,
        config: ChildConfig,
    ) -> Self {
        let id = Uuid::new_v4();
        let now = Utc::now();

        Self {
            id,
            title,
            program,
            conversation_id: None,
            status: Status::Starting,
            branch,
            worktree_path,
            repo_root: config.repo_root,
            workspace_kind: WorkspaceKind::GitWorktree,
            runtime: AgentRuntime::Host,
            runtime_scope: String::new(),
            mux_session: config.mux_session,
            created_at: now,
            updated_at: now,
            parent_id: Some(config.parent_id),
            window_index: Some(config.window_index),
            collapsed: true,
            is_terminal: false,
        }
    }

    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    #[must_use]
    pub const fn is_child(&self) -> bool {
        self.parent_id.is_some()
    }

    /// Whether this agent represents an interactive terminal window.
    ///
    /// `is_terminal` is the canonical flag, but older state files may only have `program="terminal"`.
    #[must_use]
    pub fn is_terminal_agent(&self) -> bool {
        self.is_terminal || self.program == "terminal"
    }

    #[must_use]
    pub const fn is_git_workspace(&self) -> bool {
        matches!(self.workspace_kind, WorkspaceKind::GitWorktree)
    }

    /// Get a short display ID (first 8 chars of UUID)
    #[must_use]
    pub fn short_id(&self) -> String {
        self.id.to_string()[..8].to_string()
    }

    /// Runtime resource scope for this agent tree.
    ///
    /// Older agents fall back to their mux session name because that was the original Docker
    /// identity key.
    #[must_use]
    pub fn effective_runtime_scope(&self) -> &str {
        if self.runtime_scope.trim().is_empty() {
            &self.mux_session
        } else {
            &self.runtime_scope
        }
    }

    /// Set the status and refresh `updated_at`.
    pub fn set_status(&mut self, status: Status) {
        self.status = status;
        self.updated_at = Utc::now();
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
