#[cfg(test)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

fn mux_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn env_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub fn lock_mux_test_environment() -> MutexGuard<'static, ()> {
    mux_test_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub fn lock_env_test_environment() -> MutexGuard<'static, ()> {
    env_test_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub fn unique_mux_socket_path(tag: &str) -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tag = tag
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect::<String>();

    if tag.is_empty() {
        return format!("/tmp/tx-mux-{pid}-{counter}.sock");
    }

    format!("/tmp/tx-{tag}-{pid}-{counter}.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_env_test_environment_smoke() {
        let _guard = lock_env_test_environment();
    }

    #[test]
    fn test_unique_mux_socket_path_falls_back_when_tag_is_empty_after_filtering() {
        let path = unique_mux_socket_path("!!!");
        assert!(path.starts_with("/tmp/tx-mux-"));
        assert!(
            std::path::Path::new(&path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
        );
    }
}
