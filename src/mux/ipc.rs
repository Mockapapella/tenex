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
mod tests {
    use super::*;
    use interprocess::local_socket::traits::Stream as _;
    use serde::{Deserialize, Serialize};
    use std::io::Cursor;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct Payload {
        hello: String,
    }

    #[test]
    fn test_write_and_read_json_roundtrip() {
        let payload = Payload {
            hello: "world".to_string(),
        };

        let mut bytes = Vec::new();
        write_json(&mut bytes, &payload).unwrap();

        let mut cursor = Cursor::new(bytes);
        let decoded: Payload = read_json(&mut cursor).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_read_json_errors_when_length_is_unreadable() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let error = read_json::<Payload>(&mut cursor).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to read message length"));
    }

    #[test]
    fn test_read_json_errors_when_payload_is_truncated() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(b"{}\n"); // not enough payload bytes

        let mut cursor = Cursor::new(bytes);
        let error = read_json::<Payload>(&mut cursor).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to read message"));
    }

    #[test]
    fn test_read_json_errors_when_payload_is_not_valid_json() {
        let payload = b"not-json";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(payload);

        let mut cursor = Cursor::new(bytes);
        let error = read_json::<Payload>(&mut cursor).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to decode JSON message"));
    }

    struct FailSerialize;

    impl Serialize for FailSerialize {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("serialize boom"))
        }
    }

    #[test]
    fn test_write_json_errors_when_encode_fails() {
        let mut bytes = Vec::new();
        let error = write_json(&mut bytes, &FailSerialize).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to encode JSON message"));
    }

    #[test]
    fn test_write_json_errors_when_message_too_large() {
        let error = payload_len_bytes(u32::MAX as usize + 1).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Message too large"));
    }

    #[test]
    fn test_exercise_len_prefixed_payload_length_for_tests_covers_boundary() {
        exercise_len_prefixed_payload_length_for_tests(0).unwrap();

        let error =
            exercise_len_prefixed_payload_length_for_tests(u32::MAX as usize + 1).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Message too large"));
    }

    #[derive(Default)]
    struct SpyWriter {
        bytes: Vec<u8>,
        write_calls: usize,
        fail_on_write_call: Option<usize>,
        flush_error: bool,
    }

    impl Write for SpyWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.write_calls = self.write_calls.saturating_add(1);
            if self.fail_on_write_call == Some(self.write_calls) {
                return Err(std::io::Error::other("write boom"));
            }

            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            if self.flush_error {
                return Err(std::io::Error::other("flush boom"));
            }
            Ok(())
        }
    }

    #[test]
    fn test_write_json_errors_when_length_write_fails() {
        let mut writer = SpyWriter {
            fail_on_write_call: Some(1),
            ..SpyWriter::default()
        };

        let error = write_len_prefixed_payload(&mut writer, &[]).unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to write message length"));
    }

    #[test]
    fn test_write_len_prefixed_payload_writes_length_only_for_empty_payload() {
        let mut writer = SpyWriter::default();
        write_len_prefixed_payload(&mut writer, &[]).unwrap();
        assert_eq!(writer.write_calls, 1);
        assert_eq!(writer.bytes, 0u32.to_le_bytes().to_vec());
    }

    #[test]
    fn test_write_len_prefixed_payload_succeeds_when_writer_ok() {
        let mut writer = SpyWriter::default();
        write_len_prefixed_payload(&mut writer, b"hi").unwrap();
        assert_eq!(writer.write_calls, 2);
        assert_eq!(
            writer.bytes,
            [2u32.to_le_bytes().as_slice(), b"hi".as_slice()].concat()
        );
    }

    #[test]
    fn test_write_json_errors_when_payload_write_fails() {
        let mut writer = SpyWriter {
            fail_on_write_call: Some(2),
            ..SpyWriter::default()
        };

        let error = write_len_prefixed_payload(&mut writer, b"hi").unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to write message"));
    }

    #[test]
    fn test_write_json_errors_when_flush_fails() {
        let mut writer = SpyWriter {
            flush_error: true,
            ..SpyWriter::default()
        };

        let error = write_len_prefixed_payload(&mut writer, b"hi").unwrap_err();
        let message = format!("{error}");
        assert!(message.contains("Failed to flush message"));
    }

    fn run_stream_read_error_test(test_name: &str, f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .name(test_name.to_string())
            .spawn(f)
            .expect("Spawn mux ipc stream read test thread")
            .join()
            .expect("Join mux ipc stream read test thread");
    }

    fn setup_stream_pair(
        server: impl FnOnce(interprocess::local_socket::Stream) + Send + 'static,
    ) -> (
        tempfile::TempDir,
        interprocess::local_socket::Stream,
        std::thread::JoinHandle<()>,
    ) {
        use interprocess::local_socket::traits::ListenerExt as _;

        let temp_dir = tempfile::TempDir::new().expect("Create mux ipc temp dir");
        let socket_path = temp_dir.path().join("mux.sock");
        crate::mux::set_socket_override(&socket_path.to_string_lossy())
            .expect("Set mux socket override");
        let endpoint = crate::mux::socket_endpoint().expect("Resolve mux endpoint");

        let listener = interprocess::local_socket::ListenerOptions::new()
            .name(endpoint.name.clone())
            .create_sync()
            .expect("Create mux listener");

        let handle = std::thread::spawn(move || {
            let mut incoming = listener.incoming();
            let stream = incoming
                .next()
                .expect("Expected mux client connection")
                .expect("Mux accept failed");
            server(stream);
        });

        let stream = interprocess::local_socket::Stream::connect(endpoint.name.clone())
            .expect("Connect to mux listener");

        (temp_dir, stream, handle)
    }

    #[test]
    fn test_read_json_errors_when_stream_closes_before_length_is_read() {
        run_stream_read_error_test("mux-ipc-read-length-error", || {
            let (_temp_dir, mut stream, handle) = setup_stream_pair(drop);

            let err = read_json::<Payload>(&mut stream).unwrap_err();
            assert!(format!("{err}").contains("Failed to read message length"));

            handle.join().expect("Mux ipc server thread panicked");
        });
    }

    #[test]
    fn test_read_json_errors_when_stream_payload_is_truncated() {
        run_stream_read_error_test("mux-ipc-read-payload-error", || {
            let (_temp_dir, mut stream, handle) = setup_stream_pair(|mut stream| {
                stream
                    .write_all(&5u32.to_le_bytes())
                    .expect("Write payload length");
                stream.write_all(b"{}\n").expect("Write payload bytes");
            });

            let err = read_json::<Payload>(&mut stream).unwrap_err();
            assert!(format!("{err}").contains("Failed to read message"));

            handle.join().expect("Mux ipc server thread panicked");
        });
    }

    #[test]
    fn test_read_json_errors_when_stream_payload_is_not_valid_json() {
        run_stream_read_error_test("mux-ipc-read-json-error", || {
            let (_temp_dir, mut stream, handle) = setup_stream_pair(|mut stream| {
                let payload = b"not-json";
                stream
                    .write_all(
                        &u32::try_from(payload.len())
                            .expect("convert payload len")
                            .to_le_bytes(),
                    )
                    .expect("Write payload length");
                stream.write_all(payload).expect("Write payload bytes");
            });

            let err = read_json::<Payload>(&mut stream).unwrap_err();
            assert!(format!("{err}").contains("Failed to decode JSON message"));

            handle.join().expect("Mux ipc server thread panicked");
        });
    }
}
