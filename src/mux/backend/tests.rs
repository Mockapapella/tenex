use super::*;
use crate::test_support::BlockingWriter;
use portable_pty::{Child, ChildKiller, MasterPty, PtyPair, PtySystem, SlavePty};
use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::sink)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::with_default(&dispatch, f)
}

#[derive(Debug, Clone, Copy)]
struct StubMasterPty;

impl MasterPty for StubMasterPty {
    fn resize(&self, _size: PtySize) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn get_size(&self) -> Result<PtySize, anyhow::Error> {
        Ok(default_pty_size())
    }

    fn try_clone_reader(&self) -> Result<Box<dyn io::Read + Send>, anyhow::Error> {
        Ok(Box::new(io::empty()))
    }

    fn take_writer(&self) -> Result<Box<dyn io::Write + Send>, anyhow::Error> {
        Ok(Box::new(io::sink()))
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::prelude::RawFd> {
        None
    }

    #[cfg(unix)]
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug)]
struct NeverSpawnSlavePty;

impl SlavePty for NeverSpawnSlavePty {
    fn spawn_command(
        &self,
        _cmd: portable_pty::CommandBuilder,
    ) -> Result<Box<dyn Child + Send + Sync>, anyhow::Error> {
        unreachable!("spawn_command should not be called")
    }
}

#[derive(Debug, Clone, Copy)]
struct CloneReaderFailMasterPty;

impl MasterPty for CloneReaderFailMasterPty {
    fn resize(&self, _size: PtySize) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn get_size(&self) -> Result<PtySize, anyhow::Error> {
        Ok(default_pty_size())
    }

    fn try_clone_reader(&self) -> Result<Box<dyn io::Read + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("forced clone reader failure for test"))
    }

    fn take_writer(&self) -> Result<Box<dyn io::Write + Send>, anyhow::Error> {
        Ok(Box::new(io::sink()))
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::prelude::RawFd> {
        None
    }

    #[cfg(unix)]
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct TakeWriterFailMasterPty;

impl MasterPty for TakeWriterFailMasterPty {
    fn resize(&self, _size: PtySize) -> Result<(), anyhow::Error> {
        Ok(())
    }

    fn get_size(&self) -> Result<PtySize, anyhow::Error> {
        Ok(default_pty_size())
    }

    fn try_clone_reader(&self) -> Result<Box<dyn io::Read + Send>, anyhow::Error> {
        Ok(Box::new(io::empty()))
    }

    fn take_writer(&self) -> Result<Box<dyn io::Write + Send>, anyhow::Error> {
        Err(anyhow::anyhow!("forced take writer failure for test"))
    }

    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }

    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<std::os::unix::prelude::RawFd> {
        None
    }

    #[cfg(unix)]
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug)]
struct OpenptyFailSystem;

impl PtySystem for OpenptyFailSystem {
    fn openpty(&self, _size: PtySize) -> anyhow::Result<PtyPair> {
        Err(anyhow::anyhow!("forced openpty failure for test"))
    }
}

#[derive(Debug)]
struct CloneReaderFailSystem;

impl PtySystem for CloneReaderFailSystem {
    fn openpty(&self, _size: PtySize) -> anyhow::Result<PtyPair> {
        Ok(PtyPair {
            slave: Box::new(NeverSpawnSlavePty),
            master: Box::new(CloneReaderFailMasterPty),
        })
    }
}

#[derive(Debug)]
struct TakeWriterFailSystem;

impl PtySystem for TakeWriterFailSystem {
    fn openpty(&self, _size: PtySize) -> anyhow::Result<PtyPair> {
        Ok(PtyPair {
            slave: Box::new(NeverSpawnSlavePty),
            master: Box::new(TakeWriterFailMasterPty),
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct StubChild;

impl ChildKiller for StubChild {
    fn kill(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(*self)
    }
}

impl Child for StubChild {
    fn try_wait(&mut self) -> io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }

    fn wait(&mut self) -> io::Result<portable_pty::ExitStatus> {
        Ok(portable_pty::ExitStatus::with_exit_code(0))
    }

    fn process_id(&self) -> Option<u32> {
        None
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

#[derive(Debug)]
struct ErrorReader;

impl io::Read for ErrorReader {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("read failed"))
    }
}

#[derive(Debug)]
struct FailingWriter;

impl io::Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("flush failed"))
    }
}

