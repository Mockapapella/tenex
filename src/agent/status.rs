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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_status() {
        let status = Status::default();
        assert_eq!(status, Status::Starting);
    }

    #[test]
    fn test_is_active() {
        assert!(!Status::Starting.is_active());
        assert!(Status::Running.is_active());
    }

    #[test]
    fn test_can_kill() {
        assert!(Status::Starting.can_kill());
        assert!(Status::Running.can_kill());
    }

    #[test]
    fn test_symbol() {
        assert_eq!(Status::Starting.symbol(), "...");
        assert_eq!(Status::Running.symbol(), "●");
    }

    #[test]
    fn test_color_name() {
        assert_eq!(Status::Starting.color_name(), "yellow");
        assert_eq!(Status::Running.color_name(), "green");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Status::Starting), "Starting");
        assert_eq!(format!("{}", Status::Running), "Running");
    }

    #[test]
    fn test_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        for status in [Status::Starting, Status::Running] {
            let json = serde_json::to_string(&status)?;
            let parsed: Status = serde_json::from_str(&json)?;
            assert_eq!(status, parsed);
        }
        Ok(())
    }

    #[test]
    fn test_serde_format() -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string(&Status::Running)?;
        assert_eq!(json, "\"running\"");
        Ok(())
    }
}
