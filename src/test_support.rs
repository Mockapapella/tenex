use std::sync::{Mutex, MutexGuard, OnceLock};

fn mux_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
pub fn lock_mux_test_environment() -> MutexGuard<'static, ()> {
    mux_test_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
