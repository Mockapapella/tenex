//! Exercises mux socket endpoint defaults in an integration test build.

use tenex::mux;

#[test]
fn test_socket_endpoint_default_is_used_when_env_is_removed_for_child_process()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::TempDir::new()?;
    let state_path = temp.path().join("state.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env_remove("TENEX_MUX_SOCKET")
        .env("TENEX_STATE_PATH", &state_path)
        .output()?;
    assert!(
        output.status.success(),
        "mux endpoint probe failed: status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let display = String::from_utf8_lossy(&output.stdout);
    let display = display.trim();
    assert!(!display.is_empty());
    assert!(
        display.contains("tenex-mux"),
        "expected default socket display, got {display}",
    );

    let endpoint = mux::socket_endpoint()?;
    assert!(!endpoint.display.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_mux_endpoint_probe_exits_nonzero_when_stdout_write_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env("TENEX_MUX_SOCKET", "/tmp/tenex-mux-test\nsocket")
        .env("TENEX_TEST_MUX_ENDPOINT_PROBE_STDOUT_FAIL", "write")
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected mux_endpoint_probe to exit 1, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_mux_endpoint_probe_exits_nonzero_when_stdout_flush_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env("TENEX_MUX_SOCKET", "tenex-mux-test-name")
        .env("TENEX_TEST_MUX_ENDPOINT_PROBE_STDOUT_FAIL", "flush")
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected mux_endpoint_probe to exit 1, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(debug_assertions)]
fn test_mux_endpoint_probe_ignores_unknown_stdout_fail_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let socket_name = format!("tenex-mux-test-{}", std::process::id());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env("TENEX_MUX_SOCKET", &socket_name)
        .env("TENEX_TEST_MUX_ENDPOINT_PROBE_STDOUT_FAIL", "bogus")
        .output()?;
    assert!(
        output.status.success(),
        "expected mux_endpoint_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), socket_name);
    Ok(())
}

#[test]
fn test_mux_endpoint_probe_prints_env_override() -> Result<(), Box<dyn std::error::Error>> {
    let socket_name = format!("tenex-mux-test-{}", std::process::id());
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env("TENEX_MUX_SOCKET", &socket_name)
        .output()?;
    assert!(
        output.status.success(),
        "expected mux_endpoint_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, socket_name);
    Ok(())
}

#[test]
fn test_mux_endpoint_probe_exits_nonzero_when_socket_display_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::TempDir::new()?;
    let state_path = temp.path().join("state.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env("TENEX_STATE_PATH", &state_path)
        .env("TENEX_MUX_SOCKET", "tenex-mux-test-name")
        .env("TENEX_TEST_MUX_ENDPOINT_PROBE_FORCE_INVALID_SOCKET", "1")
        .output()?;
    assert!(
        !output.status.success(),
        "expected mux_endpoint_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn test_mux_endpoint_probe_exits_nonzero_when_socket_override_is_empty()
-> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env(
            "TENEX_TEST_MUX_ENDPOINT_PROBE_FORCE_INVALID_SOCKET",
            "empty",
        )
        .output()?;
    assert!(
        !output.status.success(),
        "expected mux_endpoint_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be empty"),
        "unexpected stderr: {stderr}"
    );
    Ok(())
}

#[test]
#[cfg(all(debug_assertions, target_os = "linux"))]
fn test_mux_endpoint_probe_exits_nonzero_when_namespaced_name_is_too_long()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::TempDir::new()?;
    let state_path = temp.path().join("state.json");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_endpoint_probe"))
        .env("TENEX_STATE_PATH", &state_path)
        .env("TENEX_TEST_MUX_ENDPOINT_PROBE_FORCE_INVALID_SOCKET", "namespaced-too-long")
        .output()?;
    assert!(
        !output.status.success(),
        "expected mux_endpoint_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to build namespaced mux socket name"),
        "unexpected stderr: {stderr}",
    );
    Ok(())
}
