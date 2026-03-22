//! IPC endpoint naming for the mux daemon.

use crate::config::Config;
use anyhow::{Context, Result, bail};
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, Name, prelude::*};
#[cfg(test)]
use parking_lot::Mutex;
#[cfg(test)]
use std::collections::HashMap;
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

#[cfg(not(test))]
static SOCKET_OVERRIDE: OnceLock<String> = OnceLock::new();
#[cfg(not(test))]
static DEFAULT_SOCKET_NAME: OnceLock<String> = OnceLock::new();
#[cfg(test)]
static TEST_SOCKET_OVERRIDES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

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

    #[cfg(test)]
    {
        let key = test_scope_key();
        {
            let overrides = TEST_SOCKET_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()));
            let mut overrides = overrides.lock();
            if overrides.contains_key(&key) {
                bail!("Mux socket override is already set");
            }
            overrides.insert(key, trimmed.to_string());
        }
        Ok(())
    }

    #[cfg(not(test))]
    {
        SOCKET_OVERRIDE
            .set(trimmed.to_string())
            .map_err(|_| anyhow::anyhow!("Mux socket override is already set"))?;
        Ok(())
    }
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
    #[cfg(test)]
    if let Some(override_value) = test_socket_override() {
        return socket_endpoint_from_value(&override_value);
    }

    #[cfg(not(test))]
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

/// Resolve a mux socket endpoint from a display value.
///
/// This mirrors the parsing rules of `TENEX_MUX_SOCKET`:
/// - Values containing a path separator are treated as filesystem socket paths.
/// - Otherwise, values are treated as namespaced socket names when supported.
///
/// # Errors
///
/// Returns an error if the endpoint cannot be constructed.
pub(super) fn socket_endpoint_from_value(value: &str) -> Result<SocketEndpoint> {
    let display = value.to_string();
    let looks_like_path = display.contains('/') || display.contains('\\');

    if looks_like_path {
        #[cfg(windows)]
        {
            let socket_path = PathBuf::from(&display);
            if let Ok(name) = socket_path.as_path().to_fs_name::<GenericFilePath>() {
                return Ok(SocketEndpoint {
                    name: name.into_owned(),
                    cleanup_path: None,
                    display,
                });
            }

            return Ok(SocketEndpoint {
                name: display
                    .clone()
                    .to_ns_name::<GenericNamespaced>()?
                    .into_owned(),
                cleanup_path: None,
                display,
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
    #[cfg(test)]
    {
        let scope_suffix = test_scope_suffix();
        if let Some(fingerprint) = socket_fingerprint() {
            return format!("{DEFAULT_SOCKET_PREFIX}-{fingerprint}-{scope_suffix}");
        }
        format!("{DEFAULT_SOCKET_PREFIX}-{scope_suffix}")
    }

    #[cfg(not(test))]
    {
        DEFAULT_SOCKET_NAME
            .get_or_init(|| {
                let Some(fingerprint) = socket_fingerprint() else {
                    return DEFAULT_SOCKET_PREFIX.to_string();
                };
                format!("{DEFAULT_SOCKET_PREFIX}-{fingerprint}")
            })
            .clone()
    }
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

    #[cfg(target_os = "linux")]
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
fn test_scope_key() -> String {
    std::thread::current().name().map_or_else(
        || format!("{:?}", std::thread::current().id()),
        std::borrow::ToOwned::to_owned,
    )
}

#[cfg(test)]
fn test_socket_override() -> Option<String> {
    let key = test_scope_key();
    TEST_SOCKET_OVERRIDES
        .get()
        .and_then(|overrides| overrides.lock().get(&key).cloned())
}

#[cfg(test)]
fn test_scope_suffix() -> String {
    let mut hash = FNV_OFFSET_BASIS;
    hash = fnv1a_update(hash, test_scope_key().as_bytes());
    format!("{hash:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

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
        #[cfg(windows)]
        assert!(endpoint.cleanup_path.is_none());
        #[cfg(not(windows))]
        assert!(endpoint.cleanup_path.is_some());
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
        if let Err(err) = set_socket_override(&name)
            && !err.to_string().contains("already set")
        {
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

    #[test]
    fn test_test_scope_key_uses_current_thread_name() {
        let thread = std::thread::current();
        let current = thread.name().unwrap_or("unknown");
        assert_eq!(test_scope_key(), current);
    }

    #[test]
    fn test_test_scope_key_falls_back_for_unnamed_thread() -> Result<(), Box<dyn std::error::Error>>
    {
        let handle = std::thread::spawn(test_scope_key);
        let scope = handle
            .join()
            .map_err(|_| anyhow!("Unnamed thread panicked"))?;
        assert!(scope.starts_with("ThreadId("));
        Ok(())
    }

    #[test]
    fn test_default_socket_name_is_scoped_per_named_thread()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = std::thread::Builder::new()
            .name("endpoint-scope-one".to_string())
            .spawn(default_socket_name)?
            .join()
            .map_err(|_| anyhow!("First endpoint thread panicked"))?;
        let second = std::thread::Builder::new()
            .name("endpoint-scope-two".to_string())
            .spawn(default_socket_name)?
            .join()
            .map_err(|_| anyhow!("Second endpoint thread panicked"))?;

        assert!(!first.is_empty());
        assert!(!second.is_empty());
        assert_ne!(first, second);
        Ok(())
    }

    #[test]
    fn test_socket_override_is_scoped_per_named_thread() -> Result<(), Box<dyn std::error::Error>> {
        let first = std::thread::Builder::new()
            .name("override-scope-one".to_string())
            .spawn(|| -> Result<String> {
                set_socket_override("tenex-mux-override-one")?;
                Ok(socket_endpoint()?.display)
            })?
            .join()
            .map_err(|_| anyhow!("First override thread panicked"))??;
        let second = std::thread::Builder::new()
            .name("override-scope-two".to_string())
            .spawn(|| -> Result<String> {
                set_socket_override("tenex-mux-override-two")?;
                Ok(socket_endpoint()?.display)
            })?
            .join()
            .map_err(|_| anyhow!("Second override thread panicked"))??;

        assert_eq!(first, "tenex-mux-override-one");
        assert_eq!(second, "tenex-mux-override-two");
        Ok(())
    }
}
