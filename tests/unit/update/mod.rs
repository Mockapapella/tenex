use anyhow::{Result, anyhow};
use semver::Version;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use tempfile::TempDir;
use tenex::test_support::lock_env_test_environment;
use tenex::update::test_support as update_support;
use tenex::update::{UpdateInfo, check_for_update, install_latest};

fn error_message<T>(result: anyhow::Result<T>, context: &str) -> Result<String> {
    match result {
        Ok(_) => anyhow::bail!("{context}"),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn test_crates_io_base_url_defaults_to_crates_io() {
    let _guard = lock_env_test_environment();
    assert_eq!(update_support::crates_io_base_url(), "https://crates.io");
}

#[test]
fn test_cargo_program_defaults_to_cargo() {
    let _guard = lock_env_test_environment();
    assert_eq!(
        update_support::cargo_program(),
        std::path::PathBuf::from("cargo")
    );
}

fn mock_crates_response(version: &str) -> String {
    format!(r#"{{"crate":{{"max_version":"{version}"}}}}"#)
}

#[test]
fn test_check_for_update_uses_base_url_override() -> Result<()> {
    let _guard = lock_env_test_environment();
    let mut server = mockito::Server::new();
    let base = server.url();

    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_crates_response("99.0.0"))
        .create();

    let result = update_support::with_crates_io_base_url_override(base, check_for_update);
    mock.assert();
    drop(server);

    let info = result?.ok_or_else(|| anyhow!("expected update"))?;
    assert!(info.latest_version > info.current_version);
    Ok(())
}

#[test]
fn test_check_for_update_errors_when_cargo_pkg_version_is_not_semver() -> Result<()> {
    let _guard = lock_env_test_environment();
    let err = update_support::with_cargo_pkg_version_override("not-a-version".to_string(), || {
        error_message(check_for_update(), "expected update check to error")
    })?;
    assert!(err.contains("Tenex version in Cargo metadata must be valid semver"));
    Ok(())
}

#[test]
fn test_update_available() -> Result<()> {
    let mut server = mockito::Server::new();
    let url = format!("{}/api/v1/crates/tenex", server.url());
    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_crates_response("99.0.0"))
        .create();

    let current = Version::new(1, 0, 0);
    let result = update_support::check_for_update_impl(&url, &current);
    mock.assert();
    drop(server);

    let info = result?.ok_or_else(|| anyhow!("expected update"))?;
    assert_eq!(info.current_version, Version::new(1, 0, 0));
    assert_eq!(info.latest_version, Version::new(99, 0, 0));
    Ok(())
}

#[test]
fn test_no_update_same_version() -> Result<()> {
    let mut server = mockito::Server::new();
    let url = format!("{}/api/v1/crates/tenex", server.url());
    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_crates_response("1.0.0"))
        .create();

    let current = Version::new(1, 0, 0);
    let result = update_support::check_for_update_impl(&url, &current);
    mock.assert();
    drop(server);

    assert!(result?.is_none());
    Ok(())
}

#[test]
fn test_no_update_older_version() -> Result<()> {
    let mut server = mockito::Server::new();
    let url = format!("{}/api/v1/crates/tenex", server.url());
    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_crates_response("0.9.0"))
        .create();

    let current = Version::new(1, 0, 0);
    let result = update_support::check_for_update_impl(&url, &current);
    mock.assert();
    drop(server);

    assert!(result?.is_none());
    Ok(())
}

#[test]
fn test_http_error() -> Result<()> {
    let mut server = mockito::Server::new();
    let url = format!("{}/api/v1/crates/tenex", server.url());
    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(500)
        .create();

    let current = Version::new(1, 0, 0);
    let result = update_support::check_for_update_impl(&url, &current);
    mock.assert();
    drop(server);

    let err = error_message(result, "expected status error")?;
    assert!(err.contains("500"));
    Ok(())
}

#[test]
fn test_invalid_json() {
    let mut server = mockito::Server::new();
    let url = format!("{}/api/v1/crates/tenex", server.url());
    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("not valid json")
        .create();

    let current = Version::new(1, 0, 0);
    let result = update_support::check_for_update_impl(&url, &current);
    mock.assert();
    drop(server);

    assert!(result.is_err());
}

#[test]
fn test_invalid_version_string() {
    let mut server = mockito::Server::new();
    let url = format!("{}/api/v1/crates/tenex", server.url());
    let mock = server
        .mock("GET", "/api/v1/crates/tenex")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(mock_crates_response("not-a-version"))
        .create();

    let current = Version::new(1, 0, 0);
    let result = update_support::check_for_update_impl(&url, &current);
    mock.assert();
    drop(server);

    assert!(result.is_err());
}

#[test]
fn test_check_for_update_impl_returns_transport_errors_with_context() -> Result<()> {
    let current = Version::new(1, 0, 0);
    let result =
        update_support::check_for_update_impl("http://127.0.0.1:1/api/v1/crates/tenex", &current);
    let err = error_message(result, "expected transport error")?;
    assert!(err.contains("Failed to query crates.io for Tenex updates"));
    Ok(())
}

#[cfg(unix)]
fn write_fake_cargo_script(temp: &TempDir, body: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let script = temp.path().join("cargo");
    fs::write(&script, body)?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms)?;
    Ok(script)
}

#[cfg(unix)]
#[test]
fn test_install_latest_succeeds_when_cargo_exits_zero() -> Result<()> {
    let _guard = lock_env_test_environment();
    let temp = TempDir::new()?;
    let script = write_fake_cargo_script(&temp, "#!/bin/sh\nexit 0\n")?;
    update_support::with_cargo_program_override(&script, install_latest)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_install_latest_errors_when_cargo_exit_is_nonzero() -> Result<()> {
    let _guard = lock_env_test_environment();
    let temp = TempDir::new()?;
    let script = write_fake_cargo_script(&temp, "#!/bin/sh\nexit 1\n")?;

    let err = update_support::with_cargo_program_override(&script, || {
        error_message(install_latest(), "expected cargo install failure")
    })?;
    assert!(err.contains("cargo install exited unsuccessfully"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_install_latest_errors_when_cargo_cannot_run() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env_test_environment();
    let temp = TempDir::new()?;
    let script = temp.path().join("cargo");
    fs::write(&script, "#!/bin/sh\nexit 0\n")?;
    let mut perms = fs::metadata(&script)?.permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&script, perms)?;

    let err = update_support::with_cargo_program_override(&script, || {
        error_message(install_latest(), "expected cargo run failure")
    })?;
    assert!(err.contains("Failed to run cargo install"));
    Ok(())
}

#[test]
fn test_update_info_equality() {
    let info1 = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };
    let info2 = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };
    assert_eq!(info1, info2);
}

#[test]
fn test_update_info_debug() {
    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };
    let debug = format!("{info:?}");
    assert!(debug.contains("UpdateInfo"));
    // Version uses "major: 1" format in debug, not "1.0.0"
    assert!(debug.contains("current_version"));
    assert!(debug.contains("latest_version"));
}

#[test]
fn test_update_info_clone() {
    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };
    let cloned = info.clone();
    assert_eq!(info, cloned);
}