#[derive(Debug)]
struct GatedReader {
    first: Option<Vec<u8>>,
    second: Option<Vec<u8>>,
    proceed: mpsc::Receiver<()>,
}

impl GatedReader {
    fn new(first: Vec<u8>, second: Vec<u8>, proceed: mpsc::Receiver<()>) -> Self {
        Self {
            first: Some(first),
            second: Some(second),
            proceed,
        }
    }
}

impl io::Read for GatedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(chunk) = self.first.take() {
            return Ok(copy_read_chunk(buf, &chunk));
        }

        if let Some(chunk) = self.second.take() {
            if self.proceed.recv().is_err() {
                return Ok(0);
            }
            return Ok(copy_read_chunk(buf, &chunk));
        }

        Ok(0)
    }
}

fn copy_read_chunk(buf: &mut [u8], chunk: &[u8]) -> usize {
    let len = chunk.len().min(buf.len());
    buf[..len].copy_from_slice(&chunk[..len]);
    len
}

fn stub_window(name: &str, writer: Box<dyn io::Write + Send>) -> Arc<Mutex<MuxWindow>> {
    let size = default_pty_size();
    let parser = vt100::Parser::new(size.rows, size.cols, DEFAULT_SCROLLBACK);
    let input = WindowInput::new(window_input_label(name, 0), writer);
    Arc::new(Mutex::new(MuxWindow {
        index: 0,
        name: name.to_string(),
        working_dir: std::env::temp_dir(),
        command: Vec::new(),
        master: Box::new(StubMasterPty),
        input,
        child: Box::new(StubChild),
        parser,
        output_history: OutputHistory::default(),
        size,
    }))
}

#[test]
fn test_window_input_accepts_empty_payload() {
    let input = WindowInput::new("empty-payload".to_string(), Box::new(FailingWriter));
    input.enqueue(&[]).expect("empty payload is a no-op");
    input.close();
}

#[test]
fn test_window_input_closes_when_writer_thread_spawn_fails() {
    let _guard = force_writer_thread_spawn_error();
    let input = WindowInput::new("forced-spawn-failure".to_string(), Box::new(io::sink()));
    let err = input.enqueue(b"input").expect_err("queue should be closed");
    assert!(err.to_string().contains("mux input queue is closed"));
}

#[test]
fn test_default_pty_size_has_nonzero_dims() {
    let size = default_pty_size();
    assert_eq!(size.rows, DEFAULT_ROWS);
    assert_eq!(size.cols, DEFAULT_COLS);
}

#[test]
fn test_parse_target_root_and_indexed() {
    let root = parse_target("session").expect("parse root target");
    assert_eq!(
        root,
        WindowTarget {
            session: "session".to_string(),
            window_index: 0
        }
    );

    let indexed = parse_target("session:3").expect("parse indexed target");
    assert_eq!(
        indexed,
        WindowTarget {
            session: "session".to_string(),
            window_index: 3
        }
    );

    assert!(parse_target("session:not-a-number").is_err());
}

#[test]
fn test_resolve_window_reports_index_parse_error() {
    let err =
        resolve_window("session:not-a-number").expect_err("expected invalid window index error");
    err.to_string()
        .contains("Invalid window index")
        .then_some(())
        .expect("expected invalid window index context");
}

#[test]
fn test_resolve_window_errors_when_window_missing() {
    let session_name = format!("tenex-test-resolve-window-{}", uuid::Uuid::new_v4());
    let session = Arc::new(Mutex::new(MuxSession {
        name: session_name.clone(),
        created: 0,
        root_restart_attempts: 0,
        last_root_restart: 0,
        windows: vec![stub_window("root", Box::new(io::sink()))],
    }));

    {
        let mut state = global_state().lock();
        state.sessions.insert(session_name.clone(), session);
    }

    let err = resolve_window(&format!("{session_name}:1")).expect_err("expected missing window");
    assert!(err.to_string().contains("Window '1' not found"));

    {
        let mut state = global_state().lock();
        state.sessions.remove(&session_name);
    }
}

