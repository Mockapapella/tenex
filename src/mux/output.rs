//! Client-side helpers for reading raw output bytes from the mux daemon.

use super::protocol::{MuxRequest, MuxResponse};
use anyhow::{Context, Result, bail};
use base64::Engine as _;

/// Read raw output bytes from a mux target.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputStream;

/// A contiguous chunk of output bytes from the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputChunk {
    /// First sequence number included in this chunk.
    pub start: u64,
    /// Sequence number after the last byte included in this chunk.
    pub end: u64,
    /// Raw output bytes in the range `[start, end)`.
    pub data: Vec<u8>,
}

/// A reset response indicating the client must rebuild local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputReset {
    /// Sequence number to restart from.
    pub start: u64,
    /// Checkpoint stream representing the terminal state at `start`.
    pub checkpoint: Vec<u8>,
}

/// Result of reading output bytes from the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputRead {
    /// A chunk of raw output bytes.
    Chunk(OutputChunk),
    /// The client is behind and must reset from the provided checkpoint.
    Reset(OutputReset),
}

/// Current raw output sequence bounds for a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputCursor {
    /// First sequence number still retained by the daemon.
    pub start: u64,
    /// Sequence number after the last observed byte.
    pub end: u64,
}

impl OutputStream {
    /// Create a new output stream client.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Read output bytes for a target since `after`.
    ///
    /// # Errors
    ///
    /// Returns an error if the daemon cannot be reached or responds with an error.
    pub fn read_output(&self, target: &str, after: u64, max_bytes: u32) -> Result<OutputRead> {
        let response = super::client::request(&MuxRequest::ReadOutput {
            target: target.to_string(),
            after,
            max_bytes,
        })?;
        decode_read_output_response(response)
    }

    /// Read the current raw output sequence bounds for a target.
    ///
    /// # Errors
    ///
    /// Returns an error if the daemon cannot be reached or responds with an error.
    pub fn cursor(&self, target: &str) -> Result<OutputCursor> {
        let response = super::client::request(&MuxRequest::OutputCursor {
            target: target.to_string(),
        })?;
        decode_output_cursor_response(response)
    }
}

fn decode_read_output_response(response: MuxResponse) -> Result<OutputRead> {
    use base64::engine::general_purpose::STANDARD as BASE64;

    match response {
        MuxResponse::OutputChunk {
            start,
            end,
            data_b64,
        } => {
            let data = if data_b64.is_empty() {
                Vec::new()
            } else {
                BASE64
                    .decode(data_b64.as_bytes())
                    .context("Failed to decode mux output chunk base64")?
            };
            Ok(OutputRead::Chunk(OutputChunk { start, end, data }))
        }
        MuxResponse::OutputReset {
            start,
            checkpoint_b64,
        } => {
            let checkpoint = if checkpoint_b64.is_empty() {
                Vec::new()
            } else {
                BASE64
                    .decode(checkpoint_b64.as_bytes())
                    .context("Failed to decode mux output checkpoint base64")?
            };
            Ok(OutputRead::Reset(OutputReset { start, checkpoint }))
        }
        MuxResponse::Err { message } => bail!("{message}"),
        other => bail!("Unexpected response: {other:?}"),
    }
}

fn decode_output_cursor_response(response: MuxResponse) -> Result<OutputCursor> {
    match response {
        MuxResponse::OutputCursor { start, end } => Ok(OutputCursor { start, end }),
        MuxResponse::Err { message } => bail!("{message}"),
        other => bail!("Unexpected response: {other:?}"),
    }
}

#[cfg(test)]
mod tests;
