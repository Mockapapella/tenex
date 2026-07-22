//! IPC endpoint naming for the mux daemon.

use crate::config::Config;
use anyhow::{Context, Result, bail};
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, Name, prelude::*};

use std::path::{Path, PathBuf};
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

fn fs_socket_endpoint(socket_path: PathBuf) -> Result<SocketEndpoint> {
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

fn state_dir_socket_endpoint(state_path: &Path, socket_name: &str) -> Result<SocketEndpoint> {
    let dir = state_path
        .parent()
        .context("State path has no parent directory")?;
    let socket_path = dir.join(format!("{socket_name}.sock"));
    fs_socket_endpoint(socket_path)
}

static SOCKET_OVERRIDE: OnceLock<String> = OnceLock::new();

static DEFAULT_SOCKET_NAME: OnceLock<String> = OnceLock::new();

const DEFAULT_SOCKET_PREFIX: &str = "tenex-mux";

/// Override the mux daemon socket for this process.
///
/// The override must be set before the first mux request. Otherwise the existing endpoint choice
/// is already cached.
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
    if let Some(override_value) = SOCKET_OVERRIDE.get() {
        return socket_endpoint_from_value(override_value);
    }

    if let Some(raw) = tenex_mux_socket_env() {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return socket_endpoint_from_value(trimmed);
        }
    }

    if namespaced_supported() {
        let display = default_socket_name();
        return socket_endpoint_from_value(&display);
    }

    let socket_name = default_socket_name();
    state_dir_socket_endpoint(&Config::state_path(), &socket_name)
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
            return fs_socket_endpoint(socket_path);
        }
    }

    if namespaced_supported() {
        if display.contains('\0') {
            bail!("Mux socket name cannot contain interior NUL bytes");
        }

        #[cfg(windows)]
        let name = to_namespaced_name(display.as_str())?;

        #[cfg(not(windows))]
        let name = to_namespaced_name(display.as_str())
            .context("Failed to build namespaced mux socket name")?;
        return Ok(SocketEndpoint {
            name,
            cleanup_path: None,
            display,
        });
    }

    state_dir_socket_endpoint(&Config::state_path(), &display)
}

fn default_socket_name() -> String {
    default_socket_name_cached()
}

fn default_socket_name_cached() -> String {
    DEFAULT_SOCKET_NAME
        .get_or_init(|| format!("{DEFAULT_SOCKET_PREFIX}-{}", socket_fingerprint()))
        .clone()
}

fn namespaced_supported() -> bool {
    GenericNamespaced::is_supported()
}

fn tenex_mux_socket_env() -> Option<String> {
    std::env::var("TENEX_MUX_SOCKET").ok()
}

fn to_namespaced_name_io(value: &str) -> std::io::Result<Name<'_>> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        const MAX_LEN: usize = 107;
        let len = value.len();
        if len > MAX_LEN {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Namespaced mux socket name is too long: {len} bytes, max {MAX_LEN}"),
            ));
        }
    }

    value.to_ns_name::<GenericNamespaced>()
}

fn to_namespaced_name(value: &str) -> Result<Name<'static>> {
    let name = to_namespaced_name_io(value)?.into_owned();
    Ok(name)
}

fn socket_fingerprint() -> String {
    socket_fingerprint_impl().unwrap_or_else(fallback_socket_fingerprint)
}

fn fallback_socket_fingerprint() -> String {
    "0000000000000000".to_string()
}

fn socket_fingerprint_impl() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let metadata = std::fs::metadata(exe).ok()?;

    let len = metadata.len();
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
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
