use anyhow::{Result, anyhow};
use semver::Version;
use tenex::release_notes::test_support as release_support;
use tenex::release_notes::{
    ReleaseNoteEntry, changelog_lines_for_version, current_version, note_for, notes_between,
    whats_new_lines,
};
use tenex::test_support::lock_env_test_environment;

static DATED_OVERRIDE: &[ReleaseNoteEntry] = &[ReleaseNoteEntry {
    version: "1.2.3",
    date: Some("2026-01-19"),
    body: "Test body",
}];

static UNDATED_OVERRIDE: &[ReleaseNoteEntry] = &[ReleaseNoteEntry {
    version: "1.2.3",
    date: None,
    body: "Test body",
}];

static INVALID_VERSION_OVERRIDE: &[ReleaseNoteEntry] = &[ReleaseNoteEntry {
    version: "not-a-version",
    date: None,
    body: "",
}];

static RANGE_OVERRIDE: &[ReleaseNoteEntry] = &[
    ReleaseNoteEntry {
        version: "3.0.0",
        date: Some("2026-03-01"),
        body: "Future body",
    },
    ReleaseNoteEntry {
        version: "2.0.0",
        date: Some("2026-02-01"),
        body: "Current body",
    },
    ReleaseNoteEntry {
        version: "1.5.0",
        date: None,
        body: "Undated body",
    },
    ReleaseNoteEntry {
        version: "1.0.0",
        date: Some("2026-01-01"),
        body: "Previous body",
    },
];

fn error_message<T>(result: anyhow::Result<T>, context: &str) -> Result<String> {
    match result {
        Ok(_) => anyhow::bail!("{context}"),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn test_parse_release_note_version_reports_context_for_invalid_entry() -> Result<()> {
    let err = error_message(
        release_support::parse_release_note_version("not-a-version"),
        "expected invalid version error",
    )?;
    assert!(err.contains("Invalid embedded release notes version: not-a-version"));
    Ok(())
}

#[test]
fn test_append_release_note_body_inserts_no_details_for_empty_body() {
    let mut lines = Vec::new();
    release_support::append_release_note_body(&mut lines, "\n\n");
    assert_eq!(lines, vec!["(No details.)".to_string()]);
}

#[test]
fn test_note_for_current_version_exists() -> Result<()> {
    let current = current_version()?;
    let note = note_for(&current)?;
    assert!(note.is_some());
    Ok(())
}

#[test]
fn test_whats_new_lines_empty_range_returns_message() -> Result<()> {
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
fn test_changelog_lines_for_unknown_version_has_fallback() -> Result<()> {
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
fn test_notes_between_respects_bounds() -> Result<()> {
    let current = current_version()?;
    let all_notes = notes_between(None, &current)?;
    let oldest = all_notes
        .iter()
        .map(|note| note.version.clone())
        .min()
        .ok_or_else(|| anyhow!("embedded release notes missing"))?;

    let only_oldest = notes_between(None, &oldest)?;
    assert!(only_oldest.iter().all(|note| note.version <= oldest));

    let after_oldest = notes_between(Some(&oldest), &current)?;
    assert!(after_oldest.iter().all(|note| note.version > oldest));
    Ok(())
}

#[test]
fn test_notes_between_filters_future_and_includes_current_and_undated_notes() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(RANGE_OVERRIDE);

    let from_version = Version::new(1, 0, 0);
    let to_version = Version::new(2, 0, 0);
    let notes = notes_between(Some(&from_version), &to_version)?;

    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].version, Version::new(2, 0, 0));
    assert_eq!(notes[0].date, Some("2026-02-01"));
    assert_eq!(notes[0].body, "Current body");
    assert_eq!(notes[1].version, Version::new(1, 5, 0));
    assert_eq!(notes[1].date, None);
    assert_eq!(notes[1].body, "Undated body");
    assert!(
        notes
            .iter()
            .all(|note| note.version != Version::new(3, 0, 0))
    );
    assert!(
        notes
            .iter()
            .all(|note| note.version != Version::new(1, 0, 0))
    );
    Ok(())
}

#[test]
fn test_whats_new_lines_includes_footer() -> Result<()> {
    let current = current_version()?;
    let lines = whats_new_lines(None, &current)?;
    assert!(lines.iter().any(|line| line == "Esc closes"));
    Ok(())
}

#[test]
fn test_whats_new_lines_formats_current_and_undated_override_notes() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(RANGE_OVERRIDE);

    let from_version = Version::new(1, 0, 0);
    let to_version = Version::new(2, 0, 0);
    let lines = whats_new_lines(Some(&from_version), &to_version)?;

    assert!(lines.iter().any(|line| line == "v2.0.0 (2026-02-01)"));
    assert!(lines.iter().any(|line| line == "Current body"));
    assert!(lines.iter().any(|line| line == "v1.5.0"));
    assert!(!lines.iter().any(|line| line == "v1.5.0 ()"));
    assert!(lines.iter().any(|line| line == "Undated body"));
    assert!(!lines.iter().any(|line| line.contains("Future body")));
    Ok(())
}

