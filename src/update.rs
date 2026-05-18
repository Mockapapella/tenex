//! Self-update support for Tenex.
//!
//! On startup, Tenex can query crates.io for a newer published version,
//! prompt the user, and optionally reinstall itself.

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::Deserialize;
use std::borrow::Cow;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use ureq::Agent;

/// Information about an available update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// Version of the running Tenex binary.
    pub current_version: Version,
    /// Latest version published on crates.io.
    pub latest_version: Version,
}

/// Parse the version of the running Tenex binary from Cargo metadata.
///
/// # Errors
///
/// Returns an error if Cargo metadata contains an invalid semver string.
pub fn current_version() -> Result<Version> {
    Version::parse(cargo_pkg_version().as_ref())
        .context("Tenex version in Cargo metadata must be valid semver")
}

#[derive(Debug, Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    krate: CratesIoCrate,
}

#[derive(Debug, Deserialize)]
struct CratesIoCrate {
    max_version: String,
}

#[cfg(test)]
fn crates_io_base_url_override_store() -> &'static std::sync::RwLock<Option<String>> {
    use std::sync::{OnceLock, RwLock};
    static STORE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

fn crates_io_base_url() -> String {
    #[cfg(test)]
    {
        let override_value = crates_io_base_url_override_store()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(value) = override_value {
            return value;
        }
    }

    "https://crates.io".to_string()
}

#[cfg(test)]
fn cargo_pkg_version_override_store() -> &'static std::sync::RwLock<Option<String>> {
    use std::sync::{OnceLock, RwLock};
    static STORE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

#[cfg(not(test))]
const fn cargo_pkg_version() -> Cow<'static, str> {
    Cow::Borrowed(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
fn cargo_pkg_version() -> Cow<'static, str> {
    let override_value = cargo_pkg_version_override_store()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(value) = override_value {
        return Cow::Owned(value);
    }

    Cow::Borrowed(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
fn with_cargo_pkg_version_override_for_tests<T>(version: String, f: impl FnOnce() -> T) -> T {
    let store = cargo_pkg_version_override_store();
    let previous = {
        let mut guard = store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.replace(version)
    };

    let result = f();

    {
        let mut guard = store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = previous;
    }

    result
}

#[cfg(test)]
fn with_crates_io_base_url_override_for_tests<T>(base_url: String, f: impl FnOnce() -> T) -> T {
    let store = crates_io_base_url_override_store();
    let previous = {
        let mut guard = store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.replace(base_url)
    };

    let result = f();

    {
        let mut guard = store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = previous;
    }

    result
}

#[cfg(test)]
fn cargo_program_override_store() -> &'static std::sync::RwLock<Option<PathBuf>> {
    use std::sync::{OnceLock, RwLock};
    static STORE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

fn cargo_program() -> PathBuf {
    #[cfg(test)]
    {
        let override_value = cargo_program_override_store()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(value) = override_value {
            return value;
        }
    }

    PathBuf::from("cargo")
}

#[cfg(test)]
fn with_cargo_program_override_for_tests<T>(program: PathBuf, f: impl FnOnce() -> T) -> T {
    let store = cargo_program_override_store();
    let previous = {
        let mut guard = store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.replace(program)
    };

    let result = f();

    {
        let mut guard = store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = previous;
    }

    result
}

/// Check crates.io to see if a newer version is available.
///
/// Returns `Ok(Some(UpdateInfo))` if an update exists, or `Ok(None)` if not.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response cannot be parsed.
pub fn check_for_update() -> Result<Option<UpdateInfo>> {
    let current_version = current_version()?;
    let base = crates_io_base_url();
    let base = base.trim_end_matches('/');
    let url = format!("{base}/api/v1/crates/{}", env!("CARGO_PKG_NAME"));
    check_for_update_impl(&url, &current_version)
}

/// Internal implementation that allows injecting the URL and current version for testing.
fn check_for_update_impl(url: &str, current_version: &Version) -> Result<Option<UpdateInfo>> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(3)))
        .build();
    let agent: Agent = config.new_agent();
    let user_agent = format!("tenex/{current_version}");

    let response = match agent.get(url).header("User-Agent", user_agent).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(status)) => {
            return Err(anyhow!(
                "crates.io update check failed with status {status}"
            ));
        }
        Err(err) => {
            return Err(anyhow!(err)).context("Failed to query crates.io for Tenex updates");
        }
    };

    let body: CratesIoResponse = response
        .into_body()
        .read_json()
        .context("Failed to deserialize crates.io response")?;

    let latest_version = Version::parse(&body.krate.max_version)
        .context("Failed to parse latest Tenex version from crates.io")?;

    if latest_version > *current_version {
        Ok(Some(UpdateInfo {
            current_version: current_version.clone(),
            latest_version,
        }))
    } else {
        Ok(None)
    }
}