#[test]
fn test_count_pattern_respects_tail_boundary() {
    let haystack = b"abcd\x1b[c----\x1b[c";
    let needle = b"\x1b[c";

    assert_eq!(count_pattern(haystack, b"", 0), 0);
    assert_eq!(count_pattern(b"ab", needle, 0), 0);

    // Tail boundary after first match should only count second match.
    let count = count_pattern(haystack, needle, 10);
    assert_eq!(count, 1);

    // No tail boundary counts both.
    let count = count_pattern(haystack, needle, 0);
    assert_eq!(count, 2);
}

#[test]
fn test_scan_terminal_queries_across_chunks() {
    let mut scan_buf = Vec::new();
    let tail = b"\x1b[";
    let chunk = b"6n\x1b[c";
    let (cpr, da, osc10, osc11) = scan_terminal_queries(&mut scan_buf, tail, chunk);
    assert_eq!(cpr, 1);
    assert_eq!(da, 1);
    assert_eq!(osc10, 0);
    assert_eq!(osc11, 0);

    let mut tail_buf = Vec::new();
    update_terminal_query_tail(&mut tail_buf, &scan_buf);
    assert!(!tail_buf.is_empty());
    assert!(tail_buf.len() <= TERMINAL_QUERY_TAIL);
}

#[test]
fn test_build_command_builder_rejects_empty_argv() {
    let argv: Vec<String> = Vec::new();
    let err = build_command_builder(Some(&argv));
    assert!(err.is_err());
}

#[test]
fn test_build_command_builder_returns_builder_for_nonempty_argv() {
    let argv = vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()];
    let (_builder, normalized) =
        build_command_builder(Some(&argv)).expect("Expected command builder for argv");
    assert_eq!(normalized, argv);
}

#[test]
fn test_output_history_record_checkpoint_resets_buffer() {
    let mut history = OutputHistory::default();
    history.record(b"abc", Some(vec![1, 2, 3]));

    assert_eq!(history.seq_end, 3);
    assert_eq!(history.seq_start, 3);
    assert!(history.buf.is_empty());

    let checkpoint = history.checkpoint.expect("Expected checkpoint");
    assert_eq!(checkpoint.seq, 3);
    assert_eq!(checkpoint.bytes, vec![1, 2, 3]);
}

#[test]
fn test_update_terminal_query_tail_returns_early_for_empty_buffer() {
    let mut tail = vec![1, 2, 3];
    update_terminal_query_tail(&mut tail, &[]);
    assert!(tail.is_empty());
}

#[test]
fn test_respond_to_terminal_queries_noops_without_queries() {
    let window = stub_window("no-queries", Box::new(io::sink()));
    let guard = window.lock();
    respond_to_terminal_queries(&guard, 0, 0, 0, 0);
}

#[test]
fn test_respond_to_terminal_queries_succeeds_with_queries() {
    let window = stub_window("queries-ok", Box::new(io::sink()));
    let guard = window.lock();
    respond_to_terminal_queries(&guard, 1, 0, 0, 0);
}

#[test]
fn test_respond_to_terminal_queries_drops_when_queue_full() {
    let (writer, writer_handle) = BlockingWriter::new();
    let window = stub_window("queue-full", Box::new(writer));
    {
        let guard = window.lock();
        guard.input.enqueue(b"block-writer").expect("block writer");
    }
    writer_handle
        .wait_until_blocked(Duration::from_secs(2))
        .expect("writer should enter blocking write");
    {
        let guard = window.lock();
        guard
            .input
            .enqueue(&vec![b'x'; INPUT_QUEUE_CAPACITY_BYTES])
            .expect("fill queue");
    }

    let guard = window.lock();
    respond_to_terminal_queries(&guard, 1, 0, 0, 0);
    writer_handle.release().expect("release blocking writer");
}

#[test]
fn test_spawn_window_propagates_openpty_errors() {
    let size = default_pty_size();
    let err = spawn_window_with_system(
        &OpenptyFailSystem,
        1,
        "forced-openpty",
        &std::env::temp_dir(),
        None,
        size,
    )
    .expect_err("expected openpty error");
    assert!(err.to_string().contains("Failed to open PTY"));
}

