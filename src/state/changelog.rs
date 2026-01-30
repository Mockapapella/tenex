//! Changelog / "What's New" mode state type (new architecture).

use semver::Version;

/// Changelog mode - displays release notes in a scrollable modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogMode {
    /// Modal title.
    pub title: String,
    /// Content lines (rendered top-to-bottom).
    pub lines: Vec<String>,
    /// If set, mark this version as "seen" when the modal is dismissed.
    pub mark_seen_version: Option<Version>,
}
