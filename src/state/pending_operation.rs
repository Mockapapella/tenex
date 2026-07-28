//! User operations that start after Tenex renders their acknowledgement.

use std::path::PathBuf;
use uuid::Uuid;

/// A complete user request that Tenex can execute without reading mutable selection state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingOperation {
    /// Prepare the Docker image and enable Docker for future root agents.
    PrepareDocker,
    /// Create a root agent in the repository selected when Tenex accepted the command.
    CreateAgent {
        /// Repository or plain directory selected for the new agent.
        repo_path: PathBuf,
        /// Title submitted for the new agent.
        title: String,
        /// Prompt submitted with the title.
        prompt: Option<String>,
    },
    /// Delete the agent that the user confirmed.
    KillAgent {
        /// Agent identity captured before Tenex renders the acknowledgement.
        agent_id: Uuid,
    },
    /// Create reviewers from the values confirmed in the review flow.
    SpawnReviewAgents {
        /// Parent agent selected for the review.
        parent_id: Uuid,
        /// Number of reviewers requested by the user.
        count: usize,
        /// Branch selected as the review base.
        base_branch: String,
        /// Reviewer command selected from settings.
        program: String,
    },
}

impl PendingOperation {
    /// Return the modal title for this request.
    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Self::PrepareDocker => "Preparing Docker",
            Self::CreateAgent { .. } => "Starting agent",
            Self::KillAgent { .. } => "Deleting agent",
            Self::SpawnReviewAgents { .. } => "Starting review",
        }
    }

    /// Return the progress message for this request.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::PrepareDocker => "Preparing the shipped Tenex Docker worker image. The first setup can take a minute; later root agents reuse it.".to_string(),
            Self::CreateAgent { title, .. } => {
                format!("Starting {title}. Tenex will show it as soon as it is ready.")
            }
            Self::KillAgent { .. } => {
                "Stopping the agent and cleaning up its workspace.".to_string()
            }
            Self::SpawnReviewAgents { .. } => {
                "Creating the requested reviewers and starting their review flows.".to_string()
            }
        }
    }
}