#[test]
fn test_spawn_window_propagates_clone_reader_errors() {
    let size = default_pty_size();
    let err = spawn_window_with_system(
        &CloneReaderFailSystem,
        1,
        "forced-clone-reader",
        &std::env::temp_dir(),
        None,
        size,
    )
    .expect_err("expected clone reader error");
    assert!(err.to_string().contains("Failed to clone PTY reader"));
}

#[test]
fn test_spawn_window_propagates_take_writer_errors() {
    let size = default_pty_size();
    let err = spawn_window_with_system(
        &TakeWriterFailSystem,
        1,
        "forced-take-writer",
        &std::env::temp_dir(),
        None,
        size,
    )
    .expect_err("expected take writer error");
    assert!(err.to_string().contains("Failed to open PTY writer"));
}

#[test]
fn test_spawn_reader_thread_inner_exits_when_reader_errors() {
    let window_disabled = stub_window("reader-error-disabled", Box::new(io::sink()));
    let handle = spawn_reader_thread_inner(window_disabled, Box::new(ErrorReader))
        .expect("Spawn reader thread");
    handle.join().expect("Reader thread panicked");

    with_tracing_dispatch(|| {
        let window_enabled = stub_window("reader-error-enabled", Box::new(io::sink()));
        let handle = spawn_reader_thread_inner(window_enabled, Box::new(ErrorReader))
            .expect("Spawn reader thread");
        handle.join().expect("Reader thread panicked");
    });
}

#[test]
fn test_spawn_reader_thread_inner_checkpoints_without_queries() {
    let window = stub_window("checkpoint", Box::new(io::sink()));
    {
        let mut guard = window.lock();
        guard.output_history.buf.resize(OUTPUT_MAX_BYTES, 0);
    }

    let reader = io::Cursor::new(vec![b'x']);
    let handle =
        spawn_reader_thread_inner(window.clone(), Box::new(reader)).expect("Spawn reader thread");
    handle.join().expect("Reader thread panicked");

    let guard = window.lock();
    assert!(guard.output_history.checkpoint.is_some());
    assert!(guard.output_history.buf.is_empty());
    drop(guard);
}

#[test]
fn test_spawn_reader_thread_inner_logs_terminal_query_response_error() {
    with_tracing_dispatch(|| {
        let window = stub_window("query-error", Box::new(FailingWriter));
        {
            let mut guard = window.lock();
            guard.output_history.buf.resize(OUTPUT_MAX_BYTES, 0);
        }

        let reader = io::Cursor::new(b"\x1b[6n".to_vec());
        let handle = spawn_reader_thread_inner(window.clone(), Box::new(reader))
            .expect("Spawn reader thread");
        handle.join().expect("Reader thread panicked");

        let guard = window.lock();
        assert!(guard.output_history.checkpoint.is_some());
        drop(guard);
    });
}

