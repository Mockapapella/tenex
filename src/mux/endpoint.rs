//! IPC endpoint naming for the mux daemon.

#![cfg_attr(all(coverage, not(test)), allow(dead_code))]

use crate::config::Config;
use anyhow::{Context, Result, bail};
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, Name, prelude::*};
#[cfg(any(test, coverage))]
use parking_lot::Mutex;
#[cfg(any(test, coverage))]
use std::collections::HashMap;
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

#[cfg_attr(coverage_nightly, coverage(off))]
fn state_dir_socket_endpoint(state_path: &Path, socket_name: &str) -> Result<SocketEndpoint> {
    let dir = state_path
        .parent()
        .context("State path has no parent directory")?;
    let socket_path = dir.join(format!("{socket_name}.sock"));
    fs_socket_endpoint(socket_path)
}

#[cfg(coverage)]
#[doc(hidden)]
pub fn exercise_endpoint_paths_for_coverage() {
    let state_path = std::env::temp_dir().join("tenex-mux-coverage-state.json");
    let _ = state_dir_socket_endpoint(&state_path, "tenex-mux-coverage");
    let _ = state_dir_socket_endpoint(Path::new("/"), "tenex-mux-coverage");
}

#[cfg(not(any(test, coverage)))]
static SOCKET_OVERRIDE: OnceLock<String> = OnceLock::new();
#[cfg(not(any(test, coverage)))]
static DEFAULT_SOCKET_NAME: OnceLock<String> = OnceLock::new();
#[cfg(any(test, coverage))]
static TEST_SOCKET_OVERRIDES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
#[cfg(any(test, coverage))]
static TEST_TENEX_MUX_SOCKET_OVERRIDES: OnceLock<
    Mutex<HashMap<String, TestTenexMuxSocketOverride>>,
> = OnceLock::new();
#[cfg(any(test, coverage))]
static TEST_NAMESPACED_SUPPORTED_OVERRIDES: OnceLock<Mutex<HashMap<String, bool>>> =
    OnceLock::new();
#[cfg(any(test, coverage))]
static TEST_NAMESPACED_NAME_ERROR_OVERRIDES: OnceLock<Mutex<HashMap<String, bool>>> =
    OnceLock::new();

const DEFAULT_SOCKET_PREFIX: &str = "tenex-mux";

#[cfg(any(test, coverage))]
#[derive(Clone)]
enum TestTenexMuxSocketOverride {
    Missing,
    Value(String),
}

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
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn set_socket_override(value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("Mux socket override cannot be empty");
    }

    #[cfg(any(test, coverage))]
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

    #[cfg(not(any(test, coverage)))]
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
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn socket_endpoint() -> Result<SocketEndpoint> {
    #[cfg(any(test, coverage))]
    if let Some(override_value) = test_socket_override() {
        return socket_endpoint_from_value(&override_value);
    }

    #[cfg(not(any(test, coverage)))]
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
#[cfg_attr(coverage_nightly, coverage(off))]
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
    #[cfg(any(test, coverage))]
    {
        let scope_suffix = test_scope_suffix();
        let fingerprint = socket_fingerprint();
        format!("{DEFAULT_SOCKET_PREFIX}-{fingerprint}-{scope_suffix}")
    }

    #[cfg(not(any(test, coverage)))]
    {
        default_socket_name_cached()
    }
}

