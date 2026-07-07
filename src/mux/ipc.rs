//! IPC framing helpers for the mux daemon.
#![cfg_attr(coverage_nightly, coverage(off))]

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{Read, Write};

#[cfg_attr(coverage_nightly, coverage(off))]
fn payload_len_bytes(payload_len: usize) -> Result<[u8; 4]> {
    let Ok(len) = u32::try_from(payload_len) else {
        bail!("Message too large");
    };
    Ok(len.to_le_bytes())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn write_len_prefixed_payload_with_len(
    writer: &mut dyn Write,
    payload_len: usize,
    payload: Option<&[u8]>,
) -> Result<()> {
    let len_bytes = payload_len_bytes(payload_len)?;
    writer
        .write_all(&len_bytes)
        .context("Failed to write message length")?;
    if let Some(payload) = payload {
        writer
            .write_all(payload)
            .context("Failed to write message")?;
    }
    writer.flush().context("Failed to flush message")?;
    Ok(())
}

fn write_len_prefixed_payload(writer: &mut dyn Write, payload: &[u8]) -> Result<()> {
    write_len_prefixed_payload_with_len(writer, payload.len(), Some(payload))
}

#[cfg(any(test, coverage))]
#[doc(hidden)]
pub fn exercise_len_prefixed_payload_length_for_tests(payload_len: usize) -> Result<()> {
    let mut writer = std::io::sink();
    write_len_prefixed_payload_with_len(&mut writer, payload_len, None)
}

/// Read a length-prefixed JSON message.
///
/// # Errors
///
/// Returns an error if the stream cannot be read or the JSON cannot be decoded.
pub fn read_json<T: DeserializeOwned>(reader: &mut dyn Read) -> Result<T> {
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

/// Write a length-prefixed JSON message.
///
/// # Errors
///
/// Returns an error if the message cannot be encoded or written.
pub fn write_json<T: Serialize>(writer: &mut dyn Write, value: &T) -> Result<()> {
    let buf = serde_json::to_vec(value).context("Failed to encode JSON message")?;
    write_len_prefixed_payload(writer, &buf)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests;
