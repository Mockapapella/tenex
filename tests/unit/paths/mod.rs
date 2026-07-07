use std::ffi::OsString;
use std::path::PathBuf;
use tenex::paths::test_support as paths_support;
use tenex::paths::{data_local_dir, home_dir, log_path};

#[test]
fn test_log_path_suffix() {
    let path = log_path();
    assert!(path.ends_with("tenex.log"));
}

#[test]
fn test_data_local_dir_returns_when_env_present() {
    let has_xdg_data_home = std::env::var_os("XDG_DATA_HOME").is_some();
    let has_home = std::env::var_os("HOME").is_some();
    let has_env = has_xdg_data_home || has_home;
    let resolved = data_local_dir();
    assert_eq!(resolved.is_some(), has_env);
}

#[test]
fn test_home_dir_matches_home_env() {
    let expected = std::env::var_os("HOME").map(PathBuf::from);
    assert_eq!(home_dir(), expected);
}

#[test]
fn test_home_dir_from_reads_home() {
    let mut env = |key: &'static str| (key == "HOME").then(|| OsString::from("/tmp/tenex-home"));
    assert_eq!(
        paths_support::home_dir_from(&mut env),
        Some(PathBuf::from("/tmp/tenex-home"))
    );
}

#[test]
fn test_data_local_dir_from_prefers_xdg_data_home() {
    let mut env =
        |key: &'static str| (key == "XDG_DATA_HOME").then(|| OsString::from("/tmp/tenex-xdg"));

    assert_eq!(
        paths_support::data_local_dir_from(&mut env),
        Some(PathBuf::from("/tmp/tenex-xdg"))
    );
}

#[test]
fn test_data_local_dir_from_falls_back_to_home() {
    let mut env = |key: &'static str| (key == "HOME").then(|| OsString::from("/tmp/tenex-home"));

    let expected = PathBuf::from("/tmp/tenex-home")
        .join(".local")
        .join("share");

    assert_eq!(paths_support::data_local_dir_from(&mut env), Some(expected));
}

#[test]
fn test_data_local_dir_from_none_when_no_env() {
    let mut env = |_: &'static str| None::<OsString>;
    assert_eq!(paths_support::data_local_dir_from(&mut env), None);
}