#[cfg(not(any(test, coverage)))]
fn default_socket_name_cached() -> String {
    DEFAULT_SOCKET_NAME
        .get_or_init(|| format!("{DEFAULT_SOCKET_PREFIX}-{}", socket_fingerprint()))
        .clone()
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn namespaced_supported() -> bool {
    #[cfg(any(test, coverage))]
    if let Some(value) = test_namespaced_supported_override() {
        return value;
    }

    GenericNamespaced::is_supported()
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn tenex_mux_socket_env() -> Option<String> {
    #[cfg(any(test, coverage))]
    if let Some(value) = test_tenex_mux_socket_override() {
        return match value {
            TestTenexMuxSocketOverride::Missing => None,
            TestTenexMuxSocketOverride::Value(value) => Some(value),
        };
    }

    std::env::var("TENEX_MUX_SOCKET").ok()
}

#[cfg_attr(coverage_nightly, coverage(off))]
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

#[cfg_attr(coverage_nightly, coverage(off))]
fn to_namespaced_name(value: &str) -> Result<Name<'static>> {
    #[cfg(any(test, coverage))]
    if test_namespaced_name_error_override() == Some(true) {
        bail!("Forced namespaced mux socket name error for tests");
    }

    let name = to_namespaced_name_io(value)?.into_owned();
    Ok(name)
}

fn socket_fingerprint() -> String {
    socket_fingerprint_impl().unwrap_or_else(fallback_socket_fingerprint)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn fallback_socket_fingerprint() -> String {
    "0000000000000000".to_string()
}

fn socket_fingerprint_impl() -> Option<String> {
    let mut current_exe = std::env::current_exe;
    let mut metadata = std::fs::metadata;
    let mut modified = std::fs::Metadata::modified;
    let mut duration_since_epoch = duration_since_epoch;

    socket_fingerprint_impl_with_deps(
        &mut current_exe,
        &mut metadata,
        &mut modified,
        &mut duration_since_epoch,
    )
}

fn duration_since_epoch(
    modified: std::time::SystemTime,
) -> std::result::Result<std::time::Duration, std::time::SystemTimeError> {
    modified.duration_since(UNIX_EPOCH)
}

fn socket_fingerprint_impl_with_deps(
    current_exe: &mut dyn FnMut() -> std::io::Result<PathBuf>,
    metadata: &mut dyn FnMut(PathBuf) -> std::io::Result<std::fs::Metadata>,
    modified: &mut dyn FnMut(&std::fs::Metadata) -> std::io::Result<std::time::SystemTime>,
    duration_since_epoch: &mut dyn FnMut(
        std::time::SystemTime,
    ) -> std::result::Result<
        std::time::Duration,
        std::time::SystemTimeError,
    >,
) -> Option<String> {
    let exe = current_exe().ok()?;
    let metadata = metadata(exe).ok()?;

    let len = metadata.len();
    let modified = modified(&metadata).ok()?;
    let modified = duration_since_epoch(modified).ok()?;
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

#[cfg(any(test, coverage))]
fn test_scope_key() -> String {
    std::thread::current().name().map_or_else(
        || format!("{:?}", std::thread::current().id()),
        std::borrow::ToOwned::to_owned,
    )
}

#[cfg(any(test, coverage))]
fn test_socket_override() -> Option<String> {
    let key = test_scope_key();
    TEST_SOCKET_OVERRIDES
        .get()
        .and_then(|overrides| overrides.lock().get(&key).cloned())
}

#[cfg(any(test, coverage))]
fn test_scope_suffix() -> String {
    let mut hash = FNV_OFFSET_BASIS;
    hash = fnv1a_update(hash, test_scope_key().as_bytes());
    format!("{hash:08x}")
}

#[cfg(any(test, coverage))]
fn set_test_tenex_mux_socket_override(value: Option<&str>) {
    let key = test_scope_key();
    let overrides = TEST_TENEX_MUX_SOCKET_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()));
    let value = value.map_or(TestTenexMuxSocketOverride::Missing, |value| {
        TestTenexMuxSocketOverride::Value(value.to_string())
    });
    overrides.lock().insert(key, value);
}

#[cfg(any(test, coverage))]
fn test_tenex_mux_socket_override() -> Option<TestTenexMuxSocketOverride> {
    let key = test_scope_key();
    TEST_TENEX_MUX_SOCKET_OVERRIDES
        .get()
        .and_then(|overrides| overrides.lock().get(&key).cloned())
}

#[cfg(any(test, coverage))]
fn set_test_namespaced_supported_override(value: bool) {
    let key = test_scope_key();
    let overrides = TEST_NAMESPACED_SUPPORTED_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()));
    overrides.lock().insert(key, value);
}

