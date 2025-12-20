//! IPC endpoint naming for the mux daemon.

use crate::config::Config;
use anyhow::{Context, Result, bail};
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, Name, prelude::*};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

/// IPC endpoint details for the mux daemon.
#[derive(Debug, Clone)]
pub struct SocketEndpoint {
    /// The interprocess socket name.
    pub name: Name<'static>,
    /// Optional filesystem path to remove during cleanup (when path-based sockets are used).
    pub cleanup_path: Option<PathBuf>,
    /// Human-friendly name for logs/errors.
    pub display: String,
}

static SOCKET_OVERRIDE: OnceLock<String> = OnceLock::new();
static DEFAULT_SOCKET_NAME: OnceLock<String> = OnceLock::new();

const DEFAULT_SOCKET_PREFIX: &str = "tenex-mux";

/// Override the mux daemon socket for this process.
///
/// This is intended for integration tests and embedding Tenex as a library. The
/// override must be set before the first mux request; otherwise the existing
/// endpoint choice will already be cached.
///
/// The value uses the same interpretation as `TENEX_MUX_SOCKET`.
///
/// # Errors
///
/// Returns an error if the override is empty or already set.
pub fn set_socket_override(value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("Mux socket override cannot be empty");
    }

    SOCKET_OVERRIDE
        .set(trimmed.to_string())
        .map_err(|_| anyhow::anyhow!("Mux socket override is already set"))?;
    Ok(())
}

/// Resolve the mux daemon's IPC endpoint.
///
/// The name is namespaced when supported (preferred), otherwise it falls back
/// to a filesystem path under Tenex's data directory.
///
/// Set `TENEX_MUX_SOCKET` to override the endpoint name/path. When the value
/// contains a path separator it is treated as a filesystem path; otherwise it
/// is treated as a namespaced socket name when supported.
///
/// # Errors
///
/// Returns an error if the name cannot be constructed.
pub fn socket_endpoint() -> Result<SocketEndpoint> {
    if let Some(override_value) = SOCKET_OVERRIDE.get() {
        return socket_endpoint_from_value(override_value);
    }

    if let Ok(raw) = std::env::var("TENEX_MUX_SOCKET") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return socket_endpoint_from_value(trimmed);
        }
    }

    if GenericNamespaced::is_supported() {
        let display = default_socket_name();
        return Ok(SocketEndpoint {
            name: display
                .clone()
                .to_ns_name::<GenericNamespaced>()?
                .into_owned(),
            cleanup_path: None,
            display,
        });
    }

    let state_path = Config::state_path();
    let dir = state_path
        .parent()
        .context("State path has no parent directory")?;
    let socket_name = default_socket_name();
    let socket_path = dir.join(format!("{socket_name}.sock"));
    let display = socket_path.to_string_lossy().into_owned();
    Ok(SocketEndpoint {
        name: socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()?
            .into_owned(),
        cleanup_path: Some(socket_path),
        display,
    })
}

fn socket_endpoint_from_value(value: &str) -> Result<SocketEndpoint> {
    let display = value.to_string();
    let looks_like_path = display.contains('/') || display.contains('\\');

    if looks_like_path {
        #[cfg(windows)]
        {
            let pipe_path = windows_pipe_path(&display);
            return Ok(SocketEndpoint {
                name: PathBuf::from(&pipe_path)
                    .as_path()
                    .to_fs_name::<GenericFilePath>()?
                    .into_owned(),
                cleanup_path: None,
                display: pipe_path,
            });
        }

        #[cfg(not(windows))]
        {
            let socket_path = PathBuf::from(&display);
            return Ok(SocketEndpoint {
                name: socket_path
                    .as_path()
                    .to_fs_name::<GenericFilePath>()?
                    .into_owned(),
                cleanup_path: Some(socket_path),
                display,
            });
        }
    }

    if GenericNamespaced::is_supported() {
        return Ok(SocketEndpoint {
            name: display
                .clone()
                .to_ns_name::<GenericNamespaced>()?
                .into_owned(),
            cleanup_path: None,
            display,
        });
    }

    let state_path = Config::state_path();
    let dir = state_path
        .parent()
        .context("State path has no parent directory")?;
    let socket_path = dir.join(format!("{display}.sock"));
    let display = socket_path.to_string_lossy().into_owned();
    Ok(SocketEndpoint {
        name: socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()?
            .into_owned(),
        cleanup_path: Some(socket_path),
        display,
    })
}

