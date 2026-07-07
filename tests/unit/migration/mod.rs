use anyhow::Result;
use std::fs;
use std::path::Path;
use tenex::migration::test_support as migration_support;

fn rename_cross_device(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from_raw_os_error(18))
}

fn rename_failed(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("rename failed"))
}

fn copy_failed(_src: &Path, _dst: &Path) -> std::io::Result<u64> {
    Err(std::io::Error::other("copy failed"))
}

fn copy_file(src: &Path, dst: &Path) -> std::io::Result<u64> {
    fs::copy(src, dst)
}

fn remove_failed(_src: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("remove failed"))
}

fn remove_file(src: &Path) -> std::io::Result<()> {
    fs::remove_file(src)
}

fn error_chain<T>(result: anyhow::Result<T>, context: &str) -> Result<String> {
    match result {
        Ok(_) => anyhow::bail!("{context}"),
        Err(error) => Ok(format!("{error:#}")),
    }
}

#[test]
fn test_move_file_accepts_missing_source() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let src = tmp.path().join("missing.json");
    let dst = tmp.path().join("dst.json");
    assert!(migration_support::move_file(&src, &dst).is_err());
    Ok(())
}

#[test]
fn test_migration_file_helpers_copy_and_remove_files() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let src = tmp.path().join("src.json");
    let dst = tmp.path().join("dst.json");
    fs::write(&src, "hello")?;

    migration_support::copy_file(&src, &dst)?;
    assert_eq!(fs::read_to_string(&dst)?, "hello");

    migration_support::remove_file(&src)?;
    assert!(!src.exists());
    Ok(())
}

#[test]
fn test_move_file_falls_back_to_copy_on_cross_device_link_error() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let src = tmp.path().join("src.json");
    let dst = tmp.path().join("dst.json");
    fs::write(&src, "hello")?;

    migration_support::move_file_with_ops(&src, &dst, rename_cross_device, copy_file, remove_file)?;

    assert!(!src.exists());
    assert_eq!(fs::read_to_string(&dst)?, "hello");
    Ok(())
}

#[test]
fn test_move_file_reports_copy_errors_with_context() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let src = tmp.path().join("src.json");
    let dst = tmp.path().join("dst.json");
    fs::write(&src, "hello")?;

    let msg = error_chain(
        migration_support::move_file_with_ops(
            &src,
            &dst,
            rename_cross_device,
            copy_failed,
            remove_file,
        ),
        "expected copy failure",
    )?;
    assert!(msg.contains("Failed to copy"));
    Ok(())
}

#[test]
fn test_move_file_reports_remove_errors_with_context() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let src = tmp.path().join("src.json");
    let dst = tmp.path().join("dst.json");
    fs::write(&src, "hello")?;

    let msg = error_chain(
        migration_support::move_file_with_ops(
            &src,
            &dst,
            rename_cross_device,
            copy_file,
            remove_failed,
        ),
        "expected remove failure",
    )?;
    assert!(msg.contains("Failed to remove"));
    Ok(())
}

#[test]
fn test_move_file_reports_rename_errors_with_context() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let src = tmp.path().join("src.json");
    let dst = tmp.path().join("dst.json");
    fs::write(&src, "hello")?;

    let msg = error_chain(
        migration_support::move_file_with_ops(&src, &dst, rename_failed, copy_file, remove_file),
        "expected rename failure",
    )?;
    assert!(msg.contains("Failed to rename"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_migrate_state_dir_skips_bak_scan_when_old_dir_unreadable() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");
    fs::create_dir_all(&old_dir)?;
    fs::write(old_dir.join("state.json"), "hello")?;

    let mut perms = fs::metadata(&old_dir)?.permissions();
    perms.set_mode(0o300);
    fs::set_permissions(&old_dir, perms)?;

    let migrated = migration_support::migrate_state_dir(&old_dir, &new_dir)?;
    assert!(migrated);
    assert!(new_dir.join("state.json").exists());

    let mut perms = fs::metadata(&old_dir)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(&old_dir, perms)?;
    Ok(())
}

#[test]
fn test_migrate_state_dir_returns_false_when_old_dir_missing() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");
    assert!(!migration_support::migrate_state_dir(&old_dir, &new_dir)?);
    Ok(())
}

