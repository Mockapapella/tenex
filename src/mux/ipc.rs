//! IPC framing helpers for the mux daemon.

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Read a length-prefixed JSON message.
///
/// # Errors
///
/// Returns an error if the stream cannot be read or the JSON cannot be decoded.
pub fn read_json<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    let mut len_bytes = [0u8; 4];
    reader
        .read_exact(&mut len_bytes)
        .context("Failed to read message length")?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .context("Failed to read message")?;
    serde_json::from_slice(&buf).context("Failed to decode JSON message")
}

/// Read a length-prefixed JSON message with a timeout.
///
/// This is intended for nonblocking streams (see `Stream::set_nonblocking(true)`).
///
/// # Errors
///
/// Returns an error if the stream cannot be read, the JSON cannot be decoded, or the timeout
/// expires.
pub fn read_json_with_timeout<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    timeout: Duration,
) -> Result<T> {
    let start = Instant::now();

    let mut len_bytes = [0u8; 4];
    read_exact_with_timeout(reader, &mut len_bytes, timeout, start)
        .context("Failed to read message length")?;

    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    read_exact_with_timeout(reader, &mut buf, timeout, start).context("Failed to read message")?;

    serde_json::from_slice(&buf).context("Failed to decode JSON message")
}

/// Write a length-prefixed JSON message.
///
/// # Errors
///
/// Returns an error if the message cannot be encoded or written.
pub fn write_json<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let buf = serde_json::to_vec(value).context("Failed to encode JSON message")?;
    let len = u32::try_from(buf.len()).context("Message too large")?;
    writer
        .write_all(&len.to_le_bytes())
        .context("Failed to write message length")?;
    writer.write_all(&buf).context("Failed to write message")?;
    writer.flush().context("Failed to flush message")?;
    Ok(())
}

/// Write a length-prefixed JSON message with a timeout.
///
/// This is intended for nonblocking streams (see `Stream::set_nonblocking(true)`).
///
/// # Errors
///
/// Returns an error if the message cannot be encoded/written or the timeout expires.
pub fn write_json_with_timeout<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    let buf = serde_json::to_vec(value).context("Failed to encode JSON message")?;
    let len = u32::try_from(buf.len()).context("Message too large")?;

    write_all_with_timeout(writer, &len.to_le_bytes(), timeout, start)
        .context("Failed to write message length")?;
    write_all_with_timeout(writer, &buf, timeout, start).context("Failed to write message")?;

    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    anyhow::bail!("Timed out flushing message");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err).context("Failed to flush message"),
        }
    }
}

fn read_exact_with_timeout<R: Read>(
    reader: &mut R,
    mut buf: &mut [u8],
    timeout: Duration,
    start: Instant,
) -> Result<()> {
    while !buf.is_empty() {
        match reader.read(buf) {
            Ok(0) => anyhow::bail!("Unexpected EOF"),
            Ok(n) => {
                buf = &mut buf[n..];
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    anyhow::bail!("Timed out reading from IPC stream");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err).context("Failed to read from IPC stream"),
        }
    }

    Ok(())
}

fn write_all_with_timeout<W: Write>(
    writer: &mut W,
    mut buf: &[u8],
    timeout: Duration,
    start: Instant,
) -> Result<()> {
    while !buf.is_empty() {
        match writer.write(buf) {
            Ok(0) => anyhow::bail!("Write returned 0 bytes"),
            Ok(n) => buf = &buf[n..],
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    anyhow::bail!("Timed out writing to IPC stream");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(err) => return Err(err).context("Failed to write to IPC stream"),
        }
    }

    Ok(())
}
