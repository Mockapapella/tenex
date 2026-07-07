use tenex::test_support::{
    lock_env_test_environment, lock_mux_test_environment, unique_mux_socket_path,
};

#[test]
fn test_lock_env_test_environment_smoke() {
    let _guard = lock_env_test_environment();
}

#[test]
fn test_lock_mux_test_environment_smoke() {
    let _guard = lock_mux_test_environment();
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

#[test]
fn test_unique_mux_socket_path_sanitizes_non_empty_tag() {
    let path = unique_mux_socket_path("abc-DEF-ghi-JKL");
    assert!(path.starts_with("/tmp/tx-abcDEFghiJKL-"));
    assert!(
        std::path::Path::new(&path)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("sock"))
    );
}
