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

#[cfg(any(test, feature = "test-support"))]
fn crates_io_base_url_override_store() -> &'static std::sync::RwLock<Option<String>> {
    use std::sync::{OnceLock, RwLock};
    static STORE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

fn crates_io_base_url() -> String {
    #[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
fn cargo_pkg_version_override_store() -> &'static std::sync::RwLock<Option<String>> {
    use std::sync::{OnceLock, RwLock};
    static STORE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

#[cfg(not(any(test, feature = "test-support")))]
const fn cargo_pkg_version() -> Cow<'static, str> {
    Cow::Borrowed(env!("CARGO_PKG_VERSION"))
}

#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
fn cargo_program_override_store() -> &'static std::sync::RwLock<Option<PathBuf>> {
    use std::sync::{OnceLock, RwLock};
    static STORE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(None))
}

fn cargo_program() -> PathBuf {
    #[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
/// Integration-test helpers for otherwise private update injection points.
pub mod test_support {
    use super::UpdateInfo;
    use anyhow::Result;
    use semver::Version;
    use std::path::{Path, PathBuf};

    /// Return the current crates.io base URL, including any active test override.
    #[must_use]
    pub fn crates_io_base_url() -> String {
        super::crates_io_base_url()
    }

    /// Return the current cargo program, including any active test override.
    #[must_use]
    pub fn cargo_program() -> PathBuf {
        super::cargo_program()
    }

    /// Check a supplied update endpoint against an injected current version.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub fn check_for_update_impl(
        url: &str,
        current_version: &Version,
    ) -> Result<Option<UpdateInfo>> {
        super::check_for_update_impl(url, current_version)
    }

    /// Run a closure with a temporary Cargo package version override.
    pub fn with_cargo_pkg_version_override<T>(version: String, f: impl FnOnce() -> T) -> T {
        super::with_cargo_pkg_version_override_for_tests(version, f)
    }

    /// Run a closure with a temporary crates.io base URL override.
    pub fn with_crates_io_base_url_override<T>(base_url: String, f: impl FnOnce() -> T) -> T {
        super::with_crates_io_base_url_override_for_tests(base_url, f)
    }

    /// Run a closure with a temporary cargo program override.
    pub fn with_cargo_program_override<T>(program: &Path, f: impl FnOnce() -> T) -> T {
        super::with_cargo_program_override_for_tests(program.to_path_buf(), f)
    }
}