#[test]
fn test_migrate_state_dir_moves_bak_files_and_skips_non_files() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");

    fs::create_dir_all(&old_dir)?;
    fs::create_dir_all(&new_dir)?;

    fs::create_dir_all(old_dir.join("dir-entry"))?;

    fs::write(old_dir.join("keep.txt"), "not a backup")?;
    fs::write(old_dir.join("move.bak"), "backup")?;
    fs::write(new_dir.join("skip.bak"), "existing")?;
    fs::write(old_dir.join("skip.bak"), "would be skipped")?;

    assert!(migration_support::migrate_state_dir(&old_dir, &new_dir)?);
    assert!(new_dir.join("move.bak").exists());
    assert!(new_dir.join("skip.bak").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_migrate_state_dir_ignores_non_utf8_bak_names() {
    use std::os::unix::ffi::OsStringExt;

    let file_name = std::ffi::OsString::from_vec(vec![0xff, b'.', b'b', b'a', b'k']);
    assert!(!migration_support::is_migratable_bak_file_name(&file_name));
}

#[test]
fn test_migrate_state_dir_reports_create_errors_with_context() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");
    fs::create_dir_all(&old_dir)?;
    fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;
    fs::write(&new_dir, "not a directory")?;

    let msg = error_chain(
        migration_support::migrate_state_dir(&old_dir, &new_dir),
        "expected create_dir_all error",
    )?;
    assert!(msg.contains("Failed to create"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_migrate_state_dir_reports_move_errors_with_context() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");
    fs::create_dir_all(&old_dir)?;
    fs::create_dir_all(&new_dir)?;
    fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    let mut perms = fs::metadata(&new_dir)?.permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&new_dir, perms)?;

    let result = migration_support::migrate_state_dir(&old_dir, &new_dir);

    let mut perms = fs::metadata(&new_dir)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&new_dir, perms)?;

    let msg = error_chain(result, "expected move failure")?;
    assert!(msg.contains("Failed to move"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_migrate_state_dir_reports_remove_dir_errors_with_context() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new()?;
    let parent = tmp.path().join("old-parent");
    let old_dir = parent.join("old");
    let new_dir = tmp.path().join("new");

    fs::create_dir_all(&old_dir)?;
    fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    let mut perms = fs::metadata(&parent)?.permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&parent, perms)?;

    let result = migration_support::migrate_state_dir(&old_dir, &new_dir);

    let mut perms = fs::metadata(&parent)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&parent, perms)?;
    let _remove_result = fs::remove_dir_all(&parent);

    let msg = error_chain(result, "expected remove_dir_all failure")?;
    assert!(msg.contains("Failed to remove old state directory"));
    Ok(())
}

#[test]
fn test_migrate_state_dir_moves_settings_without_state() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");

    fs::create_dir_all(&old_dir)?;

    fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    assert!(migration_support::migrate_state_dir(&old_dir, &new_dir)?);
    assert!(new_dir.join("settings.json").exists());
    assert!(!old_dir.exists());
    Ok(())
}

#[test]
fn test_migrate_state_dir_does_not_overwrite_existing_settings() -> Result<()> {
    let tmp = tempfile::TempDir::new()?;
    let old_dir = tmp.path().join("old");
    let new_dir = tmp.path().join("new");

    fs::create_dir_all(&old_dir)?;
    fs::create_dir_all(&new_dir)?;

    fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;
    fs::write(
        new_dir.join("settings.json"),
        r#"{"agent_program":"claude"}"#,
    )?;

    assert!(!migration_support::migrate_state_dir(&old_dir, &new_dir)?);

    let settings = fs::read_to_string(new_dir.join("settings.json"))?;
    assert!(settings.contains("claude"));
    Ok(())
}
