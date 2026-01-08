//! Data migrations between Tenex versions.

use crate::config::Config;
use crate::paths;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use tracing::info;

/// Migrate Tenex's default state directory from the legacy XDG data location
/// (for example `~/.local/share/tenex/`) to `~/.tenex/`.
///
/// This migration only applies when `TENEX_STATE_PATH` is not set.
///
/// # Errors
///
/// Returns an error if the migration is needed but file operations fail.
pub fn migrate_default_state_dir() -> Result<()> {
    if std::env::var_os("TENEX_STATE_PATH").is_some() {
        return Ok(());
    }

    let Some(old_root) = paths::data_local_dir() else {
        return Ok(());
    };
    let old_dir = old_root.join("tenex");

    let new_dir = Config::default_instance_root();

    if !migrate_state_dir(&old_dir, &new_dir)? {
        return Ok(());
    }

    info!(
        old_dir = %old_dir.display(),
        new_dir = %new_dir.display(),
        "Migrated Tenex state directory"
    );

    Ok(())
}

fn migrate_state_dir(old_dir: &Path, new_dir: &Path) -> Result<bool> {
    if !old_dir.exists() {
        return Ok(false);
    }

    let mut to_move = Vec::new();
    for name in ["state.json", "settings.json"] {
        let src = old_dir.join(name);
        if !src.exists() {
            continue;
        }

        let dst = new_dir.join(name);
        if dst.exists() {
            continue;
        }

        to_move.push((src, dst));
    }

    if let Ok(entries) = fs::read_dir(old_dir) {
        for entry in entries.flatten() {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }

            let Some(file_name) = src.file_name() else {
                continue;
            };
            let Some(file_name_str) = file_name.to_str() else {
                continue;
            };

            if !Path::new(file_name_str)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"))
            {
                continue;
            }

            let dst = new_dir.join(file_name);
            if dst.exists() {
                continue;
            }

            to_move.push((src, dst));
        }
    }

    if to_move.is_empty() {
        return Ok(false);
    }

    fs::create_dir_all(new_dir)
        .with_context(|| format!("Failed to create {}", new_dir.display()))?;

    for (src, dst) in &to_move {
        move_file(src, dst)
            .with_context(|| format!("Failed to move {} to {}", src.display(), dst.display()))?;
    }

    // Only delete the old directory if it is empty after migration.
    if let Ok(mut entries) = fs::read_dir(old_dir)
        && entries.next().is_none()
    {
        fs::remove_dir_all(old_dir).with_context(|| {
            format!("Failed to remove old state directory {}", old_dir.display())
        })?;
    }

    Ok(true)
}

fn move_file(src: &Path, dst: &Path) -> Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(err) if is_cross_device_link_error(&err) => {
            fs::copy(src, dst).with_context(|| {
                format!("Failed to copy {} to {}", src.display(), dst.display())
            })?;
            fs::remove_file(src).with_context(|| format!("Failed to remove {}", src.display()))?;
            Ok(())
        }
        Err(err) => Err(err).with_context(|| format!("Failed to rename {}", src.display())),
    }
}

fn is_cross_device_link_error(err: &std::io::Error) -> bool {
    // EXDEV indicates an invalid cross-device rename.
    err.raw_os_error() == Some(18)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_file_accepts_missing_source() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let src = tmp.path().join("missing.json");
        let dst = tmp.path().join("dst.json");
        assert!(move_file(&src, &dst).is_err());
        Ok(())
    }

    #[test]
    fn test_migrate_state_dir_moves_settings_without_state() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");

        fs::create_dir_all(&old_dir)
            .with_context(|| format!("Failed to create {}", old_dir.display()))?;

        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .with_context(|| "Failed to write legacy settings")?;

        assert!(migrate_state_dir(&old_dir, &new_dir)?);
        assert!(new_dir.join("settings.json").exists());
        assert!(!old_dir.exists());
        Ok(())
    }

    #[test]
    fn test_migrate_state_dir_does_not_overwrite_existing_settings() -> Result<()> {
        let tmp = tempfile::TempDir::new()?;
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");

        fs::create_dir_all(&old_dir)
            .with_context(|| format!("Failed to create {}", old_dir.display()))?;
        fs::create_dir_all(&new_dir)
            .with_context(|| format!("Failed to create {}", new_dir.display()))?;

        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .with_context(|| "Failed to write legacy settings")?;
        fs::write(
            new_dir.join("settings.json"),
            r#"{"agent_program":"claude"}"#,
        )
        .with_context(|| "Failed to write current settings")?;

        assert!(!migrate_state_dir(&old_dir, &new_dir)?);

        let settings = fs::read_to_string(new_dir.join("settings.json"))
            .with_context(|| "Failed to read current settings")?;
        assert!(settings.contains("claude"));
        Ok(())
    }
}
