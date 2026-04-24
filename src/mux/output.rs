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
mod tests {
    use super::*;

    #[test]
    fn test_decode_output_chunk_empty_data() {
        let read = decode_read_output_response(MuxResponse::OutputChunk {
            start: 10,
            end: 10,
            data_b64: String::new(),
        })
        .expect("Decode output chunk");
        assert_eq!(
            read,
            OutputRead::Chunk(OutputChunk {
                start: 10,
                end: 10,
                data: Vec::new()
            })
        );
    }

    #[test]
    fn test_decode_output_chunk_decodes_base64() {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        let bytes = b"hello";
        let read = decode_read_output_response(MuxResponse::OutputChunk {
            start: 0,
            end: 5,
            data_b64: BASE64.encode(bytes),
        })
        .expect("Decode output chunk base64");
        assert_eq!(
            read,
            OutputRead::Chunk(OutputChunk {
                start: 0,
                end: 5,
                data: bytes.to_vec()
            })
        );
    }

    #[test]
    fn test_decode_output_chunk_errors_on_invalid_base64() {
        let err = decode_read_output_response(MuxResponse::OutputChunk {
            start: 1,
            end: 2,
            data_b64: "not base64".to_string(),
        })
        .unwrap_err();

        let message = format!("{err}");
        assert!(message.contains("Failed to decode mux output chunk base64"));
    }

    #[test]
    fn test_decode_output_reset_empty_checkpoint() {
        let read = decode_read_output_response(MuxResponse::OutputReset {
            start: 42,
            checkpoint_b64: String::new(),
        })
        .expect("Decode output reset");
        assert_eq!(
            read,
            OutputRead::Reset(OutputReset {
                start: 42,
                checkpoint: Vec::new()
            })
        );
    }

    #[test]
    fn test_decode_output_reset_decodes_checkpoint() {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD as BASE64;

        let checkpoint = b"\x1b[2J";
        let read = decode_read_output_response(MuxResponse::OutputReset {
            start: 7,
            checkpoint_b64: BASE64.encode(checkpoint),
        })
        .expect("Decode output reset base64");
        assert_eq!(
            read,
            OutputRead::Reset(OutputReset {
                start: 7,
                checkpoint: checkpoint.to_vec()
            })
        );
    }

    #[test]
    fn test_decode_output_reset_errors_on_invalid_base64() {
        let err = decode_read_output_response(MuxResponse::OutputReset {
            start: 1,
            checkpoint_b64: "not base64".to_string(),
        })
        .unwrap_err();

        let message = format!("{err}");
        assert!(message.contains("Failed to decode mux output checkpoint base64"));
    }

    #[test]
    fn test_decode_read_output_response_propagates_mux_error() {
        let err = decode_read_output_response(MuxResponse::Err {
            message: "boom".to_string(),
        })
        .unwrap_err();

        assert!(format!("{err}").contains("boom"));
    }

    #[test]
    fn test_decode_output_cursor() {
        let cursor = decode_output_cursor_response(MuxResponse::OutputCursor { start: 3, end: 9 })
            .expect("Decode output cursor");
        assert_eq!(cursor, OutputCursor { start: 3, end: 9 });
    }

    #[test]
    fn test_decode_errors_on_unexpected_response() {
        let result = decode_read_output_response(MuxResponse::Ok);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_output_cursor_propagates_mux_error() {
        let err = decode_output_cursor_response(MuxResponse::Err {
            message: "boom".to_string(),
        })
        .unwrap_err();

        assert!(format!("{err}").contains("boom"));
    }

    #[test]
    fn test_decode_output_cursor_errors_on_unexpected_response() {
        let result = decode_output_cursor_response(MuxResponse::Ok);
        assert!(result.is_err());
    }

    fn run_mux_failing_request_test(test_name: &str, f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .name(test_name.to_string())
            .spawn(f)
            .expect("Spawn mux failing request thread")
            .join()
            .expect("Join mux failing request thread");
    }

    fn setup_mux_listener_that_closes_connections(
        expected_connections: usize,
    ) -> (tempfile::TempDir, std::thread::JoinHandle<()>) {
        use interprocess::local_socket::traits::ListenerExt as _;

        let temp_dir = tempfile::TempDir::new().expect("Create mux temp dir");
        let socket_path = temp_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy())
            .expect("Set socket override");
        let endpoint = crate::mux::socket_endpoint().expect("Resolve socket endpoint");
        let listener = interprocess::local_socket::ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .expect("Create mux listener");

        let accept_thread = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            for _ in 0..expected_connections {
                let mut stream = incoming
                    .next()
                    .expect("Expected mux client connection")
                    .expect("Mux accept failed");
                let _: super::super::protocol::MuxRequest =
                    crate::mux::read_json(&mut stream).expect("Read mux request");
            }
        });

        (temp_dir, accept_thread)
    }

    #[test]
    fn test_output_stream_read_output_reports_request_errors() {
        run_mux_failing_request_test("output-stream-read-output-error", || {
            let (_temp_dir, accept_thread) = setup_mux_listener_that_closes_connections(2);

            let stream = OutputStream::new();
            let err = stream.read_output("root", 0, 64).unwrap_err();
            assert!(format!("{err}").contains("Failed to read message length"));

            accept_thread.join().expect("Mux accept thread panicked");
        });
    }

    #[test]
    fn test_output_stream_cursor_reports_request_errors() {
        run_mux_failing_request_test("output-stream-cursor-error", || {
            let (_temp_dir, accept_thread) = setup_mux_listener_that_closes_connections(2);

            let stream = OutputStream::new();
            let err = stream.cursor("root").unwrap_err();
            assert!(format!("{err}").contains("Failed to read message length"));

            accept_thread.join().expect("Mux accept thread panicked");
        });
    }
}
