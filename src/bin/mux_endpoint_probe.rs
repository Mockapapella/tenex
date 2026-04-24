//! Minimal helper binary for integration tests.
//!
//! This intentionally prints only the mux socket display string, so integration
//! tests can validate default endpoint selection without mutating the current
//! process environment.

use std::io::{self, Write as _};

fn main() {
    if let Err(err) = run() {
        let _ = writeln!(io::stderr(), "{err}");
        std::process::exit(1);
    }
}

#[cfg(debug_assertions)]
fn maybe_force_socket_override_for_tests() -> anyhow::Result<()> {
    let Some(mode) = std::env::var_os("TENEX_TEST_MUX_ENDPOINT_PROBE_FORCE_INVALID_SOCKET") else {
        return Ok(());
    };

    let mode = mode.to_string_lossy();
    let override_value = match mode.as_ref() {
        "empty" => "   ".to_string(),
        "namespaced-too-long" => "x".repeat(4096),
        _ => {
            // Environment variables cannot contain NUL bytes. Injecting one directly guarantees that
            // endpoint parsing fails, exercising the error path in `socket_display`.
            "/tmp/tenex-mux-test\0socket".to_string()
        }
    };

    tenex::mux::set_socket_override(override_value.as_str())?;
    Ok(())
}

#[cfg(not(debug_assertions))]
fn maybe_force_socket_override_for_tests() -> anyhow::Result<()> {
    Ok(())
}

fn run() -> anyhow::Result<()> {
    maybe_force_socket_override_for_tests()?;

    let display = tenex::mux::socket_display()?;
    write_stdout_for_tests(&display)?;
    Ok(())
}

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StdoutFailMode {
    None,
    Write,
    Flush,
}

#[cfg(debug_assertions)]
fn stdout_fail_mode_for_tests() -> StdoutFailMode {
    let Ok(mode) = std::env::var("TENEX_TEST_MUX_ENDPOINT_PROBE_STDOUT_FAIL") else {
        return StdoutFailMode::None;
    };

    match mode.as_str() {
        "write" => StdoutFailMode::Write,
        "flush" => StdoutFailMode::Flush,
        _ => StdoutFailMode::None,
    }
}

#[cfg(debug_assertions)]
struct MaybeFailingWriter<W> {
    inner: W,
    mode: StdoutFailMode,
}

#[cfg(debug_assertions)]
impl<W> MaybeFailingWriter<W> {
    const fn new(inner: W, mode: StdoutFailMode) -> Self {
        Self { inner, mode }
    }
}

#[cfg(debug_assertions)]
impl<W: std::io::Write> std::io::Write for MaybeFailingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.mode == StdoutFailMode::Write {
            return Err(io::Error::other("forced stdout write error"));
        }

        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.mode == StdoutFailMode::Flush {
            return Err(io::Error::other("forced stdout flush error"));
        }

        self.inner.flush()
    }
}

fn write_stdout_for_tests(display: &str) -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    {
        let stdout = io::stdout();
        let stdout = stdout.lock();
        let mode = stdout_fail_mode_for_tests();
        let mut stdout = MaybeFailingWriter::new(stdout, mode);
        stdout.write_all(display.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }

    #[cfg(not(debug_assertions))]
    {
        let mut stdout = io::stdout().lock();
        stdout.write_all(display.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }
}