#[test]
fn test_whats_new_lines_omits_date_suffix_when_missing() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(UNDATED_OVERRIDE);

    let version = Version::new(1, 2, 3);
    let lines = whats_new_lines(None, &version)?;
    assert!(lines.iter().any(|line| line == "v1.2.3"));
    assert!(!lines.iter().any(|line| line == "v1.2.3 ()"));
    Ok(())
}

#[test]
fn test_changelog_lines_for_current_version_includes_title() -> Result<()> {
    let current = current_version()?;
    let lines = changelog_lines_for_version(&current)?;
    assert!(
        lines
            .iter()
            .any(|line| line == &format!("Tenex v{current}"))
    );
    Ok(())
}

#[test]
fn test_changelog_lines_for_missing_override_version_has_fallback() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(RANGE_OVERRIDE);

    let version = Version::new(9, 9, 9);
    let lines = changelog_lines_for_version(&version)?;
    assert!(
        lines
            .iter()
            .any(|line| line == "No release notes available for this version.")
    );
    Ok(())
}

#[test]
fn test_changelog_lines_for_version_includes_date_suffix() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(DATED_OVERRIDE);

    let version = Version::new(1, 2, 3);
    let lines = changelog_lines_for_version(&version)?;
    assert!(lines.iter().any(|line| line == "Released: 2026-01-19"));
    assert!(lines.iter().any(|line| line == "Test body"));
    Ok(())
}

#[test]
fn test_changelog_lines_for_version_omits_date_when_missing() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(UNDATED_OVERRIDE);

    let version = Version::new(1, 2, 3);
    let lines = changelog_lines_for_version(&version)?;
    assert!(!lines.iter().any(|line| line.starts_with("Released: ")));
    assert!(lines.iter().any(|line| line == "Test body"));
    Ok(())
}

#[test]
fn test_release_notes_override_restores_previous_override() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _outer_override = release_support::set_release_notes_override(DATED_OVERRIDE);

    {
        let _inner_override = release_support::set_release_notes_override(UNDATED_OVERRIDE);
        let version = Version::new(1, 2, 3);
        let note = note_for(&version)?.ok_or_else(|| anyhow!("expected override note"))?;
        assert_eq!(note.date, None);
    }

    let version = Version::new(1, 2, 3);
    let note = note_for(&version)?.ok_or_else(|| anyhow!("expected restored note"))?;
    assert_eq!(note.date, Some("2026-01-19"));
    Ok(())
}

#[test]
fn test_notes_between_propagates_invalid_embedded_versions() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(INVALID_VERSION_OVERRIDE);

    let version = Version::new(1, 0, 0);
    let message = error_message(
        notes_between(None, &version),
        "expected invalid release-note version error",
    )?;
    assert!(message.contains("Invalid embedded release notes version: not-a-version"));
    Ok(())
}

#[test]
fn test_whats_new_lines_propagates_release_note_parse_errors() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(INVALID_VERSION_OVERRIDE);

    let version = Version::new(1, 0, 0);
    let message = error_message(
        whats_new_lines(None, &version),
        "expected invalid release-note version error",
    )?;
    assert!(message.contains("Invalid embedded release notes version: not-a-version"));
    Ok(())
}

#[test]
fn test_changelog_lines_for_version_propagates_release_note_parse_errors() -> Result<()> {
    let _guard = lock_env_test_environment();
    let _override = release_support::set_release_notes_override(INVALID_VERSION_OVERRIDE);

    let version = Version::new(1, 0, 0);
    let message = error_message(
        changelog_lines_for_version(&version),
        "expected invalid release-note version error",
    )?;
    assert!(message.contains("Invalid embedded release notes version: not-a-version"));
    Ok(())
}
