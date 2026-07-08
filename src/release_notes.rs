//! Embedded release notes derived from `CHANGELOG.md`.
//!
//! Tenex displays "What's New" on first run after an upgrade. To avoid parsing
//! markdown at runtime, a build script extracts per-version sections from
//! `CHANGELOG.md` and embeds them in the binary.

use anyhow::{Context, Result};
use semver::Version;

/// A single release note entry extracted from `CHANGELOG.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseNoteEntry {
    /// Version string (for example, `1.0.7`).
    pub version: &'static str,
    /// Optional release date string (for example, `2026-01-19`).
    pub date: Option<&'static str>,
    /// Changelog body for the version (markdown-ish text).
    pub body: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/release_notes.rs"));

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static RELEASE_NOTES_OVERRIDE: std::cell::RefCell<Option<&'static [ReleaseNoteEntry]>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(not(any(test, feature = "test-support")))]
const fn release_note_entries() -> &'static [ReleaseNoteEntry] {
    RELEASE_NOTES
}

#[cfg(any(test, feature = "test-support"))]
fn release_note_entries() -> &'static [ReleaseNoteEntry] {
    if let Some(entries) =
        RELEASE_NOTES_OVERRIDE.with(|override_entries| *override_entries.borrow())
    {
        return entries;
    }

    RELEASE_NOTES
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
struct ReleaseNotesOverrideRestore(Option<&'static [ReleaseNoteEntry]>);

#[cfg(any(test, feature = "test-support"))]
impl Drop for ReleaseNotesOverrideRestore {
    fn drop(&mut self) {
        let previous = self.0.take();
        RELEASE_NOTES_OVERRIDE.with(|override_entries| {
            *override_entries.borrow_mut() = previous;
        });
    }
}

#[cfg(any(test, feature = "test-support"))]
fn set_release_notes_override_for_tests(
    entries: &'static [ReleaseNoteEntry],
) -> ReleaseNotesOverrideRestore {
    let previous = RELEASE_NOTES_OVERRIDE
        .with(|override_entries| (*override_entries.borrow_mut()).replace(entries));
    ReleaseNotesOverrideRestore(previous)
}

#[cfg(test)]
/// Run a closure with temporary embedded release notes.
pub(crate) fn with_release_notes_override_for_tests<T>(
    entries: &'static [ReleaseNoteEntry],
    f: impl FnOnce() -> T,
) -> T {
    let _restore = set_release_notes_override_for_tests(entries);
    f()
}

/// A parsed release note entry with a semver version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNote {
    /// Parsed semver version.
    pub version: Version,
    /// Optional release date string.
    pub date: Option<&'static str>,
    /// Changelog body for the version.
    pub body: &'static str,
}

/// Parse the running Tenex version (`CARGO_PKG_VERSION`).
///
/// # Errors
///
/// Returns an error if the embedded version is not valid semver.
pub fn current_version() -> Result<Version> {
    Version::parse(env!("CARGO_PKG_VERSION")).context("Failed to parse current Tenex version")
}

fn parse_release_note_version(value: &str) -> Result<Version> {
    Version::parse(value)
        .with_context(|| format!("Invalid embedded release notes version: {value}"))
}

fn append_release_note_body(lines: &mut Vec<String>, body: &str) {
    let body = body.trim_matches('\n');
    if body.is_empty() {
        lines.push("(No details.)".to_string());
    } else {
        lines.extend(body.lines().map(ToString::to_string));
    }
}

/// Load release notes for versions in `(from_exclusive, to_inclusive]`.
///
/// Release notes are returned in the same order as `CHANGELOG.md` (newest first).
///
/// # Errors
///
/// Returns an error if an embedded release note has an invalid version string.
pub fn notes_between(
    from_exclusive: Option<&Version>,
    to_inclusive: &Version,
) -> Result<Vec<ReleaseNote>> {
    let mut out = Vec::new();

    for entry in release_note_entries() {
        let version = parse_release_note_version(entry.version)?;

        if version > *to_inclusive {
            continue;
        }
        if matches!(from_exclusive, Some(from) if version <= *from) {
            continue;
        }

        out.push(ReleaseNote {
            version,
            date: entry.date,
            body: entry.body,
        });
    }

    Ok(out)
}