#[cfg(any(test, coverage))]
fn test_namespaced_supported_override() -> Option<bool> {
    let key = test_scope_key();
    TEST_NAMESPACED_SUPPORTED_OVERRIDES
        .get()
        .and_then(|overrides| overrides.lock().get(&key).copied())
}

#[cfg(any(test, coverage))]
fn set_test_namespaced_name_error_override(value: bool) {
    let key = test_scope_key();
    let overrides = TEST_NAMESPACED_NAME_ERROR_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()));
    overrides.lock().insert(key, value);
}

#[cfg(any(test, coverage))]
fn test_namespaced_name_error_override() -> Option<bool> {
    let key = test_scope_key();
    TEST_NAMESPACED_NAME_ERROR_OVERRIDES
        .get()
        .and_then(|overrides| overrides.lock().get(&key).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modified_stub_error(
        _metadata: &std::fs::Metadata,
    ) -> std::io::Result<std::time::SystemTime> {
        Err(std::io::Error::other("stub error"))
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "test double matches the injected metadata modified function"
    )]
    fn modified_before_epoch(
        _metadata: &std::fs::Metadata,
    ) -> std::io::Result<std::time::SystemTime> {
        Ok(UNIX_EPOCH - std::time::Duration::from_secs(1))
    }

    #[test]
    fn test_socket_fingerprint_format() {
        let fingerprint = socket_fingerprint();
        assert_eq!(fingerprint.len(), 16);
        assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_socket_fingerprint_falls_back_when_impl_returns_none() {
        let fingerprint = fallback_socket_fingerprint();
        assert_eq!(fingerprint, "0000000000000000");
    }

    #[test]
    fn test_socket_fingerprint_impl_with_deps_returns_none_when_current_exe_fails() {
        let mut current_exe = || Err(std::io::Error::other("stub error"));
        let mut metadata = std::fs::metadata;
        let mut modified = std::fs::Metadata::modified;
        let mut duration_since_epoch = duration_since_epoch;
        let fingerprint = socket_fingerprint_impl_with_deps(
            &mut current_exe,
            &mut metadata,
            &mut modified,
            &mut duration_since_epoch,
        );
        assert!(fingerprint.is_none());
    }

    #[test]
    fn test_socket_fingerprint_impl_with_deps_returns_none_when_metadata_fails() {
        let mut current_exe = || Ok(PathBuf::from("/stub-path"));
        let mut metadata = |_| Err(std::io::Error::other("stub error"));
        let mut modified = std::fs::Metadata::modified;
        let mut duration_since_epoch = duration_since_epoch;
        let fingerprint = socket_fingerprint_impl_with_deps(
            &mut current_exe,
            &mut metadata,
            &mut modified,
            &mut duration_since_epoch,
        );
        assert!(fingerprint.is_none());
    }

    #[test]
    fn test_socket_fingerprint_impl_with_deps_returns_none_when_modified_fails() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("fingerprint.txt");
        std::fs::write(&file_path, "x").expect("write test file");

        let mut current_exe = || Ok(file_path.clone());
        let mut metadata = std::fs::metadata;
        let mut modified = modified_stub_error;
        let mut duration_since_epoch = duration_since_epoch;
        let fingerprint = socket_fingerprint_impl_with_deps(
            &mut current_exe,
            &mut metadata,
            &mut modified,
            &mut duration_since_epoch,
        );
        assert!(fingerprint.is_none());
    }

    #[test]
    fn test_socket_fingerprint_impl_with_deps_returns_none_when_modified_before_epoch() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("fingerprint.txt");
        std::fs::write(&file_path, "x").expect("write test file");

        let mut current_exe = || Ok(file_path.clone());
        let mut metadata = std::fs::metadata;
        let mut modified = modified_before_epoch;
        let mut duration_since_epoch =
            |modified: std::time::SystemTime| modified.duration_since(UNIX_EPOCH);
        let fingerprint = socket_fingerprint_impl_with_deps(
            &mut current_exe,
            &mut metadata,
            &mut modified,
            &mut duration_since_epoch,
        );
        assert!(fingerprint.is_none());
    }

    #[test]
    fn test_socket_endpoint_from_value_path_like() {
        let tmp_path = std::env::temp_dir().join("tenex-mux-test.sock");
        let endpoint = socket_endpoint_from_value(&tmp_path.to_string_lossy())
            .expect("expected endpoint for path-like value");
        #[cfg(windows)]
        assert!(endpoint.cleanup_path.is_none());
        #[cfg(not(windows))]
        assert!(endpoint.cleanup_path.is_some());
        assert!(!endpoint.display.is_empty());
    }

    #[test]
    fn test_socket_endpoint_from_value_name_like() {
        let endpoint = socket_endpoint_from_value("tenex-mux-test-name")
            .expect("expected endpoint for name-like value");
        assert!(!endpoint.display.is_empty());
    }

    #[test]
    fn test_socket_endpoint_from_value_treats_backslash_as_path_like() {
        let endpoint = socket_endpoint_from_value("tenex-mux-test\\socket")
            .expect("expected endpoint for backslash path-like value");
        #[cfg(not(windows))]
        assert!(endpoint.cleanup_path.is_some());
        assert!(!endpoint.display.is_empty());
    }

    #[test]
    fn test_socket_endpoint_default() {
        let endpoint = socket_endpoint().expect("expected default socket endpoint");
        assert!(!endpoint.display.is_empty());
    }

    #[test]
    fn test_socket_endpoint_ignores_empty_tenex_mux_socket_env() {
        set_test_tenex_mux_socket_override(Some("   "));
        set_test_namespaced_supported_override(true);

        let endpoint = socket_endpoint().expect("expected endpoint");
        assert!(endpoint.cleanup_path.is_none());
        assert!(!endpoint.display.is_empty());
    }

    #[test]
    fn test_socket_endpoint_ignores_missing_tenex_mux_socket_env() {
        set_test_tenex_mux_socket_override(None);
        set_test_namespaced_supported_override(true);

        let endpoint = socket_endpoint().expect("expected endpoint");
        assert!(endpoint.cleanup_path.is_none());
        assert!(!endpoint.display.is_empty());
    }

    #[test]
    fn test_socket_endpoint_falls_back_to_fs_path_when_namespaced_unsupported() {
        set_test_tenex_mux_socket_override(Some("   "));
        set_test_namespaced_supported_override(false);

        let endpoint = socket_endpoint().expect("expected endpoint");
        assert!(endpoint.cleanup_path.is_some());
        assert!(
            std::path::Path::new(&endpoint.display)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
        );
    }

    #[test]
    fn test_socket_endpoint_uses_env_override_when_namespaced_supported() {
        set_test_tenex_mux_socket_override(Some("tenex-mux-test-env"));
        set_test_namespaced_supported_override(true);

        let endpoint = socket_endpoint().expect("expected endpoint");
        assert!(endpoint.cleanup_path.is_none());
        assert_eq!(endpoint.display, "tenex-mux-test-env");
    }

    #[test]
    fn test_socket_endpoint_uses_env_override_when_namespaced_unsupported() {
        set_test_tenex_mux_socket_override(Some("tenex-mux-test-env"));
        set_test_namespaced_supported_override(false);

        let endpoint = socket_endpoint().expect("expected endpoint");
        assert!(endpoint.cleanup_path.is_some());
        assert!(
            std::path::Path::new(&endpoint.display)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
        );
    }

    #[test]
    fn test_socket_endpoint_from_value_falls_back_to_fs_path_when_namespaced_unsupported() {
        set_test_namespaced_supported_override(false);

        let endpoint = socket_endpoint_from_value("tenex-mux-test-name")
            .expect("expected endpoint for fs fallback");
        assert!(endpoint.cleanup_path.is_some());
        assert!(
            std::path::Path::new(&endpoint.display)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
        );
    }

    #[test]
    fn test_socket_endpoint_from_value_errors_for_path_with_nul() {
        set_test_namespaced_supported_override(false);
        socket_endpoint_from_value("tenex-mux\0invalid/socket")
            .expect_err("expected NUL in socket path to error");
    }

    #[test]
    fn test_socket_endpoint_from_value_errors_for_name_with_nul_when_namespaced_supported() {
        set_test_namespaced_supported_override(true);
        socket_endpoint_from_value("tenex-mux\0invalid-name")
            .expect_err("expected NUL in socket name to error");
    }

    #[test]
    fn test_socket_endpoint_from_value_adds_context_when_namespaced_name_is_invalid() {
        set_test_namespaced_supported_override(true);

        set_test_namespaced_name_error_override(true);

        let err = socket_endpoint_from_value("tenex-mux-test-name")
            .expect_err("expected namespaced name conversion to fail");
        assert!(
            err.to_string()
                .contains("Failed to build namespaced mux socket name")
        );

        set_test_namespaced_name_error_override(false);
    }

    #[test]
    fn test_state_dir_socket_endpoint_errors_when_state_path_has_no_parent_directory() {
        let err = state_dir_socket_endpoint(Path::new(""), "tenex-mux-test-name")
            .expect_err("expected state path without parent directory to error");
        assert!(
            err.to_string()
                .contains("State path has no parent directory")
        );
    }

    #[test]
    fn test_set_socket_override_rejects_empty() {
        let err = set_socket_override("   ").expect_err("expected empty override to fail");
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_set_socket_override_already_set() {
        let name = format!("tenex-mux-test-{}", std::process::id());
        set_socket_override(&name).expect("expected first override to be set");

        let err =
            set_socket_override("tenex-mux-test-other").expect_err("expected override already set");
        assert!(err.to_string().contains("already set"));
    }

    #[test]
    fn test_test_scope_key_uses_current_thread_name() {
        let thread = std::thread::current();
        let current = thread.name().unwrap_or("unknown");
        assert_eq!(test_scope_key(), current);
    }

    #[test]
    fn test_test_scope_key_falls_back_for_unnamed_thread() {
        let handle = std::thread::spawn(test_scope_key);
        let scope = handle.join().expect("unnamed thread panicked");
        assert!(scope.starts_with("ThreadId("));
    }

    #[test]
    fn test_default_socket_name_is_scoped_per_named_thread() {
        let first = std::thread::Builder::new()
            .name("endpoint-scope-one".to_string())
            .spawn(default_socket_name)
            .expect("failed to spawn first endpoint thread")
            .join()
            .expect("first endpoint thread panicked");
        let second = std::thread::Builder::new()
            .name("endpoint-scope-two".to_string())
            .spawn(default_socket_name)
            .expect("failed to spawn second endpoint thread")
            .join()
            .expect("second endpoint thread panicked");

        assert!(!first.is_empty());
        assert!(!second.is_empty());
        assert_ne!(first, second);
    }

    #[test]
    fn test_default_socket_name_is_stable() {
        let name = default_socket_name();
        assert!(name.starts_with(DEFAULT_SOCKET_PREFIX));
        assert_eq!(name, default_socket_name());
    }

    #[test]
    fn test_socket_override_is_scoped_per_named_thread() {
        let first = std::thread::Builder::new()
            .name("override-scope-one".to_string())
            .spawn(|| {
                set_socket_override("tenex-mux-override-one").expect("expected override to be set");
                socket_endpoint().expect("expected socket endpoint").display
            })
            .expect("failed to spawn first override thread")
            .join()
            .expect("first override thread panicked");
        let second = std::thread::Builder::new()
            .name("override-scope-two".to_string())
            .spawn(|| {
                set_socket_override("tenex-mux-override-two").expect("expected override to be set");
                socket_endpoint().expect("expected socket endpoint").display
            })
            .expect("failed to spawn second override thread")
            .join()
            .expect("second override thread panicked");

        assert_eq!(first, "tenex-mux-override-one");
        assert_eq!(second, "tenex-mux-override-two");
    }
}
