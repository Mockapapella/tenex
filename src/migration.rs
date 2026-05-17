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

#[cfg(test)]
mod tests {
    use super::*;

    fn rename_cross_device(_src: &Path, _dst: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from_raw_os_error(18))
    }

    fn rename_failed(_src: &Path, _dst: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("rename failed"))
    }

    fn copy_failed(_src: &Path, _dst: &Path) -> std::io::Result<u64> {
        Err(std::io::Error::other("copy failed"))
    }

    fn remove_failed(_src: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("remove failed"))
    }

    #[test]
    fn test_move_file_accepts_missing_source() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let src = tmp.path().join("missing.json");
        let dst = tmp.path().join("dst.json");
        assert!(move_file(&src, &dst).is_err());
    }

    #[test]
    fn test_move_file_falls_back_to_copy_on_cross_device_link_error() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let src = tmp.path().join("src.json");
        let dst = tmp.path().join("dst.json");
        fs::write(&src, "hello").expect("write src");

        move_file_with_ops(&src, &dst, rename_cross_device, copy_file, remove_file)
            .expect("move file with ops");

        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).expect("read dst"), "hello");
    }

    #[test]
    fn test_move_file_reports_copy_errors_with_context() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let src = tmp.path().join("src.json");
        let dst = tmp.path().join("dst.json");
        fs::write(&src, "hello").expect("write src");

        let err = move_file_with_ops(&src, &dst, rename_cross_device, copy_failed, remove_file)
            .expect_err("expected copy failure");

        let msg = format!("{err:#}");
        assert!(msg.contains("Failed to copy"));
    }

    #[test]
    fn test_move_file_reports_remove_errors_with_context() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let src = tmp.path().join("src.json");
        let dst = tmp.path().join("dst.json");
        fs::write(&src, "hello").expect("write src");

        let err = move_file_with_ops(&src, &dst, rename_cross_device, copy_file, remove_failed)
            .expect_err("expected remove failure");

        let msg = format!("{err:#}");
        assert!(msg.contains("Failed to remove"));
    }

    #[test]
    fn test_move_file_reports_rename_errors_with_context() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let src = tmp.path().join("src.json");
        let dst = tmp.path().join("dst.json");
        fs::write(&src, "hello").expect("write src");

        let err = move_file_with_ops(&src, &dst, rename_failed, copy_file, remove_file)
            .expect_err("expected rename failure");

        let msg = format!("{err:#}");
        assert!(msg.contains("Failed to rename"));
    }

    #[cfg(unix)]
    #[test]
    fn test_migrate_state_dir_skips_bak_scan_when_old_dir_unreadable() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");
        fs::create_dir_all(&old_dir).expect("create old dir");
        fs::write(old_dir.join("state.json"), "hello").expect("write state");

        let mut perms = fs::metadata(&old_dir)
            .expect("metadata old dir")
            .permissions();
        perms.set_mode(0o300);
        fs::set_permissions(&old_dir, perms).expect("set perms");

        let migrated = migrate_state_dir(&old_dir, &new_dir).expect("migrate state dir");
        assert!(migrated);
        assert!(new_dir.join("state.json").exists());

        let mut perms = fs::metadata(&old_dir)
            .expect("metadata old dir")
            .permissions();
        perms.set_mode(0o700);
        fs::set_permissions(&old_dir, perms).expect("restore perms");
    }

    #[test]
    fn test_migrate_state_dir_returns_false_when_old_dir_missing() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");
        assert!(!migrate_state_dir(&old_dir, &new_dir).expect("migrate_state_dir should succeed"));
    }

    #[test]
    fn test_migrate_state_dir_moves_bak_files_and_skips_non_files() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");

        fs::create_dir_all(&old_dir).expect("create old dir");
        fs::create_dir_all(&new_dir).expect("create new dir");

        fs::create_dir_all(old_dir.join("dir-entry")).expect("create subdir");

        fs::write(old_dir.join("keep.txt"), "not a backup").expect("write keep file");
        fs::write(old_dir.join("move.bak"), "backup").expect("write move file");
        fs::write(new_dir.join("skip.bak"), "existing").expect("write skip file");
        fs::write(old_dir.join("skip.bak"), "would be skipped").expect("write skip file");

        assert!(migrate_state_dir(&old_dir, &new_dir).expect("migrate state dir"));
        assert!(new_dir.join("move.bak").exists());
        assert!(new_dir.join("skip.bak").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_migrate_state_dir_ignores_non_utf8_bak_names() {
        use std::os::unix::ffi::OsStringExt;

        let file_name = std::ffi::OsString::from_vec(vec![0xff, b'.', b'b', b'a', b'k']);
        assert!(!is_migratable_bak_file_name(&file_name));
    }

    #[test]
    fn test_migrate_state_dir_reports_create_errors_with_context() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");
        fs::create_dir_all(&old_dir).expect("create old dir");
        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .unwrap();
        fs::write(&new_dir, "not a directory").expect("write file at new_dir");

        let err = migrate_state_dir(&old_dir, &new_dir).expect_err("expected create_dir_all error");
        let msg = format!("{err:#}");
        assert!(msg.contains("Failed to create"));
    }

    #[cfg(unix)]
    #[test]
    fn test_migrate_state_dir_reports_move_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");
        fs::create_dir_all(&old_dir).expect("create old dir");
        fs::create_dir_all(&new_dir).expect("create new dir");
        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .unwrap();

        let mut perms = fs::metadata(&new_dir)
            .expect("metadata new dir")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&new_dir, perms).expect("set perms");

        let result = migrate_state_dir(&old_dir, &new_dir);

        let mut perms = fs::metadata(&new_dir)
            .expect("metadata new dir")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&new_dir, perms).expect("restore perms");

        let err = result.expect_err("expected move failure");
        let msg = format!("{err:#}");
        assert!(msg.contains("Failed to move"));
    }

    #[cfg(unix)]
    #[test]
    fn test_migrate_state_dir_reports_remove_dir_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let parent = tmp.path().join("old-parent");
        let old_dir = parent.join("old");
        let new_dir = tmp.path().join("new");

        fs::create_dir_all(&old_dir).expect("create old dir");
        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .unwrap();

        let mut perms = fs::metadata(&parent)
            .expect("metadata parent")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&parent, perms).expect("set perms");

        let result = migrate_state_dir(&old_dir, &new_dir);

        let mut perms = fs::metadata(&parent)
            .expect("metadata parent")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&parent, perms).expect("restore perms");
        let _ = fs::remove_dir_all(&parent);

        let err = result.expect_err("expected remove_dir_all failure");
        let msg = format!("{err:#}");
        assert!(msg.contains("Failed to remove old state directory"));
    }

    #[test]
    fn test_migrate_state_dir_moves_settings_without_state() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");

        fs::create_dir_all(&old_dir).expect("create old dir");

        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .expect("write legacy settings");

        assert!(migrate_state_dir(&old_dir, &new_dir).expect("migrate state dir"));
        assert!(new_dir.join("settings.json").exists());
        assert!(!old_dir.exists());
    }

    #[test]
    fn test_migrate_state_dir_does_not_overwrite_existing_settings() {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let old_dir = tmp.path().join("old");
        let new_dir = tmp.path().join("new");

        fs::create_dir_all(&old_dir).expect("create old dir");
        fs::create_dir_all(&new_dir).expect("create new dir");

        fs::write(
            old_dir.join("settings.json"),
            r#"{"agent_program":"codex"}"#,
        )
        .expect("write legacy settings");
        fs::write(
            new_dir.join("settings.json"),
            r#"{"agent_program":"claude"}"#,
        )
        .expect("write current settings");

        assert!(!migrate_state_dir(&old_dir, &new_dir).expect("migrate state dir"));

        let settings =
            fs::read_to_string(new_dir.join("settings.json")).expect("read current settings");
        assert!(settings.contains("claude"));
    }
}
