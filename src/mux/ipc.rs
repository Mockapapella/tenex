//! IPC framing helpers for the mux daemon.

use anyhow::{Context, Result, bail};
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

/// Read a length-prefixed JSON message, returning an error if no complete message arrives within
/// `timeout`.
///
/// This function is designed for use with streams placed in non-blocking mode (for example by
/// calling `set_nonblocking(true)` on an `interprocess::local_socket::Stream`). When used with a
/// blocking reader, the timeout cannot be enforced.
///
/// # Errors
///
/// Returns an error if the stream cannot be read, the operation times out, or the JSON cannot be
/// decoded.
pub fn read_json_with_timeout<R: Read, T: DeserializeOwned>(
    reader: &mut R,
    timeout: Duration,
) -> Result<T> {
    let mut len_bytes = [0u8; 4];
    read_exact_with_timeout(reader, &mut len_bytes, timeout)
        .context("Failed to read message length")?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut buf = vec![0u8; len];
    read_exact_with_timeout(reader, &mut buf, timeout).context("Failed to read message")?;
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

/// Write a length-prefixed JSON message, returning an error if it cannot be fully written within
/// `timeout`.
///
/// This function is designed for use with streams placed in non-blocking mode (for example by
/// calling `set_nonblocking(true)` on an `interprocess::local_socket::Stream`). When used with a
/// blocking writer, the timeout cannot be enforced.
///
/// # Errors
///
/// Returns an error if the message cannot be encoded, the operation times out, or the data cannot
/// be written.
pub fn write_json_with_timeout<W: Write, T: Serialize>(
    writer: &mut W,
    value: &T,
    timeout: Duration,
) -> Result<()> {
    let buf = serde_json::to_vec(value).context("Failed to encode JSON message")?;
    let len = u32::try_from(buf.len()).context("Message too large")?;
    write_all_with_timeout(writer, &len.to_le_bytes(), timeout)
        .context("Failed to write message length")?;
    write_all_with_timeout(writer, &buf, timeout).context("Failed to write message")?;
    flush_with_timeout(writer, timeout).context("Failed to flush message")?;
    Ok(())
}

fn read_exact_with_timeout<R: Read>(
    reader: &mut R,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    let mut offset = 0usize;

    while offset < buf.len() {
        match reader.read(&mut buf[offset..]) {
            Ok(0) => bail!("Unexpected EOF"),
            Ok(read) => {
                offset = offset.saturating_add(read);
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    bail!("Timed out");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err).context("Read failed"),
        }
    }

    Ok(())
}

fn write_all_with_timeout<W: Write>(
    writer: &mut W,
    mut buf: &[u8],
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();

    while !buf.is_empty() {
        match writer.write(buf) {
            Ok(0) => bail!("Failed to write message"),
            Ok(written) => buf = &buf[written..],
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    bail!("Timed out");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err).context("Write failed"),
        }
    }

    Ok(())
}

fn flush_with_timeout<W: Write>(writer: &mut W, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= timeout {
                    bail!("Timed out");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err).context("Flush failed"),
        }
    }
}