/// Load release notes for a single version.
///
/// # Errors
///
/// Returns an error if an embedded release note has an invalid version string.
pub fn note_for(version: &Version) -> Result<Option<ReleaseNote>> {
    for entry in release_note_entries() {
        let parsed = parse_release_note_version(entry.version)?;

        if &parsed == version {
            return Ok(Some(ReleaseNote {
                version: parsed,
                date: entry.date,
                body: entry.body,
            }));
        }
    }

    Ok(None)
}

/// Build display lines for a "What's New" view for versions in `(from_exclusive, to_inclusive]`.
///
/// # Errors
///
/// Returns an error if release notes cannot be loaded.
pub fn whats_new_lines(
    from_exclusive: Option<&Version>,
    to_inclusive: &Version,
) -> Result<Vec<String>> {
    let notes = notes_between(from_exclusive, to_inclusive)?;

    let mut lines = Vec::new();
    lines.push(format!("What's New in Tenex v{to_inclusive}"));
    lines.push(String::new());

    if notes.is_empty() {
        lines.push("No release notes available.".to_string());
        return Ok(lines);
    }

    for note in notes {
        let date_suffix = if note.date.is_none() {
            String::new()
        } else {
            format!(" ({})", note.date.unwrap_or_default())
        };
        lines.push(format!("v{}{date_suffix}", note.version));
        lines.push(String::new());

        append_release_note_body(&mut lines, note.body);

        lines.push(String::new());
    }

    lines.push("Scroll: ↑/↓, PgUp/PgDn, Ctrl+u/d, g/G".to_string());
    lines.push("Esc closes".to_string());

    Ok(lines)
}

/// Build display lines for the changelog entry of a single version.
///
/// # Errors
///
/// Returns an error if the embedded release notes cannot be loaded.
pub fn changelog_lines_for_version(version: &Version) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    lines.push(format!("Tenex v{version}"));
    lines.push(String::new());

    let Some(note) = note_for(version)? else {
        lines.push("No release notes available for this version.".to_string());
        lines.push(String::new());
        lines.push("Esc closes".to_string());
        return Ok(lines);
    };

    if let Some(date) = note.date {
        lines.push(format!("Released: {date}"));
        lines.push(String::new());
    }

    append_release_note_body(&mut lines, note.body);

    lines.push(String::new());
    lines.push("Scroll: ↑/↓, PgUp/PgDn, Ctrl+u/d, g/G".to_string());
    lines.push("Esc closes".to_string());

    Ok(lines)
}

#[cfg(all(feature = "test-support", not(test)))]
/// Integration-test helpers for otherwise private release-note logic.
pub mod test_support {
    use anyhow::Result;
    use semver::Version;

    /// Parse an embedded release-note version string.
    ///
    /// # Errors
    ///
    /// Returns an error if the version is not valid semver.
    pub fn parse_release_note_version(value: &str) -> Result<Version> {
        super::parse_release_note_version(value)
    }

    /// Append a release-note body to display lines.
    pub fn append_release_note_body(lines: &mut Vec<String>, body: &str) {
        super::append_release_note_body(lines, body);
    }

    /// Restores a previous release-notes override when dropped.
    #[must_use]
    #[derive(Debug)]
    pub struct ReleaseNotesOverrideGuard {
        _restore: super::ReleaseNotesOverrideRestore,
    }

    /// Set temporary embedded release notes until the returned guard is dropped.
    pub fn set_release_notes_override(
        entries: &'static [super::ReleaseNoteEntry],
    ) -> ReleaseNotesOverrideGuard {
        ReleaseNotesOverrideGuard {
            _restore: super::set_release_notes_override_for_tests(entries),
        }
    }
}
