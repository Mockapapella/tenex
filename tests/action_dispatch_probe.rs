//! Exercises action dispatch paths in a non-test build via `action_dispatch_probe`.

use std::process::Command;

#[test]
fn test_action_dispatch_probe_usage_errors_exit_nonzero() -> Result<(), Box<dyn std::error::Error>>
{
    let output = Command::new(env!("CARGO_BIN_EXE_action_dispatch_probe")).output()?;
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage: action_dispatch_probe"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
fn test_action_dispatch_probe_runs_all() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_action_dispatch_probe"))
        .arg("all")
        .output()?;
    assert!(
        output.status.success(),
        "expected probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_action_dispatch_probe_exits_nonzero_when_state_path_is_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_action_dispatch_probe"))
        .arg("all")
        .env("TENEX_TEST_ACTION_DISPATCH_PROBE_STATE_PATH_IS_DIR", "1")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_action_dispatch_probe_exits_nonzero_when_stdout_write_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_action_dispatch_probe"))
        .arg("all")
        .env("TENEX_TEST_ACTION_DISPATCH_PROBE_STDOUT_FAIL", "write")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_action_dispatch_probe_exits_nonzero_when_stdout_flush_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_action_dispatch_probe"))
        .arg("all")
        .env("TENEX_TEST_ACTION_DISPATCH_PROBE_STDOUT_FAIL", "flush")
        .output()?;
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_action_dispatch_probe_ignores_unknown_stdout_fail_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_action_dispatch_probe"))
        .arg("all")
        .env("TENEX_TEST_ACTION_DISPATCH_PROBE_STDOUT_FAIL", "bogus")
        .output()?;
    assert!(
        output.status.success(),
        "expected probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
    Ok(())
}