fn default_socket_name() -> String {
    DEFAULT_SOCKET_NAME
        .get_or_init(|| {
            let Some(fingerprint) = socket_fingerprint() else {
                return DEFAULT_SOCKET_PREFIX.to_string();
            };
            format!("{DEFAULT_SOCKET_PREFIX}-{fingerprint}")
        })
        .clone()
}

#[cfg(windows)]
fn windows_pipe_path(value: &str) -> String {
    if is_named_pipe_path(value) {
        return value.to_string();
    }

    let file_name = std::path::Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_SOCKET_PREFIX);

    format!(r"\\.\pipe\{file_name}")
}

#[cfg(windows)]
fn is_named_pipe_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with(r"\\.\pipe\")
        || lower.starts_with(r"\\?\pipe\")
        || lower.starts_with(r"//./pipe/")
        || lower.starts_with(r"//?/pipe/")
}

fn socket_fingerprint() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let metadata = std::fs::metadata(exe).ok()?;

    let len = metadata.len();
    let modified = metadata.modified().ok()?;
    let modified = modified.duration_since(UNIX_EPOCH).ok()?;
    let modified_secs = modified.as_secs();
    let modified_nanos = modified.subsec_nanos();

    let mut hash = FNV_OFFSET_BASIS;
    hash = fnv1a_update(hash, &len.to_le_bytes());
    hash = fnv1a_update(hash, &modified_secs.to_le_bytes());
    hash = fnv1a_update(hash, &modified_nanos.to_le_bytes());

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hash = fnv1a_update(hash, &metadata.ino().to_le_bytes());
    }

    Some(format!("{hash:016x}"))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_fingerprint_format() {
        let fingerprint = socket_fingerprint();
        if let Some(fingerprint) = fingerprint {
            assert_eq!(fingerprint.len(), 16);
            assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn test_socket_endpoint_from_value_path_like() -> Result<()> {
        let tmp_path = std::env::temp_dir().join("tenex-mux-test.sock");
        let endpoint = socket_endpoint_from_value(&tmp_path.to_string_lossy())?;
        if cfg!(windows) {
            assert!(endpoint.cleanup_path.is_none());
        } else {
            assert!(endpoint.cleanup_path.is_some());
        }
        assert!(!endpoint.display.is_empty());
        Ok(())
    }

    #[test]
    fn test_socket_endpoint_from_value_name_like() -> Result<()> {
        let endpoint = socket_endpoint_from_value("tenex-mux-test-name")?;
        assert!(!endpoint.display.is_empty());
        Ok(())
    }

    #[test]
    fn test_socket_endpoint_default() -> Result<()> {
        let endpoint = socket_endpoint()?;
        assert!(!endpoint.display.is_empty());
        Ok(())
    }

    #[test]
    fn test_set_socket_override_rejects_empty() -> Result<(), Box<dyn std::error::Error>> {
        match set_socket_override("   ") {
            Ok(()) => Err("Expected empty override to fail".into()),
            Err(err) => {
                assert!(err.to_string().contains("cannot be empty"));
                Ok(())
            }
        }
    }

    #[test]
    fn test_set_socket_override_already_set() -> Result<(), Box<dyn std::error::Error>> {
        let name = format!("tenex-mux-test-{}", std::process::id());
        if let Err(err) = set_socket_override(&name) {
            if err.to_string().contains("already set") {
                return Ok(());
            }
            return Err(err.into());
        }

        match set_socket_override("tenex-mux-test-other") {
            Ok(()) => Err("Expected override already set".into()),
            Err(err) => {
                assert!(err.to_string().contains("already set"));
                Ok(())
            }
        }
    }
}