fn wait_for_output_len(window: &Arc<Mutex<MuxWindow>>, min_len: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if window.lock().output_history.seq_end >= min_len {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn test_reader_survives_full_queue_terminal_query_response_drop() {
    with_tracing_dispatch(|| {
        let (writer, writer_handle) = BlockingWriter::new();
        let window = stub_window("query-full-survives", Box::new(writer));
        {
            let guard = window.lock();
            guard.input.enqueue(b"block-writer").expect("block writer");
        }
        writer_handle
            .wait_until_blocked(Duration::from_secs(2))
            .expect("writer should enter blocking write");
        {
            let guard = window.lock();
            guard
                .input
                .enqueue(&vec![b'x'; INPUT_QUEUE_CAPACITY_BYTES])
                .expect("fill queue");
        }

        let (proceed_tx, proceed_rx) = mpsc::sync_channel(1);
        let reader = GatedReader::new(b"\x1b[6n".to_vec(), b"after-drain".to_vec(), proceed_rx);
        let handle =
            spawn_reader_thread_inner(window.clone(), Box::new(reader)).expect("Spawn reader");

        assert!(
            wait_for_output_len(&window, 4, Duration::from_secs(2)),
            "reader should record the terminal query"
        );
        std::thread::sleep(Duration::from_millis(50));
        let survived = !handle.is_finished();

        writer_handle.release().expect("release blocking writer");
        let _ = proceed_tx.send(());
        handle.join().expect("Reader thread panicked");

        assert!(
            survived,
            "reader should keep running after dropping a response"
        );
        let guard = window.lock();
        assert!(
            guard
                .output_history
                .buf
                .windows(b"after-drain".len())
                .any(|window| window == b"after-drain"),
            "reader should capture later output after the input queue drains"
        );
    });
}

#[test]
fn test_spawn_reader_thread_warns_on_spawn_failure() {
    let window = stub_window("spawn-failure", Box::new(io::sink()));
    let _guard = force_reader_thread_spawn_error();

    spawn_reader_thread_inner(window.clone(), Box::new(io::empty()))
        .expect_err("Expected forced thread spawn failure");
    spawn_reader_thread(window.clone(), Box::new(io::empty()));
    with_tracing_dispatch(|| spawn_reader_thread(window, Box::new(io::empty())));
}

#[test]
fn test_stub_types_cover_required_trait_methods() {
    let master = StubMasterPty;
    master
        .resize(default_pty_size())
        .expect("Expected resize to succeed");
    assert_eq!(
        master.get_size().expect("Expected get_size to succeed"),
        default_pty_size()
    );

    let mut reader = master
        .try_clone_reader()
        .expect("Expected clone reader to succeed");
    let mut buf = [0u8; 1];
    let _ = reader.read(&mut buf).expect("Expected reader to succeed");

    let mut writer = master.take_writer().expect("Expected writer to succeed");
    writer
        .write_all(b"hi")
        .expect("Expected writer to accept bytes");

    #[cfg(unix)]
    {
        assert_eq!(master.process_group_leader(), None);
        assert_eq!(master.as_raw_fd(), None);
        assert_eq!(master.tty_name(), None);
    }

    let slave = NeverSpawnSlavePty;
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = slave.spawn_command(CommandBuilder::new("sh"));
    }));
    assert!(panicked.is_err());

    let clone_reader_fail_master = CloneReaderFailMasterPty;
    clone_reader_fail_master
        .resize(default_pty_size())
        .expect("Expected resize to succeed");
    assert_eq!(
        clone_reader_fail_master
            .get_size()
            .expect("Expected get_size to succeed"),
        default_pty_size()
    );
    let mut writer = clone_reader_fail_master
        .take_writer()
        .expect("Expected writer to succeed");
    writer
        .write_all(b"hi")
        .expect("Expected writer to accept bytes");
    assert!(clone_reader_fail_master.try_clone_reader().is_err());

    #[cfg(unix)]
    {
        assert_eq!(clone_reader_fail_master.process_group_leader(), None);
        assert_eq!(clone_reader_fail_master.as_raw_fd(), None);
        assert_eq!(clone_reader_fail_master.tty_name(), None);
    }

    let take_writer_fail_master = TakeWriterFailMasterPty;
    take_writer_fail_master
        .resize(default_pty_size())
        .expect("Expected resize to succeed");
    assert_eq!(
        take_writer_fail_master
            .get_size()
            .expect("Expected get_size to succeed"),
        default_pty_size()
    );
    let mut reader = take_writer_fail_master
        .try_clone_reader()
        .expect("Expected clone reader to succeed");
    let mut buf = [0u8; 1];
    let _ = reader.read(&mut buf).expect("Expected reader to succeed");
    assert!(take_writer_fail_master.take_writer().is_err());

    #[cfg(unix)]
    {
        assert_eq!(take_writer_fail_master.process_group_leader(), None);
        assert_eq!(take_writer_fail_master.as_raw_fd(), None);
        assert_eq!(take_writer_fail_master.tty_name(), None);
    }

    let mut child = StubChild;
    child.kill().expect("Expected kill to succeed");
    let mut killer = child.clone_killer();
    killer.kill().expect("Expected cloned killer to succeed");

    assert!(
        child
            .try_wait()
            .expect("Expected try_wait to succeed")
            .is_none()
    );
    assert!(child.wait().expect("Expected wait to succeed").success());
    assert_eq!(child.process_id(), None);

    let mut reader = ErrorReader;
    assert!(reader.read(&mut buf).is_err());

    let mut writer = FailingWriter;
    assert!(writer.write(b"hi").is_err());
    assert!(writer.flush().is_err());
}

