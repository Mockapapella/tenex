//! Smoke test that runs the TUI entrypoint inside a PTY.

use portable_pty::{CommandBuilder, PtySize};
use std::io::{Read, Write};

fn run_tui_smoke_in_pty(
    extra_env: &[(&str, &str)],
) -> Result<(portable_pty::ExitStatus, Vec<u8>), Box<dyn std::error::Error>> {
    run_tui_smoke_in_pty_with_input_delay(extra_env, std::time::Duration::ZERO)
}

fn run_tui_smoke_in_pty_with_input_delay(
    extra_env: &[(&str, &str)],
    input_delay: std::time::Duration,
) -> Result<(portable_pty::ExitStatus, Vec<u8>), Box<dyn std::error::Error>> {
    let pty_system = portable_pty::native_pty_system();
    let portable_pty::PtyPair { master, slave } = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_tui_smoke"));
    cmd.env("TERM", "xterm-256color");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }

    let mut child = slave.spawn_command(cmd)?;
    drop(slave);

    let mut reader = master.try_clone_reader()?;
    let reader_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });

    if let Ok(mut writer) = master.take_writer() {
        if !input_delay.is_zero() {
            std::thread::sleep(input_delay);
        }
        let _ = writer.write_all(b"\x11");
        let _ = writer.flush();
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            let output = reader_handle.join().unwrap_or_default();
            return Ok((status, output));
        }

        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let output = reader_handle.join().unwrap_or_default();
            return Err(
                format!("tui_smoke timed out:\n{}", String::from_utf8_lossy(&output)).into(),
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn test_tui_smoke_binary_exits_successfully() -> Result<(), Box<dyn std::error::Error>> {
    let pty_system = portable_pty::native_pty_system();
    let portable_pty::PtyPair { master, slave } = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_tui_smoke"));
    cmd.env("TERM", "xterm-256color");

    let mut child = slave.spawn_command(cmd)?;
    drop(slave);

    let mut writer = master.take_writer()?;
    writer.write_all(b"q")?;
    writer.flush()?;

    let mut reader = master.try_clone_reader()?;

    let reader_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait()? {
            let output = reader_handle.join().unwrap_or_default();
            if status.success() {
                return Ok(());
            }
            let output = String::from_utf8_lossy(&output);
            return Err(format!("tui_smoke exited unsuccessfully:\n{output}").into());
        }

        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let output = reader_handle.join().unwrap_or_default();
            let output = String::from_utf8_lossy(&output);
            return Err(format!("tui_smoke timed out:\n{output}").into());
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_without_tty() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tui_smoke"))
        .env("TERM", "xterm-256color")
        .output()?;
    assert!(
        !output.status.success(),
        "expected tui_smoke to fail without a tty, got status={:?}",
        output.status.code()
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_stdout_is_not_tty()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_SMOKE_FORCE_STDOUT_NOT_TTY", "1")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when stdout is not a tty, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_state_write_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_SMOKE_FORCE_STATE_SAVE_ERROR", "1")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when state write fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    assert!(!output.is_empty());
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_tui_run_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "enable_raw_mode")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when TUI run fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_enter_tui_screen_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "enter_tui_screen")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when enter_tui_screen fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_terminal_creation_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "create_terminal")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when terminal creation fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_disable_raw_mode_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "disable_raw_mode")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when disable_raw_mode fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_leave_tui_screen_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) =
        run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "leave_tui_screen")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when leave_tui_screen fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_show_cursor_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) = run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "show_cursor")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when show_cursor fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_nonzero_when_poll_immediate_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) = run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "poll_immediate")])?;
    assert!(
        !status.success(),
        "expected tui_smoke to fail when poll_immediate fails, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_successfully_with_unknown_tui_failpoint()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) = run_tui_smoke_in_pty(&[("TENEX_TEST_TUI_FAILPOINT", "unknown")])?;
    assert!(
        status.success(),
        "expected tui_smoke to succeed with unknown failpoint, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_successfully_with_preview_diff_digest_refresh()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) = run_tui_smoke_in_pty_with_input_delay(
        &[
            ("TENEX_TEST_TUI_SMOKE_PREVIEW_TAB", "1"),
            ("TENEX_TEST_TUI_SMOKE_WAIT_FOR_QUIT", "1"),
        ],
        std::time::Duration::from_millis(1_200),
    )?;
    assert!(
        status.success(),
        "expected tui_smoke to succeed with preview diff digest refresh, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_successfully_with_diff_refresh_while_waiting()
-> Result<(), Box<dyn std::error::Error>> {
    let (status, output) = run_tui_smoke_in_pty_with_input_delay(
        &[("TENEX_TEST_TUI_SMOKE_WAIT_FOR_QUIT", "1")],
        std::time::Duration::from_millis(1_200),
    )?;
    assert!(
        status.success(),
        "expected tui_smoke to succeed with diff refresh while waiting, got status={status:?} output={}",
        String::from_utf8_lossy(&output),
    );
    Ok(())
}

#[test]
fn test_tui_smoke_binary_exits_successfully_with_disable_mouse_env_values()
-> Result<(), Box<dyn std::error::Error>> {
    for value in ["1", "true", "yes", "on", "0"] {
        let (status, output) = run_tui_smoke_in_pty(&[("TENEX_DISABLE_MOUSE", value)])?;
        assert!(
            status.success(),
            "expected tui_smoke to succeed with TENEX_DISABLE_MOUSE={value:?}, got status={status:?} output={}",
            String::from_utf8_lossy(&output),
        );
    }

    Ok(())
}
