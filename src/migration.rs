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

    let old_state = old_dir.join("state.json");
    let new_state = new_dir.join("state.json");

    if !old_state.exists() || new_state.exists() {
        return Ok(());
    }

    fs::create_dir_all(&new_dir)
        .with_context(|| format!("Failed to create {}", new_dir.display()))?;

    let mut to_move = Vec::new();
    for name in ["state.json", "settings.json"] {
        let path = old_dir.join(name);
        if path.exists() {
            to_move.push(path);
        }
    }

    if let Ok(entries) = fs::read_dir(&old_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        Path::new(name)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"))
                    })
            {
                to_move.push(path);
            }
        }
    }

    for src in &to_move {
        let Some(file_name) = src.file_name() else {
            continue;
        };

        let dst = new_dir.join(file_name);
        if dst.exists() {
            continue;
        }

        move_file(src, &dst)
            .with_context(|| format!("Failed to move {} to {}", src.display(), dst.display()))?;
    }

    // Only delete the old directory if it is empty after migration.
    if let Ok(mut entries) = fs::read_dir(&old_dir)
        && entries.next().is_none()
    {
        fs::remove_dir_all(&old_dir).with_context(|| {
            format!("Failed to remove old state directory {}", old_dir.display())
        })?;
    }

    info!(
        old_dir = %old_dir.display(),
        new_dir = %new_dir.display(),
        "Migrated Tenex state directory"
    );

    Ok(())
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
    #[cfg(unix)]
    {
        // On Unix platforms, EXDEV indicates an invalid cross-device rename.
        err.raw_os_error() == Some(18)
    }

    #[cfg(not(unix))]
    {
        let _ = err;
        false
    }
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
}
