//! Exercises `mux::running_daemon_version` in a non-test binary build.

use anyhow::{Context, Result};

#[cfg(unix)]
use interprocess::local_socket::traits::ListenerExt as _;
#[cfg(unix)]
use interprocess::local_socket::{GenericFilePath, ListenerOptions, prelude::*};
#[cfg(unix)]
use std::io::{Read, Write};

#[test]
fn test_mux_running_version_probe_exits_nonzero_for_invalid_socket_name() -> Result<()> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_TEST_SOCKET_OVERRIDE_VALUE", "   ")
        .output()
        .context("run mux_running_version_probe with invalid socket override value")?;

    assert!(
        !output.status.success(),
        "expected mux_running_version_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_mux_running_version_probe_exits_successfully_when_daemon_missing() -> Result<()> {
    let temp = tempfile::TempDir::new().context("create temp dir")?;
    let socket_path = temp.path().join("mux.sock");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_TEST_CALL_IS_SERVER_RUNNING", "1")
        .env("TENEX_MUX_SOCKET", &socket_path)
        .output()
        .context("run mux_running_version_probe with missing daemon")?;

    assert!(
        output.status.success(),
        "expected mux_running_version_probe to exit zero when daemon missing, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_mux_running_version_probe_exits_successfully_when_endpoint_resolution_fails() -> Result<()>
{
    let long_socket = format!("/tmp/tenex-mux-{}", "a".repeat(200));

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_TEST_CALL_IS_SERVER_RUNNING", "1")
        .env("TENEX_TEST_EXIT_AFTER_IS_SERVER_RUNNING", "1")
        .env("TENEX_TEST_SOCKET_OVERRIDE_VALUE", &long_socket)
        .output()
        .context("run mux_running_version_probe with invalid socket path")?;

    assert!(
        output.status.success(),
        "expected mux_running_version_probe to exit zero when socket endpoint invalid, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn test_mux_running_version_probe_accepts_valid_socket_override_value() -> Result<()> {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_TEST_SOCKET_OVERRIDE_VALUE", "tenex-mux-test-ok")
        .output()
        .context("run mux_running_version_probe with valid socket override value")?;

    assert!(
        output.status.success(),
        "expected mux_running_version_probe to exit zero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_mux_running_version_probe_exits_nonzero_when_daemon_responds_with_error() -> Result<()> {
    fn read_len_prefixed_json_value(reader: &mut impl Read) -> Result<serde_json::Value> {
        let mut len_bytes = [0u8; 4];
        reader
            .read_exact(&mut len_bytes)
            .context("read message length")?;
        let len = usize::try_from(u32::from_le_bytes(len_bytes)).context("message length")?;
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).context("read message bytes")?;
        serde_json::from_slice(&buf).context("parse json message")
    }

    fn write_len_prefixed_json_value(
        writer: &mut impl Write,
        value: &serde_json::Value,
    ) -> Result<()> {
        let buf = serde_json::to_vec(value).context("encode json message")?;
        let len = u32::try_from(buf.len()).context("message length")?;
        writer
            .write_all(&len.to_le_bytes())
            .context("write message length")?;
        writer.write_all(&buf).context("write message bytes")?;
        writer.flush().context("flush message bytes")?;
        Ok(())
    }

    let temp = tempfile::TempDir::new().context("create temp dir")?;
    let socket_path = temp.path().join("mux.sock");
    let socket_display = socket_path.to_string_lossy().into_owned();
    let socket_name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()
        .context("fs socket name")?
        .into_owned();

    let server = std::thread::spawn(move || -> Result<()> {
        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .context("create mux version probe listener")?;
        let mut stream = listener
            .incoming()
            .next()
            .transpose()
            .context("accept mux version probe connection")?
            .ok_or_else(|| anyhow::anyhow!("mux version probe server received no connection"))?;

        let req = read_len_prefixed_json_value(&mut stream)?;
        assert_eq!(req, serde_json::Value::String("Ping".to_string()));
        write_len_prefixed_json_value(
            &mut stream,
            &serde_json::json!({
                "Err": { "message": "forced mux probe error" },
            }),
        )?;

        Ok(())
    });

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_MUX_SOCKET", socket_display)
        .output()
        .context("run mux_running_version_probe while daemon responds with error")?;

    match server.join() {
        Ok(result) => result?,
        Err(_) => anyhow::bail!("mux version probe server panicked"),
    }

    assert!(
        !output.status.success(),
        "expected mux_running_version_probe to exit nonzero, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("forced mux probe error"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_mux_running_version_probe_exits_successfully_when_daemon_closes_after_read() -> Result<()> {
    let temp = tempfile::TempDir::new().context("create temp dir")?;
    let socket_path = temp.path().join("mux.sock");
    let socket_display = socket_path.to_string_lossy().into_owned();
    let socket_name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()
        .context("fs socket name")?
        .into_owned();

    let server = std::thread::spawn(move || -> Result<()> {
        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .context("create mux version probe listener")?;
        let mut stream = listener
            .incoming()
            .next()
            .transpose()
            .context("accept mux version probe connection")?
            .ok_or_else(|| anyhow::anyhow!("mux version probe server received no connection"))?;

        let mut len_bytes = [0u8; 4];
        stream
            .read_exact(&mut len_bytes)
            .context("read message length")?;
        let len = usize::try_from(u32::from_le_bytes(len_bytes)).context("message length")?;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).context("read message bytes")?;

        Ok(())
    });

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_MUX_SOCKET", socket_display)
        .output()
        .context("run mux_running_version_probe while server closes")?;

    match server.join() {
        Ok(result) => result?,
        Err(_) => anyhow::bail!("mux version probe server panicked"),
    }

    assert!(
        output.status.success(),
        "expected mux_running_version_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
#[cfg(unix)]
fn test_mux_running_version_probe_prints_version_when_daemon_responds() -> Result<()> {
    fn read_len_prefixed_json_value(reader: &mut impl Read) -> Result<serde_json::Value> {
        let mut len_bytes = [0u8; 4];
        reader
            .read_exact(&mut len_bytes)
            .context("read message length")?;
        let len = usize::try_from(u32::from_le_bytes(len_bytes)).context("message length")?;
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).context("read message bytes")?;
        serde_json::from_slice(&buf).context("parse json message")
    }

    fn write_len_prefixed_json_value(
        writer: &mut impl Write,
        value: &serde_json::Value,
    ) -> Result<()> {
        let buf = serde_json::to_vec(value).context("encode json message")?;
        let len = u32::try_from(buf.len()).context("message length")?;
        writer
            .write_all(&len.to_le_bytes())
            .context("write message length")?;
        writer.write_all(&buf).context("write message bytes")?;
        writer.flush().context("flush message bytes")?;
        Ok(())
    }

    let temp = tempfile::TempDir::new().context("create temp dir")?;
    let socket_path = temp.path().join("mux.sock");
    let socket_display = socket_path.to_string_lossy().into_owned();
    let socket_name = socket_path
        .as_path()
        .to_fs_name::<GenericFilePath>()
        .context("fs socket name")?
        .into_owned();

    let want_version = "tenex-mux/test-version".to_string();
    let server = std::thread::spawn(move || -> Result<()> {
        let listener = ListenerOptions::new()
            .name(socket_name)
            .create_sync()
            .context("create mux version probe listener")?;
        let mut stream = listener
            .incoming()
            .next()
            .transpose()
            .context("accept mux version probe connection")?
            .ok_or_else(|| anyhow::anyhow!("mux version probe server received no connection"))?;

        let req = read_len_prefixed_json_value(&mut stream)?;
        assert_eq!(req, serde_json::Value::String("Ping".to_string()));
        write_len_prefixed_json_value(
            &mut stream,
            &serde_json::json!({
                "Pong": { "version": want_version },
            }),
        )?;

        Ok(())
    });

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mux_running_version_probe"))
        .env("TENEX_MUX_SOCKET", socket_display)
        .output()
        .context("run mux_running_version_probe while daemon responds")?;

    match server.join() {
        Ok(result) => result?,
        Err(_) => anyhow::bail!("mux version probe server panicked"),
    }

    assert!(
        output.status.success(),
        "expected mux_running_version_probe success, got status={:?} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "tenex-mux/test-version");
    Ok(())
}