/// Reinstall Tenex to the latest published version using Cargo.
///
/// This runs `cargo install tenex --locked --force` and streams output to the terminal.
///
/// # Errors
///
/// Returns an error if `cargo` is not available or the install command fails.
pub fn install_latest() -> Result<()> {
    let status = Command::new(cargo_program())
        .args(["install", env!("CARGO_PKG_NAME"), "--locked", "--force"])
        .status()
        .context("Failed to run cargo install")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("cargo install exited unsuccessfully: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use tempfile::TempDir;

    #[test]
    fn test_crates_io_base_url_defaults_to_crates_io() {
        assert_eq!(crates_io_base_url(), "https://crates.io");
    }

    #[test]
    fn test_cargo_program_defaults_to_cargo() {
        assert_eq!(cargo_program(), PathBuf::from("cargo"));
    }

    fn mock_crates_response(version: &str) -> String {
        format!(r#"{{"crate":{{"max_version":"{version}"}}}}"#)
    }

    #[test]
    fn test_check_for_update_uses_base_url_override() {
        let mut server = mockito::Server::new();
        let base = server.url();

        let mock = server
            .mock("GET", "/api/v1/crates/tenex")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_crates_response("99.0.0"))
            .create();

        let result = with_crates_io_base_url_override_for_tests(base, check_for_update);
        mock.assert();
        drop(server);

        let info = result
            .expect("update check failed")
            .expect("expected update");
        assert!(info.latest_version > info.current_version);
    }

    #[test]
    fn test_check_for_update_errors_when_cargo_pkg_version_is_not_semver() {
        let err = with_cargo_pkg_version_override_for_tests("not-a-version".to_string(), || {
            check_for_update().expect_err("expected update check to error")
        });
        assert!(
            err.to_string()
                .contains("Tenex version in Cargo metadata must be valid semver")
        );
    }

    #[test]
    fn test_update_available() {
        let mut server = mockito::Server::new();
        let url = format!("{}/api/v1/crates/tenex", server.url());
        let mock = server
            .mock("GET", "/api/v1/crates/tenex")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_crates_response("99.0.0"))
            .create();

        let current = Version::new(1, 0, 0);
        let result = check_for_update_impl(&url, &current);
        mock.assert();
        drop(server);

        let info = result
            .expect("update check failed")
            .expect("expected update");
        assert_eq!(info.current_version, Version::new(1, 0, 0));
        assert_eq!(info.latest_version, Version::new(99, 0, 0));
    }

    #[test]
    fn test_no_update_same_version() {
        let mut server = mockito::Server::new();
        let url = format!("{}/api/v1/crates/tenex", server.url());
        let mock = server
            .mock("GET", "/api/v1/crates/tenex")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_crates_response("1.0.0"))
            .create();

        let current = Version::new(1, 0, 0);
        let result = check_for_update_impl(&url, &current);
        mock.assert();
        drop(server);

        assert!(result.is_ok());
        assert!(result.ok().flatten().is_none());
    }

    #[test]
    fn test_no_update_older_version() {
        let mut server = mockito::Server::new();
        let url = format!("{}/api/v1/crates/tenex", server.url());
        let mock = server
            .mock("GET", "/api/v1/crates/tenex")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(mock_crates_response("0.9.0"))
            .create();

        let current = Version::new(1, 0, 0);
        let result = check_for_update_impl(&url, &current);
        mock.assert();
        drop(server);

        assert!(result.is_ok());
        assert!(result.ok().flatten().is_none());
    }

    #[test]
    fn test_http_error() {
        let mut server = mockito::Server::new();
        let url = format!("{}/api/v1/crates/tenex", server.url());
        let mock = server
            .mock("GET", "/api/v1/crates/tenex")
            .with_status(500)
            .create();

        let current = Version::new(1, 0, 0);
        let result = check_for_update_impl(&url, &current);
        mock.assert();
        drop(server);

        let err = result.expect_err("expected status error");
        assert!(err.to_string().contains("500"));
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
        let result = check_for_update_impl(&url, &current);
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
        let result = check_for_update_impl(&url, &current);
        mock.assert();
        drop(server);

        assert!(result.is_err());
    }

    #[test]
    fn test_check_for_update_impl_returns_transport_errors_with_context() {
        let current = Version::new(1, 0, 0);
        let result = check_for_update_impl("http://127.0.0.1:1/api/v1/crates/tenex", &current);
        let err = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(err.contains("Failed to query crates.io for Tenex updates"));
    }

    #[cfg(unix)]
    fn write_fake_cargo_script(temp: &TempDir, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("cargo");
        fs::write(&script, body).expect("write fake cargo script");
        let mut perms = fs::metadata(&script)
            .expect("metadata fake cargo script")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("set fake cargo script permissions");
        script
    }

    #[cfg(unix)]
    #[test]
    fn test_install_latest_succeeds_when_cargo_exits_zero() {
        let temp = TempDir::new().expect("temp dir");
        let script = write_fake_cargo_script(&temp, "#!/bin/sh\nexit 0\n");
        with_cargo_program_override_for_tests(script, || {
            install_latest().expect("install latest");
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_install_latest_errors_when_cargo_exit_is_nonzero() {
        let temp = TempDir::new().expect("temp dir");
        let script = write_fake_cargo_script(&temp, "#!/bin/sh\nexit 1\n");

        let err = with_cargo_program_override_for_tests(script, install_latest)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(err.contains("cargo install exited unsuccessfully"));
    }

    #[cfg(unix)]
    #[test]
    fn test_install_latest_errors_when_cargo_cannot_run() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("temp dir");
        let script = temp.path().join("cargo");
        fs::write(&script, "#!/bin/sh\nexit 0\n").expect("write fake cargo script");
        let mut perms = fs::metadata(&script)
            .expect("metadata fake cargo script")
            .permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&script, perms).expect("set fake cargo script permissions");

        let err = with_cargo_program_override_for_tests(script, install_latest)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(err.contains("Failed to run cargo install"));
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
}
