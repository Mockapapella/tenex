//! Self-update support for Tenex.
//!
//! On startup, Tenex can query crates.io for a newer published version,
//! prompt the user, and optionally reinstall itself.

use anyhow::{Context, Result, anyhow};
use semver::Version;
use serde::Deserialize;
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
    Version::parse(env!("CARGO_PKG_VERSION"))
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

/// Check crates.io to see if a newer version is available.
///
/// Returns `Ok(Some(UpdateInfo))` if an update exists, or `Ok(None)` if not.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response cannot be parsed.
pub fn check_for_update() -> Result<Option<UpdateInfo>> {
    let current_version = current_version()?;
    let url = format!("https://crates.io/api/v1/crates/{}", env!("CARGO_PKG_NAME"));
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

    if latest_version > current_version {
        Ok(Some(UpdateInfo {
            current_version,
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
    let status = Command::new("cargo")
        .args(["install", env!("CARGO_PKG_NAME"), "--locked", "--force"])
        .status()
        .context("Failed to run cargo install")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("cargo install exited unsuccessfully: {status}"))
    }
}
