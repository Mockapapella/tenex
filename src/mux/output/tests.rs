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
    crate::mux::set_socket_override(&socket_path.to_string_lossy()).expect("Set socket override");
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
