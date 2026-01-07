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
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .context("Failed to parse current Tenex version")?;
    let url = format!("https://crates.io/api/v1/crates/{}", env!("CARGO_PKG_NAME"));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_crates_response(version: &str) -> String {
        format!(r#"{{"crate":{{"max_version":"{version}"}}}}"#)
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

        let info = result.ok().flatten();
        assert!(info.is_some());
        if let Some(info) = info {
            assert_eq!(info.current_version, Version::new(1, 0, 0));
            assert_eq!(info.latest_version, Version::new(99, 0, 0));
        }
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

        assert!(result.is_err());
        if let Err(err) = result {
            assert!(err.to_string().contains("500"));
        }
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
