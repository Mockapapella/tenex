//! Data migrations between Tenex versions.

use crate::config::Config;
use crate::paths;
use anyhow::{Context, Result};
use std::ffi::OsStr;
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

            let file_name = entry.file_name();
            if !is_migratable_bak_file_name(&file_name) {
                continue;
            }

            let dst = new_dir.join(&file_name);
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

fn is_migratable_bak_file_name(file_name: &OsStr) -> bool {
    let Some(file_name_str) = file_name.to_str() else {
        return false;
    };

    Path::new(file_name_str)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"))
}

fn move_file(src: &Path, dst: &Path) -> Result<()> {
    move_file_with_ops(src, dst, rename_file, copy_file, remove_file)
}

fn rename_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::rename(src, dst)
}

fn copy_file(src: &Path, dst: &Path) -> std::io::Result<u64> {
    fs::copy(src, dst)
}

fn remove_file(src: &Path) -> std::io::Result<()> {
    fs::remove_file(src)
}

fn move_file_with_ops(
    src: &Path,
    dst: &Path,
    rename: fn(&Path, &Path) -> std::io::Result<()>,
    copy: fn(&Path, &Path) -> std::io::Result<u64>,
    remove_file: fn(&Path) -> std::io::Result<()>,
) -> Result<()> {
    match rename(src, dst) {
        Ok(()) => Ok(()),
        Err(err) if is_cross_device_link_error(&err) => {
            copy(src, dst).with_context(|| {
                format!("Failed to copy {} to {}", src.display(), dst.display())
            })?;
            remove_file(src).with_context(|| format!("Failed to remove {}", src.display()))?;
            Ok(())
        }
        Err(err) => Err(err).with_context(|| format!("Failed to rename {}", src.display())),
    }
}

fn is_cross_device_link_error(err: &std::io::Error) -> bool {
    // EXDEV indicates an invalid cross-device rename.
    err.raw_os_error() == Some(18)
}

#[cfg(any(test, feature = "test-support"))]
/// Integration-test helpers for otherwise private migration primitives.
pub mod test_support {
    use anyhow::Result;
    use std::ffi::OsStr;
    use std::path::Path;

    /// Migrate state files between two injected directories.
    ///
    /// # Errors
    ///
    /// Returns an error if any filesystem operation needed for migration fails.
    pub fn migrate_state_dir(old_dir: &Path, new_dir: &Path) -> Result<bool> {
        super::migrate_state_dir(old_dir, new_dir)
    }

    /// Move a file using the production migration move logic.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be renamed or copied and removed.
    pub fn move_file(src: &Path, dst: &Path) -> Result<()> {
        super::move_file(src, dst)
    }

    /// Copy a file with the production migration copy helper.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem copy fails.
    pub fn copy_file(src: &Path, dst: &Path) -> std::io::Result<u64> {
        super::copy_file(src, dst)
    }

    /// Remove a file with the production migration remove helper.
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem removal fails.
    pub fn remove_file(src: &Path) -> std::io::Result<()> {
        super::remove_file(src)
    }

    /// Move a file using injected filesystem operations.
    ///
    /// # Errors
    ///
    /// Returns an error if the injected operations cannot complete the move.
    pub fn move_file_with_ops(
        src: &Path,
        dst: &Path,
        rename: fn(&Path, &Path) -> std::io::Result<()>,
        copy: fn(&Path, &Path) -> std::io::Result<u64>,
        remove_file: fn(&Path) -> std::io::Result<()>,
    ) -> Result<()> {
        super::move_file_with_ops(src, dst, rename, copy, remove_file)
    }

    /// Return whether a file name is a migratable backup file.
    #[must_use]
    pub fn is_migratable_bak_file_name(file_name: &OsStr) -> bool {
        super::is_migratable_bak_file_name(file_name)
    }
}
