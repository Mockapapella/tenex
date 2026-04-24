//! Coverage tests for Codex session discovery helpers.

use std::path::{Path, PathBuf};

use chrono::{Datelike as _, Duration as ChronoDuration, Local, Utc};
use tempfile::TempDir;

fn codex_date_dir(sessions_root: &Path, date: chrono::NaiveDate) -> PathBuf {
    sessions_root
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{:02}", date.day()))
}

fn create_candidate_date_dirs(sessions_root: &Path) -> Vec<PathBuf> {
    let local_today = Local::now().date_naive();
    let utc_today = Utc::now().date_naive();

    let candidates = [
        local_today,
        local_today
            .checked_sub_signed(ChronoDuration::days(1))
            .unwrap_or(local_today),
        utc_today,
        utc_today
            .checked_sub_signed(ChronoDuration::days(1))
            .unwrap_or(utc_today),
    ];

    let mut unique: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for date in candidates {
        unique.insert(codex_date_dir(sessions_root, date));
    }
    unique.into_iter().collect()
}

fn run_probe(
    workdir: &Path,
    codex_home: Option<&Path>,
    env_removals: &[&str],
) -> std::io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"));
    cmd.arg(workdir);
    cmd.arg("0");

    for key in env_removals {
        cmd.env_remove(key);
    }

    if let Some(codex_home) = codex_home {
        cmd.env("CODEX_HOME", codex_home);
    } else {
        cmd.env_remove("CODEX_HOME");
    }

    cmd.output()
}

#[test]
fn test_codex_session_probe_usage_errors_exit_nonzero() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe")).output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage: codex_session_probe"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn test_codex_session_probe_exits_nonzero_for_invalid_max_wait()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"))
        .arg(workdir.path())
        .arg("nope")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn test_codex_session_probe_defaults_max_wait_when_missing()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"))
        .arg(workdir.path())
        .env_remove("CODEX_HOME")
        .env_remove("HOME")
        .output()?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "NONE\n");
    Ok(())
}

#[test]
fn test_detect_agent_cli_returns_other_on_parse_error() {
    assert_eq!(
        tenex::conversation::detect_agent_cli("sh -c 'unterminated"),
        tenex::conversation::AgentCli::Other
    );
}

#[test]
fn test_build_spawn_argv_errors_on_invalid_program() {
    assert!(tenex::conversation::build_spawn_argv("sh -c 'unterminated", None, None).is_err());
}

#[test]
fn test_build_resume_argv_errors_on_invalid_program() {
    assert!(tenex::conversation::build_resume_argv("sh -c 'unterminated", "abc").is_err());
}

#[test]
fn test_codex_session_probe_prints_none_when_home_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let workdir = TempDir::new()?;

    let output = run_probe(workdir.path(), None, &["HOME"])?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "NONE\n");

    Ok(())
}

#[test]
fn test_codex_session_probe_detects_session_from_codex_home()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;
    let codex_home = TempDir::new()?;
    let sessions_root = codex_home.path().join("sessions");

    for date_dir in create_candidate_date_dirs(&sessions_root) {
        std::fs::create_dir_all(&date_dir)?;
    }

    let wanted_cwd = workdir
        .path()
        .canonicalize()
        .unwrap_or_else(|_| workdir.path().to_path_buf());
    let session_id = "codex-session-123";
    let jsonl = serde_json::json!({
        "type": "session_meta",
        "payload": {
            "id": session_id,
            "cwd": wanted_cwd.to_string_lossy(),
        }
    });

    let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
    std::fs::create_dir_all(&date_dir)?;
    std::fs::write(date_dir.join("session.jsonl"), format!("{jsonl}\n"))?;

    let output = run_probe(workdir.path(), Some(codex_home.path()), &[])?;
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{session_id}\n")
    );

    Ok(())
}

#[test]
fn test_codex_session_probe_ignores_invalid_utf8_jsonl() -> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;
    let codex_home = TempDir::new()?;
    let sessions_root = codex_home.path().join("sessions");

    let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
    std::fs::create_dir_all(&date_dir)?;

    // Invalid UTF-8 in the first line should be treated as unreadable session metadata.
    std::fs::write(date_dir.join("invalid.jsonl"), b"\xff\xfe\xfd\n")?;

    let output = run_probe(workdir.path(), Some(codex_home.path()), &[])?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "NONE\n");

    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_codex_session_probe_exits_nonzero_when_stdout_write_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"))
        .arg(workdir.path())
        .arg("0")
        .env_remove("CODEX_HOME")
        .env_remove("HOME")
        .env("TENEX_TEST_CODEX_SESSION_PROBE_STDOUT_FAIL", "write")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_codex_session_probe_exits_nonzero_when_stdout_write_fails_for_detected_session()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;
    let codex_home = TempDir::new()?;
    let sessions_root = codex_home.path().join("sessions");

    let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
    std::fs::create_dir_all(&date_dir)?;

    let wanted_cwd = workdir
        .path()
        .canonicalize()
        .unwrap_or_else(|_| workdir.path().to_path_buf());
    let session_id = "codex-session-stdout-fails";
    let jsonl = serde_json::json!({
        "type": "session_meta",
        "payload": {
            "id": session_id,
            "cwd": wanted_cwd.to_string_lossy(),
        }
    });
    std::fs::write(date_dir.join("session.jsonl"), format!("{jsonl}\n"))?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"))
        .arg(workdir.path())
        .arg("0")
        .env("CODEX_HOME", codex_home.path())
        .env("TENEX_TEST_CODEX_SESSION_PROBE_STDOUT_FAIL", "write")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());

    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_codex_session_probe_exits_nonzero_when_stdout_flush_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"))
        .arg(workdir.path())
        .arg("0")
        .env_remove("CODEX_HOME")
        .env_remove("HOME")
        .env("TENEX_TEST_CODEX_SESSION_PROBE_STDOUT_FAIL", "flush")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());

    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_codex_session_probe_ignores_unknown_stdout_fail_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let workdir = TempDir::new()?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_codex_session_probe"))
        .arg(workdir.path())
        .arg("0")
        .env_remove("CODEX_HOME")
        .env_remove("HOME")
        .env("TENEX_TEST_CODEX_SESSION_PROBE_STDOUT_FAIL", "bogus")
        .output()?;
    assert!(
        output.status.success(),
        "expected codex_session_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "NONE\n");
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_codex_session_probe_ignores_unreadable_jsonl() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;

    let workdir = TempDir::new()?;
    let codex_home = TempDir::new()?;
    let sessions_root = codex_home.path().join("sessions");

    let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
    std::fs::create_dir_all(&date_dir)?;

    let path = date_dir.join("unreadable.jsonl");
    std::fs::write(&path, "{}\n")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))?;

    let output = run_probe(workdir.path(), Some(codex_home.path()), &[])?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "NONE\n");

    Ok(())
}
