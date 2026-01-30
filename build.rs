//! Build script for Tenex.
//!
//! Generates a compile-time release notes index from `CHANGELOG.md` so the
//! application can display "What's New" without parsing markdown at runtime.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct ChangelogEntry {
    version: String,
    date: Option<String>,
    body: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=CHANGELOG.md");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let current_version = env::var("CARGO_PKG_VERSION")?;

    let changelog_path = manifest_dir.join("CHANGELOG.md");
    let changelog = fs::read_to_string(&changelog_path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Failed to read {}: {e}", changelog_path.display()),
        )
    })?;

    let entries = parse_changelog_entries(&changelog);
    assert_changelog_contains_version(&entries, &current_version, &changelog_path)?;

    let out_path = out_dir.join("release_notes.rs");
    let contents = generate_release_notes_rs(&entries)?;
    fs::write(&out_path, contents).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Failed to write {}: {e}", out_path.display()),
        )
    })?;

    Ok(())
}

fn assert_changelog_contains_version(
    entries: &[ChangelogEntry],
    version: &str,
    changelog_path: &Path,
) -> io::Result<()> {
    let mut seen: HashSet<&str> = HashSet::new();
    for entry in entries {
        if !seen.insert(entry.version.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Duplicate changelog section for version {} in {}",
                    entry.version,
                    changelog_path.display()
                ),
            ));
        }
    }

    if !seen.contains(version) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Missing changelog section for version {} in {}",
                version,
                changelog_path.display()
            ),
        ));
    }

    Ok(())
}

fn parse_changelog_entries(changelog: &str) -> Vec<ChangelogEntry> {
    let mut entries: Vec<ChangelogEntry> = Vec::new();
    let mut current_version: Option<(String, Option<String>)> = None;
    let mut body_lines: Vec<String> = Vec::new();

    for line in changelog.lines() {
        if let Some((version, date)) = parse_version_heading(line) {
            if let Some((prev_version, prev_date)) = current_version.take() {
                entries.push(ChangelogEntry {
                    version: prev_version,
                    date: prev_date,
                    body: join_and_trim_lines(&body_lines),
                });
                body_lines.clear();
            }
            current_version = Some((version, date));
            continue;
        }

        if current_version.is_some() {
            body_lines.push(line.to_string());
        }
    }

    if let Some((version, date)) = current_version.take() {
        entries.push(ChangelogEntry {
            version,
            date,
            body: join_and_trim_lines(&body_lines),
        });
    }

    entries
}

fn join_and_trim_lines(lines: &[String]) -> String {
    let joined = lines.join("\n");
    joined.trim_matches('\n').to_string()
}

fn parse_version_heading(line: &str) -> Option<(String, Option<String>)> {
    let trimmed = line.trim_end();
    if !trimmed.starts_with("## [") {
        return None;
    }

    let after_prefix = &trimmed[4..];
    let end_bracket = after_prefix.find(']')?;
    let version = &after_prefix[..end_bracket];
    if !is_simple_semver(version) {
        return None;
    }

    let mut date: Option<String> = None;
    let rest = after_prefix[end_bracket + 1..].trim();
    if let Some(after_dash) = rest.strip_prefix('-') {
        let value = after_dash.trim();
        if !value.is_empty() {
            date = Some(value.to_string());
        }
    }

    Some((version.to_string(), date))
}

fn is_simple_semver(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    [major, minor, patch]
        .into_iter()
        .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn generate_release_notes_rs(entries: &[ChangelogEntry]) -> io::Result<String> {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(&mut out, "// @generated").map_err(io::Error::other)?;
    writeln!(&mut out, "//").map_err(io::Error::other)?;
    writeln!(&mut out, "// Generated from CHANGELOG.md by build.rs.").map_err(io::Error::other)?;
    writeln!(&mut out).map_err(io::Error::other)?;
    writeln!(&mut out, "const RELEASE_NOTES: &[ReleaseNoteEntry] = &[")
        .map_err(io::Error::other)?;
    for entry in entries {
        let version = raw_string_literal(&entry.version)?;
        let body = raw_string_literal(&entry.body)?;
        let date = entry.date.as_deref().map_or_else(
            || Ok("None".to_string()),
            |value| raw_string_literal(value).map(|literal| format!("Some({literal})")),
        )?;

        writeln!(&mut out, "    ReleaseNoteEntry {{").map_err(io::Error::other)?;
        writeln!(&mut out, "        version: {version},").map_err(io::Error::other)?;
        writeln!(&mut out, "        date: {date},").map_err(io::Error::other)?;
        writeln!(&mut out, "        body: {body},").map_err(io::Error::other)?;
        writeln!(&mut out, "    }},").map_err(io::Error::other)?;
    }
    writeln!(&mut out, "];").map_err(io::Error::other)?;
    Ok(out)
}

fn raw_string_literal(value: &str) -> io::Result<String> {
    for hashes in 0..=10 {
        let hash_str = "#".repeat(hashes);
        let closing = format!("\"{hash_str}");
        if !value.contains(&closing) {
            return Ok(format!("r{hash_str}\"{value}\"{hash_str}"));
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Failed to generate raw string literal",
    ))
}
