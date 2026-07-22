//! Agent status definitions

use serde::{Deserialize, Serialize};
use std::fmt;

/// Status of an agent instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Agent is starting up
    #[default]
    Starting,
    /// Agent is actively running
    Running,
}

impl Status {
    /// Check if the agent is in an active state (can receive input)
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the agent can be killed (all agents can be killed)
    #[must_use]
    pub const fn can_kill(&self) -> bool {
        true
    }

    /// Get the display symbol for the status
    #[must_use]
    pub const fn symbol(&self) -> &'static str {
        match self {
            Self::Starting => "...",
            Self::Running => "●",
        }
    }

    /// Get the display color name for the status
    #[must_use]
    pub const fn color_name(&self) -> &'static str {
        match self {
            Self::Starting => "yellow",
            Self::Running => "green",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Starting => "Starting",
            Self::Running => "Running",
        };
        write!(f, "{s}")
    }
}
