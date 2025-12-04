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
    /// Agent is paused (work committed, resources freed)
    Paused,
    /// Agent has stopped or been terminated
    Stopped,
}

impl Status {
    /// Check if the agent is in an active state (can receive input)
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the agent can be resumed
    #[must_use]
    pub const fn can_resume(&self) -> bool {
        matches!(self, Self::Paused)
    }

    /// Check if the agent can be paused
    #[must_use]
    pub const fn can_pause(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the agent can be killed
    #[must_use]
    pub const fn can_kill(&self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Paused)
    }

    /// Get the display symbol for the status
    #[must_use]
    pub const fn symbol(&self) -> &'static str {
        match self {
            Self::Starting => "...",
            Self::Running => ">>>",
            Self::Paused => "||",
            Self::Stopped => "X",
        }
    }

    /// Get the display color name for the status
    #[must_use]
    pub const fn color_name(&self) -> &'static str {
        match self {
            Self::Starting => "yellow",
            Self::Running => "green",
            Self::Paused => "blue",
            Self::Stopped => "red",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "test assertions")]
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
        assert!(!Status::Paused.is_active());
        assert!(!Status::Stopped.is_active());
    }

    #[test]
    fn test_can_resume() {
        assert!(!Status::Starting.can_resume());
        assert!(!Status::Running.can_resume());
        assert!(Status::Paused.can_resume());
        assert!(!Status::Stopped.can_resume());
    }

    #[test]
    fn test_can_pause() {
        assert!(!Status::Starting.can_pause());
        assert!(Status::Running.can_pause());
        assert!(!Status::Paused.can_pause());
        assert!(!Status::Stopped.can_pause());
    }

    #[test]
    fn test_can_kill() {
        assert!(Status::Starting.can_kill());
        assert!(Status::Running.can_kill());
        assert!(Status::Paused.can_kill());
        assert!(!Status::Stopped.can_kill());
    }

    #[test]
    fn test_symbol() {
        assert_eq!(Status::Starting.symbol(), "...");
        assert_eq!(Status::Running.symbol(), ">>>");
        assert_eq!(Status::Paused.symbol(), "||");
        assert_eq!(Status::Stopped.symbol(), "X");
    }

    #[test]
    fn test_color_name() {
        assert_eq!(Status::Starting.color_name(), "yellow");
        assert_eq!(Status::Running.color_name(), "green");
        assert_eq!(Status::Paused.color_name(), "blue");
        assert_eq!(Status::Stopped.color_name(), "red");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Status::Starting), "Starting");
        assert_eq!(format!("{}", Status::Running), "Running");
        assert_eq!(format!("{}", Status::Paused), "Paused");
        assert_eq!(format!("{}", Status::Stopped), "Stopped");
    }

    #[test]
    fn test_serde_roundtrip() {
        for status in [
            Status::Starting,
            Status::Running,
            Status::Paused,
            Status::Stopped,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let parsed: Status = serde_json::from_str(&json).unwrap();
            assert_eq!(status, parsed);
        }
    }

    #[test]
    fn test_serde_format() {
        let json = serde_json::to_string(&Status::Running).unwrap();
        assert_eq!(json, "\"running\"");
    }
}
