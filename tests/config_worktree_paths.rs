//! Exercises config worktree path helpers in an integration test build.

use std::path::Path;

use tenex::config::Config;

const HOME_NONE_CHILD_FLAG: &str = "TENEX_CONFIG_TEST_HOME_NONE_CHILD";

#[test]
fn test_worktree_dir_for_repo_root_defaults_to_project_when_repo_root_has_no_file_name() {
    let mut config = Config::default();
    let worktree_dir = std::env::temp_dir().join("tenex-test-worktrees");
    config.worktree_dir = worktree_dir.clone();

    assert_eq!(
        config.worktree_dir_for_repo_root(Path::new("")),
        worktree_dir.join("project")
    );
}

#[test]
fn test_generate_branch_name_truncates_long_titles_in_integration_build() {
    let config = Config::default();
    let long_title = "a".repeat(100);

    assert_eq!(
        config.generate_branch_name(&long_title),
        format!("{}{}", config.branch_prefix, "a".repeat(50))
    );
}

#[test]
fn test_default_instance_root_falls_back_when_home_missing()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os(HOME_NONE_CHILD_FLAG).is_some() {
        assert_eq!(
            Config::default_instance_root(),
            std::path::PathBuf::from(".").join(".tenex")
        );
        return Ok(());
    }

    let current_exe = std::env::current_exe()?;
    let output = std::process::Command::new(current_exe)
        .arg("--exact")
        .arg("test_default_instance_root_falls_back_when_home_missing")
        .arg("--nocapture")
        .env(HOME_NONE_CHILD_FLAG, "1")
        .env_remove("HOME")
        .output()?;

    assert!(
        output.status.success(),
        "child test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}
