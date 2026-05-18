//! Exercises action dispatch paths in a non-test build via `action_dispatch_probe`.

use std::process::Command;

#[cfg(coverage)]
#[test]
fn test_agent_lifecycle_coverage_probe_runs_in_non_test_build()
-> Result<(), Box<dyn std::error::Error>> {
    let state = tempfile::NamedTempFile::new()?;
    let storage = tenex::agent::Storage::with_path(state.path().to_path_buf());
    let config = tenex::Config::default();
    let settings = tenex::app::Settings::default();
    let mut app = tenex::App::new(config, storage, settings, false);

    let _ = tenex::mux::set_socket_override("tenex-action-dispatch-probe\0invalid");
    tenex::agent::Storage::exercise_load_and_backfill_paths_for_coverage();
    tenex::mux::exercise_endpoint_paths_for_coverage();
    tenex::mux::exercise_mux_paths_for_coverage();
    tenex::conversation::exercise_agent_cli_detection_for_coverage();
    app.data.exercise_command_defaults_for_coverage();
    tenex::app::Actions::exercise_reset_all_paths_for_coverage();
    tenex::app::Actions::exercise_agent_lifecycle_paths_for_coverage(&mut app.data);
    tenex::app::Actions::exercise_swarm_paths_for_coverage(&mut app.data);
    tenex::app::Actions::exercise_sync_paths_for_coverage(&mut app);
    Ok(())
}

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
