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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_output_chunk_empty_data() -> Result<()> {
        let read = decode_read_output_response(MuxResponse::OutputChunk {
            start: 10,
            end: 10,
            data_b64: String::new(),
        })?;
        assert_eq!(
            read,
            OutputRead::Chunk(OutputChunk {
                start: 10,
                end: 10,
                data: Vec::new()
            })
        );
        Ok(())
    }

    #[test]
    fn test_decode_output_chunk_decodes_base64() -> Result<()> {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        let bytes = b"hello";
        let read = decode_read_output_response(MuxResponse::OutputChunk {
            start: 0,
            end: 5,
            data_b64: BASE64.encode(bytes),
        })?;
        assert_eq!(
            read,
            OutputRead::Chunk(OutputChunk {
                start: 0,
                end: 5,
                data: bytes.to_vec()
            })
        );
        Ok(())
    }

    #[test]
    fn test_decode_output_reset_empty_checkpoint() -> Result<()> {
        let read = decode_read_output_response(MuxResponse::OutputReset {
            start: 42,
            checkpoint_b64: String::new(),
        })?;
        assert_eq!(
            read,
            OutputRead::Reset(OutputReset {
                start: 42,
                checkpoint: Vec::new()
            })
        );
        Ok(())
    }

    #[test]
    fn test_decode_output_reset_decodes_checkpoint() -> Result<()> {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        let checkpoint = b"\x1b[2J";
        let read = decode_read_output_response(MuxResponse::OutputReset {
            start: 7,
            checkpoint_b64: BASE64.encode(checkpoint),
        })?;
        assert_eq!(
            read,
            OutputRead::Reset(OutputReset {
                start: 7,
                checkpoint: checkpoint.to_vec()
            })
        );
        Ok(())
    }

    #[test]
    fn test_decode_errors_on_unexpected_response() {
        let result = decode_read_output_response(MuxResponse::Ok);
        assert!(result.is_err());
    }
}
