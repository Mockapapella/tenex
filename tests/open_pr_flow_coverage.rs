//! Integration coverage for the open PR flow.
//!
//! This test runs in an integration target so it exercises the non-`cfg(test)` instantiation of
//! the open PR handler code without relying on network access or a real `gh` binary.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn run_git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;

    assert!(
        output.status.success(),
        "git {args:?} failed (stdout: {}, stderr: {})",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

fn init_repo(repo: &Path) -> Result<(), Box<dyn std::error::Error>> {
    run_git(repo, &["init"])?;
    run_git(repo, &["config", "user.name", "Test"])?;
    run_git(repo, &["config", "user.email", "test@test.com"])?;
    Ok(())
}

fn add_bare_origin(repo: &Path) -> Result<TempDir, Box<dyn std::error::Error>> {
    let remote_dir = TempDir::new()?;
    run_git(remote_dir.path(), &["init", "--bare"])?;
    let remote = remote_dir.path().to_string_lossy().into_owned();
    run_git(repo, &["remote", "add", "origin", &remote])?;
    Ok(remote_dir)
}

fn commit_file(
    repo: &Path,
    name: &str,
    contents: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = repo.join(name);
    std::fs::write(&path, contents)?;
    run_git(repo, &["add", name])?;
    run_git(repo, &["commit", "-m", message])?;
    Ok(())
}

fn find_program_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[test]
fn test_open_pr_flow_probe_usage_errors_exit_nonzero() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe")).output()?;
    assert!(
        !output.status.success(),
        "expected open_pr_flow_probe to exit nonzero, got status={:?} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage: open_pr_flow_probe"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn test_open_pr_flow_probe_has_unpushed_prints_confirm_push_for_pr()
-> Result<(), Box<dyn std::error::Error>> {
    let repo_dir = TempDir::new()?;
    init_repo(repo_dir.path())?;
    let _remote_dir = add_bare_origin(repo_dir.path())?;
    commit_file(repo_dir.path(), "file.txt", "base\n", "base")?;
    run_git(repo_dir.path(), &["checkout", "-b", "feature/has-unpushed"])?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(repo_dir.path())
        .arg("feature/has-unpushed")
        .output()?;
    assert!(
        output.status.success(),
        "expected open_pr_flow_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "confirm-push-for-pr",
        "unexpected stdout: {stdout}"
    );
    Ok(())
}

#[test]
fn test_open_pr_flow_probe_exits_nonzero_when_open_pr_flow_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let missing = std::env::temp_dir().join(format!(
        "tenex-open-pr-probe-missing-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    assert!(!missing.exists());

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(missing)
        .arg("feature/missing-repo")
        .output()?;
    assert!(
        !output.status.success(),
        "expected open_pr_flow_probe to exit nonzero, got status={:?} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
    );
    assert!(!output.stderr.is_empty(), "expected stderr to be non-empty");
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_open_pr_flow_probe_without_unpushed_and_missing_gh_prints_error_modal()
-> Result<(), Box<dyn std::error::Error>> {
    let remote_dir = TempDir::new()?;
    run_git(remote_dir.path(), &["init", "--bare"])?;

    let repo_dir = TempDir::new()?;
    init_repo(repo_dir.path())?;
    commit_file(repo_dir.path(), "file.txt", "base\n", "base")?;
    run_git(repo_dir.path(), &["checkout", "-B", "master"])?;
    run_git(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            &remote_dir.path().to_string_lossy(),
        ],
    )?;
    run_git(repo_dir.path(), &["push", "-u", "origin", "master"])?;

    run_git(repo_dir.path(), &["checkout", "-b", "feature/no-unpushed"])?;
    run_git(
        repo_dir.path(),
        &["push", "-u", "origin", "feature/no-unpushed"],
    )?;

    let git_bin = find_program_in_path("git").ok_or("git not found on PATH")?;
    let bin_dir = TempDir::new()?;
    let git_link = bin_dir.path().join("git");

    std::os::unix::fs::symlink(&git_bin, &git_link)?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(repo_dir.path())
        .arg("feature/no-unpushed")
        .env("PATH", bin_dir.path())
        .output()?;
    assert!(
        output.status.success(),
        "expected open_pr_flow_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("error-modal\n"),
        "unexpected stdout: {stdout}"
    );
    assert!(stdout.contains("Failed to open PR"));
    assert!(stdout.contains("gh CLI not found"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_open_pr_flow_probe_without_unpushed_and_stub_gh_prints_other_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let remote_dir = TempDir::new()?;
    run_git(remote_dir.path(), &["init", "--bare"])?;

    let repo_dir = TempDir::new()?;
    init_repo(repo_dir.path())?;
    commit_file(repo_dir.path(), "file.txt", "base\n", "base")?;
    run_git(repo_dir.path(), &["checkout", "-B", "master"])?;
    run_git(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            &remote_dir.path().to_string_lossy(),
        ],
    )?;
    run_git(repo_dir.path(), &["push", "-u", "origin", "master"])?;

    run_git(repo_dir.path(), &["checkout", "-b", "feature/no-unpushed"])?;
    run_git(
        repo_dir.path(),
        &["push", "-u", "origin", "feature/no-unpushed"],
    )?;

    let git_bin = find_program_in_path("git").ok_or("git not found on PATH")?;
    let bin_dir = TempDir::new()?;
    let git_link = bin_dir.path().join("git");
    std::os::unix::fs::symlink(&git_bin, &git_link)?;

    let gh_path = bin_dir.path().join("gh");
    std::fs::write(&gh_path, "#!/bin/sh\nexit 0\n")?;
    let mut perms = std::fs::metadata(&gh_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&gh_path, perms)?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(repo_dir.path())
        .arg("feature/no-unpushed")
        .env("PATH", bin_dir.path())
        .output()?;
    assert!(
        output.status.success(),
        "expected open_pr_flow_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("other-mode\n"),
        "unexpected stdout: {stdout}"
    );
    assert!(stdout.contains("Normal"));

    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_open_pr_flow_probe_exits_nonzero_when_stdout_write_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let repo_dir = TempDir::new()?;
    init_repo(repo_dir.path())?;
    let _remote_dir = add_bare_origin(repo_dir.path())?;
    commit_file(repo_dir.path(), "file.txt", "base\n", "base")?;
    run_git(repo_dir.path(), &["checkout", "-b", "feature/has-unpushed"])?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(repo_dir.path())
        .arg("feature/has-unpushed")
        .env("TENEX_TEST_OPEN_PR_FLOW_PROBE_STDOUT_FAIL", "write")
        .output()?;
    assert!(
        !output.status.success(),
        "expected open_pr_flow_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_open_pr_flow_probe_exits_nonzero_when_stdout_flush_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let repo_dir = TempDir::new()?;
    init_repo(repo_dir.path())?;
    let _remote_dir = add_bare_origin(repo_dir.path())?;
    commit_file(repo_dir.path(), "file.txt", "base\n", "base")?;
    run_git(repo_dir.path(), &["checkout", "-b", "feature/has-unpushed"])?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(repo_dir.path())
        .arg("feature/has-unpushed")
        .env("TENEX_TEST_OPEN_PR_FLOW_PROBE_STDOUT_FAIL", "flush")
        .output()?;
    assert!(
        !output.status.success(),
        "expected open_pr_flow_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_open_pr_flow_probe_ignores_unknown_stdout_fail_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let repo_dir = TempDir::new()?;
    init_repo(repo_dir.path())?;
    let _remote_dir = add_bare_origin(repo_dir.path())?;
    commit_file(repo_dir.path(), "file.txt", "base\n", "base")?;
    run_git(repo_dir.path(), &["checkout", "-b", "feature/has-unpushed"])?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_open_pr_flow_probe"))
        .arg(repo_dir.path())
        .arg("feature/has-unpushed")
        .env("TENEX_TEST_OPEN_PR_FLOW_PROBE_STDOUT_FAIL", "bogus")
        .output()?;
    assert!(
        output.status.success(),
        "expected open_pr_flow_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "confirm-push-for-pr");
    Ok(())
}
