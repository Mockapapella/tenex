//! Smoke tests for `tenex::migration::migrate_default_state_dir` without mutating the parent
//! process environment.

use std::process::Command;

#[test]
fn test_migration_smoke_noop_when_data_local_dir_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_migration_smoke"));
    cmd.env_remove("TENEX_STATE_PATH");
    cmd.env_remove("XDG_DATA_HOME");
    cmd.env_remove("HOME");
    let status = cmd.status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn test_migration_smoke_noop_when_legacy_dir_missing() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let xdg = tmp.path().join("xdg");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&xdg)?;
    std::fs::create_dir_all(&home)?;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_migration_smoke"));
    cmd.env_remove("TENEX_STATE_PATH");
    cmd.env("XDG_DATA_HOME", &xdg);
    cmd.env("HOME", &home);

    let status = cmd.status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn test_migration_smoke_migrates_settings_into_default_instance_root()
-> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let xdg = tmp.path().join("xdg");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&xdg)?;
    std::fs::create_dir_all(&home)?;

    let old_dir = xdg.join("tenex");
    std::fs::create_dir_all(&old_dir)?;
    std::fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_migration_smoke"));
    cmd.env_remove("TENEX_STATE_PATH");
    cmd.env("XDG_DATA_HOME", &xdg);
    cmd.env("HOME", &home);

    let status = cmd.status()?;
    assert!(status.success());

    assert!(home.join(".tenex").join("settings.json").exists());
    assert!(!old_dir.exists());
    Ok(())
}

#[test]
fn test_migration_smoke_exits_nonzero_when_default_instance_root_is_not_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let xdg = tmp.path().join("xdg");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&xdg)?;
    std::fs::create_dir_all(&home)?;

    let old_dir = xdg.join("tenex");
    std::fs::create_dir_all(&old_dir)?;
    std::fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    std::fs::write(home.join(".tenex"), "not-a-dir")?;

    let output = Command::new(env!("CARGO_BIN_EXE_migration_smoke"))
        .env_remove("TENEX_STATE_PATH")
        .env("XDG_DATA_HOME", &xdg)
        .env("HOME", &home)
        .output()?;
    assert!(
        !output.status.success(),
        "expected migration_smoke failure, got status={:?}",
        output.status.code()
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn test_migration_smoke_skips_move_when_destination_settings_exists()
-> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let xdg = tmp.path().join("xdg");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&xdg)?;
    std::fs::create_dir_all(&home)?;

    let old_dir = xdg.join("tenex");
    std::fs::create_dir_all(&old_dir)?;
    std::fs::write(
        old_dir.join("settings.json"),
        r#"{"agent_program":"codex"}"#,
    )?;

    let new_dir = home.join(".tenex");
    std::fs::create_dir_all(&new_dir)?;
    std::fs::write(
        new_dir.join("settings.json"),
        r#"{"agent_program":"claude"}"#,
    )?;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_migration_smoke"));
    cmd.env_remove("TENEX_STATE_PATH");
    cmd.env("XDG_DATA_HOME", &xdg);
    cmd.env("HOME", &home);

    let status = cmd.status()?;
    assert!(status.success());

    assert!(new_dir.join("settings.json").exists());
    assert!(old_dir.join("settings.json").exists());
    Ok(())
}

#[test]
fn test_migration_smoke_moves_bak_files_into_default_instance_root()
-> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let xdg = tmp.path().join("xdg");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&xdg)?;
    std::fs::create_dir_all(&home)?;

    let old_dir = xdg.join("tenex");
    std::fs::create_dir_all(&old_dir)?;
    std::fs::write(old_dir.join("state.json.bak"), "backup")?;

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_migration_smoke"));
    cmd.env_remove("TENEX_STATE_PATH");
    cmd.env("XDG_DATA_HOME", &xdg);
    cmd.env("HOME", &home);

    let status = cmd.status()?;
    assert!(status.success());

    let new_dir = home.join(".tenex");
    assert!(new_dir.join("state.json.bak").exists());
    assert!(!old_dir.exists());
    Ok(())
}
