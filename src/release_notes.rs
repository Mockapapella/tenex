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

    for entry in RELEASE_NOTES {
        let version = Version::parse(entry.version).with_context(|| {
            format!("Invalid embedded release notes version: {}", entry.version)
        })?;

        if version > *to_inclusive {
            continue;
        }
        if from_exclusive.is_some_and(|from| version <= *from) {
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
    for entry in RELEASE_NOTES {
        let parsed = Version::parse(entry.version).with_context(|| {
            format!("Invalid embedded release notes version: {}", entry.version)
        })?;

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
        let date_suffix = note.date.map_or_else(String::new, |d| format!(" ({d})"));
        lines.push(format!("v{}{date_suffix}", note.version));
        lines.push(String::new());

        let body = note.body.trim_matches('\n');
        if body.is_empty() {
            lines.push("(No details.)".to_string());
        } else {
            lines.extend(body.lines().map(ToString::to_string));
        }

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

    let body = note.body.trim_matches('\n');
    if body.is_empty() {
        lines.push("(No details.)".to_string());
    } else {
        lines.extend(body.lines().map(ToString::to_string));
    }

    lines.push(String::new());
    lines.push("Scroll: ↑/↓, PgUp/PgDn, Ctrl+u/d, g/G".to_string());
    lines.push("Esc closes".to_string());

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_note_for_current_version_exists() -> Result<(), Box<dyn std::error::Error>> {
        let current = current_version()?;
        let note = note_for(&current)?;
        assert!(note.is_some());
        Ok(())
    }

    #[test]
    fn test_whats_new_lines_empty_range_returns_message() -> Result<(), Box<dyn std::error::Error>>
    {
        let current = current_version()?;
        let lines = whats_new_lines(Some(&current), &current)?;
        assert!(
            lines
                .iter()
                .any(|line| line == "No release notes available.")
        );
        Ok(())
    }

    #[test]
    fn test_changelog_lines_for_unknown_version_has_fallback()
    -> Result<(), Box<dyn std::error::Error>> {
        let unknown = Version::new(0, 0, 0);
        let lines = changelog_lines_for_version(&unknown)?;
        assert!(
            lines
                .iter()
                .any(|line| line == "No release notes available for this version.")
        );
        Ok(())
    }

    #[test]
    fn test_notes_between_respects_bounds() -> Result<(), Box<dyn std::error::Error>> {
        let current = current_version()?;

        let mut versions = Vec::new();
        for entry in RELEASE_NOTES {
            versions.push(Version::parse(entry.version)?);
        }

        let Some(oldest) = versions.iter().min().cloned() else {
            return Ok(());
        };

        let only_oldest = notes_between(None, &oldest)?;
        assert!(only_oldest.iter().all(|note| note.version <= oldest));

        let after_oldest = notes_between(Some(&oldest), &current)?;
        assert!(after_oldest.iter().all(|note| note.version > oldest));

        Ok(())
    }

    #[test]
    fn test_whats_new_lines_includes_footer() -> Result<(), Box<dyn std::error::Error>> {
        let current = current_version()?;
        let lines = whats_new_lines(None, &current)?;
        assert!(lines.iter().any(|line| line == "Esc closes"));
        Ok(())
    }

    #[test]
    fn test_changelog_lines_for_current_version_includes_title()
    -> Result<(), Box<dyn std::error::Error>> {
        let current = current_version()?;
        let lines = changelog_lines_for_version(&current)?;
        assert!(
            lines
                .iter()
                .any(|line| line == &format!("Tenex v{current}"))
        );
        Ok(())
    }
}
