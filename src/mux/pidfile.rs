//! Mux daemon pid file helpers.
//!
//! Tenex writes a small pid file per mux socket so non-Linux platforms can
//! locate and terminate the daemon without relying on `/proc`.

use crate::config::Config;
use anyhow::{Context, Result};
#[cfg(test)]
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::OnceLock;

#[cfg(test)]
static TEST_INSTANCE_ROOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MuxPidFile {
    pid: u32,
    socket: String,
}

#[derive(Debug)]
pub(super) struct PidFileGuard {
    path: PathBuf,
}

impl PidFileGuard {
    pub(super) fn create(socket: &str) -> Result<Self> {
        Self::create_for_pid(socket, std::process::id())
    }

    pub(super) fn create_for_pid(socket: &str, pid: u32) -> Result<Self> {
        Self::create_for_pid_in(&current_instance_root(), socket, pid)
    }

    fn create_for_pid_in(instance_root: &Path, socket: &str, pid: u32) -> Result<Self> {
        let socket = socket.trim();
        anyhow::ensure!(!socket.is_empty(), "mux socket cannot be empty");

        fs::create_dir_all(instance_root).with_context(|| {
            format!(
                "Failed to create Tenex instance directory {}",
                instance_root.display()
            )
        })?;

        let path = pid_file_path_for_socket(instance_root, socket);
        let pid_file = MuxPidFile {
            pid,
            socket: socket.to_string(),
        };

        write_atomically(&path, &serde_json::to_vec(&pid_file)?)?;

        Ok(Self { path })
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn read_pid(socket: &str) -> Option<u32> {
    read_pid_in(&current_instance_root(), socket)
}

#[cfg(not(target_os = "linux"))]
pub(super) fn list_sockets() -> Vec<String> {
    list_sockets_in(&current_instance_root())
}

#[cfg(not(target_os = "linux"))]
pub(super) fn remove(socket: &str) {
    remove_in(&current_instance_root(), socket);
}

const PID_FILE_PREFIX: &str = "tenex-muxd-";
const PID_FILE_SUFFIX: &str = ".pid";

fn pid_file_path_for_socket(instance_root: &Path, socket: &str) -> PathBuf {
    let hash = fnv1a_64(socket.as_bytes());
    instance_root.join(format!("{PID_FILE_PREFIX}{hash:016x}{PID_FILE_SUFFIX}"))
}

fn current_instance_root() -> PathBuf {
    #[cfg(test)]
    if let Some(root) = test_instance_root() {
        return root;
    }

    Config::instance_root()
}

#[cfg(not(target_os = "linux"))]
fn read_pid_in(instance_root: &Path, socket: &str) -> Option<u32> {
    let socket = socket.trim();
    if socket.is_empty() {
        return None;
    }

    let path = pid_file_path_for_socket(instance_root, socket);
    let raw = fs::read(&path).ok()?;
    let pid_file: MuxPidFile = serde_json::from_slice(&raw).ok()?;
    if pid_file.socket.trim() != socket {
        return None;
    }
    Some(pid_file.pid)
}

#[cfg(not(target_os = "linux"))]
fn list_sockets_in(instance_root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(instance_root) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }

            let name = path.file_name()?.to_string_lossy();
            if !name.starts_with(PID_FILE_PREFIX) || !name.ends_with(PID_FILE_SUFFIX) {
                return None;
            }

            let raw = fs::read(&path).ok()?;
            let pid_file: MuxPidFile = serde_json::from_slice(&raw).ok()?;
            let socket = pid_file.socket.trim();
            if socket.is_empty() {
                return None;
            }
            Some(socket.to_string())
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn remove_in(instance_root: &Path, socket: &str) {
    let socket = socket.trim();
    if socket.is_empty() {
        return;
    }

    let path = pid_file_path_for_socket(instance_root, socket);
    let _ = fs::remove_file(path);
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let write_result = (|| -> Result<()> {
        fs::write(&tmp, contents)
            .with_context(|| format!("Failed to write mux pid file {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| {
            format!(
                "Failed to replace mux pid file {} with {}",
                path.display(),
                tmp.display()
            )
        })?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
    }

    write_result
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
fn test_instance_root() -> Option<PathBuf> {
    TEST_INSTANCE_ROOT
        .get()
        .and_then(|slot| slot.lock().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, anyhow};

    struct InstanceRootGuard {
        root: PathBuf,
    }

    impl InstanceRootGuard {
        fn set(root: PathBuf) -> Self {
            let slot = TEST_INSTANCE_ROOT.get_or_init(|| Mutex::new(None));
            *slot.lock() = Some(root.clone());
            Self { root }
        }
    }

    impl Drop for InstanceRootGuard {
        fn drop(&mut self) {
            if let Some(slot) = TEST_INSTANCE_ROOT.get() {
                *slot.lock() = None;
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn with_instance_root<T>(test: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
        let _guard = crate::test_support::lock_mux_test_environment();
        let root = std::env::temp_dir().join(format!(
            "tenex-pidfile-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root)?;
        let instance_root_guard = InstanceRootGuard::set(root.clone());
        let result = test(&root);
        drop(instance_root_guard);
        result
    }

    #[test]
    fn test_create_for_pid_round_trips_pidfile_state() -> Result<()> {
        with_instance_root(|instance_root| {
            let socket = format!("tenex-mux-test-{}", uuid::Uuid::new_v4());
            let pid = 4242;

            let guard = PidFileGuard::create_for_pid_in(instance_root, &socket, pid)?;
            let path = pid_file_path_for_socket(instance_root, &socket);
            assert!(path.is_file());
            assert_eq!(read_pid_in(instance_root, &socket), Some(pid));
            assert!(list_sockets_in(instance_root).contains(&socket));

            remove_in(instance_root, &socket);
            assert!(read_pid_in(instance_root, &socket).is_none());
            assert!(!list_sockets_in(instance_root).contains(&socket));

            drop(guard);
            Ok(())
        })
    }

    #[test]
    fn test_create_for_pid_rejects_empty_socket() -> Result<(), Box<dyn std::error::Error>> {
        with_instance_root(|instance_root| {
            match PidFileGuard::create_for_pid_in(instance_root, "   ", 7) {
                Ok(_) => Err(anyhow!("Expected empty socket to fail")),
                Err(err) => {
                    assert!(err.to_string().contains("cannot be empty"));
                    Ok(())
                }
            }
        })
        .map_err(Into::into)
    }

    #[test]
    fn test_read_pid_and_list_sockets_skip_invalid_payloads() -> Result<()> {
        with_instance_root(|instance_root| {
            fs::create_dir_all(instance_root)?;

            let socket = "tenex-mux-invalid";
            let path = pid_file_path_for_socket(instance_root, socket);
            let invalid_payload = MuxPidFile {
                pid: 99,
                socket: " ".to_string(),
            };
            fs::write(path, serde_json::to_vec(&invalid_payload)?)?;

            assert!(read_pid_in(instance_root, socket).is_none());
            assert!(list_sockets_in(instance_root).is_empty());
            Ok(())
        })
    }

    #[test]
    fn test_read_pid_in_returns_none_for_empty_socket_and_invalid_json() -> Result<()> {
        with_instance_root(|instance_root| {
            assert!(read_pid_in(instance_root, "   ").is_none());

            let socket = "tenex-mux-invalid-json";
            let path = pid_file_path_for_socket(instance_root, socket);
            fs::write(path, b"not-json")?;

            assert!(read_pid_in(instance_root, socket).is_none());
            Ok(())
        })
    }

    #[test]
    fn test_list_sockets_in_returns_empty_for_missing_dir_and_skips_unrelated_entries() -> Result<()>
    {
        with_instance_root(|instance_root| {
            fs::remove_dir_all(instance_root)?;
            assert!(list_sockets_in(instance_root).is_empty());

            fs::create_dir_all(instance_root)?;
            fs::create_dir(instance_root.join("nested"))?;
            fs::write(instance_root.join("notes.txt"), b"ignore me")?;

            let socket = "tenex-mux-valid";
            let valid_path = pid_file_path_for_socket(instance_root, socket);
            let valid_payload = MuxPidFile {
                pid: 77,
                socket: socket.to_string(),
            };
            fs::write(valid_path, serde_json::to_vec(&valid_payload)?)?;

            let sockets = list_sockets_in(instance_root);
            assert_eq!(sockets, vec![socket.to_string()]);
            Ok(())
        })
    }

    #[test]
    fn test_remove_in_handles_empty_and_existing_socket_entries() -> Result<()> {
        with_instance_root(|instance_root| {
            let socket = "tenex-mux-remove";
            let guard = PidFileGuard::create_for_pid_in(instance_root, socket, 1234)?;
            let path = pid_file_path_for_socket(instance_root, socket);
            assert!(path.is_file());

            remove_in(instance_root, "   ");
            assert!(path.is_file());

            remove_in(instance_root, socket);
            assert!(!path.exists());

            drop(guard);
            Ok(())
        })
    }

    #[test]
    fn test_public_pidfile_api_uses_overridden_instance_root() -> Result<()> {
        with_instance_root(|instance_root| {
            let socket = "tenex-mux-public-api";
            let guard = PidFileGuard::create(socket)?;
            let path = pid_file_path_for_socket(instance_root, socket);

            assert!(path.is_file());
            assert_eq!(read_pid(socket), Some(std::process::id()));
            assert!(list_sockets().contains(&socket.to_string()));

            remove(socket);
            assert!(!path.exists());

            drop(guard);
            Ok(())
        })
    }

    #[test]
    fn test_pid_file_path_changes_with_socket_value() {
        let root = Path::new("/tmp/tenex-pidfile-tests");
        let first = pid_file_path_for_socket(root, "socket-a");
        let second = pid_file_path_for_socket(root, "socket-b");

        assert_ne!(first, second);
        assert_eq!(first, pid_file_path_for_socket(root, "socket-a"));
    }
}
