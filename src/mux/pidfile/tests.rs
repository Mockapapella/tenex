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
fn test_list_sockets_in_returns_empty_for_missing_dir_and_skips_unrelated_entries() -> Result<()> {
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