#[test]
fn test_mux_window_debug_includes_fields() {
    let window = stub_window("debug-window", Box::new(io::sink()));
    let guard = window.lock();
    let rendered = format!("{guard:?}");
    assert!(rendered.contains("MuxWindow"));
    assert!(rendered.contains("debug-window"));
}

#[test]
fn test_unix_timestamp_non_negative() {
    assert!(unix_timestamp() >= 0);
}

#[cfg(unix)]
#[test]
fn test_terminal_query_responses_end_to_end() {
    let session_name = format!("tenex-test-backend-{}", uuid::Uuid::new_v4());
    let tmp = std::env::temp_dir();

    let script = concat!(
        "stty raw -echo min 0 time 10; ",
        "printf 'DA:'; printf '\\033[c\\n'; dd bs=1 count=16 2>/dev/null | od -An -tx1; printf '\\n'; ",
        "printf 'CPR:'; printf '\\033[6n\\n'; dd bs=1 count=16 2>/dev/null | od -An -tx1; printf '\\n'; ",
        "printf 'OSC10:'; printf '\\033]10;?\\a\\n'; dd bs=1 count=25 2>/dev/null | od -An -tx1; printf '\\n'; ",
        "printf 'OSC11:'; printf '\\033]11;?\\a\\n'; dd bs=1 count=25 2>/dev/null | od -An -tx1; printf '\\n'; ",
        "stty sane",
    );

    let command = vec!["sh".to_string(), "-c".to_string(), script.to_string()];

    let _ = super::super::server::SessionManager::kill(&session_name);
    super::super::server::SessionManager::create(&session_name, &tmp, Some(&command))
        .expect("create PTY session");

    std::thread::sleep(std::time::Duration::from_secs(4));

    let output = super::super::server::OutputCapture::capture_full_history(&session_name)
        .expect("capture full history");
    let _ = super::super::server::SessionManager::kill(&session_name);

    output
        .contains("DA:")
        .then_some(())
        .expect("expected DA marker");
    output
        .contains("CPR:")
        .then_some(())
        .expect("expected CPR marker");
    output
        .contains("OSC10:")
        .then_some(())
        .expect("expected OSC10 marker");
    output
        .contains("OSC11:")
        .then_some(())
        .expect("expected OSC11 marker");

    let compact: String = output
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();

    // Primary device attributes response starts with ESC [ ?.
    compact
        .contains("1b5b3f")
        .then_some(())
        .expect("expected DA hex output");

    // Text color and background color responses are OSC 10/11.
    compact
        .contains("1b5d3130")
        .then_some(())
        .expect("expected OSC10 hex output");
    compact
        .contains("1b5d3131")
        .then_some(())
        .expect("expected OSC11 hex output");
}

#[test]
fn test_build_terminal_query_responses_da() {
    let parser = vt100::Parser::new(2, 3, 0);
    let bytes = build_terminal_query_responses(parser.screen(), 0, 1, 0, 0);
    assert_eq!(bytes, b"\x1b[?1;0c");
}

#[test]
fn test_build_terminal_query_responses_cpr_uses_one_based_coords() {
    let mut parser = vt100::Parser::new(2, 3, 0);
    parser.process(b"A");
    let bytes = build_terminal_query_responses(parser.screen(), 1, 0, 0, 0);
    assert_eq!(bytes, b"\x1b[1;2R");
}

#[test]
fn test_build_terminal_query_responses_osc10_and_osc11() {
    let parser = vt100::Parser::new(2, 3, 0);
    let bytes = build_terminal_query_responses(parser.screen(), 0, 0, 1, 1);
    assert_eq!(
        bytes,
        b"\x1b]10;rgb:ffff/ffff/ffff\x1b\\\x1b]11;rgb:0000/0000/0000\x1b\\"
    );
}
