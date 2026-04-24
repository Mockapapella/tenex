//! Minimal helper binary for integration tests.
//!
//! This probes Tenex's Codex session discovery logic in the non-`cfg(test)` build by attempting to
//! detect the most recent session id for a provided working directory.

use std::collections::HashSet;
use std::io::{self, Write as _};
use std::time::{Duration, SystemTime};

fn write_probe_output(out: &mut dyn io::Write, found: Option<&str>) -> anyhow::Result<()> {
    match found {
        Some(id) => out.write_all(format!("{id}\n").as_bytes())?,
        None => out.write_all(b"NONE\n")?,
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        let _ = writeln!(io::stderr(), "{err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(args.len() == 1 || args.len() == 2) {
        anyhow::bail!("Usage: codex_session_probe <workdir> [max-wait-ms]");
    }

    let workdir = std::path::PathBuf::from(&args[0]);
    let max_wait = args
        .get(1)
        .map(|raw| raw.to_string_lossy().parse::<u64>())
        .transpose()?
        .map_or(Duration::from_millis(0), Duration::from_millis);

    let exclude_ids: HashSet<String> = HashSet::new();
    let found = tenex::conversation::try_detect_codex_session_id(
        &workdir,
        SystemTime::UNIX_EPOCH,
        &exclude_ids,
        max_wait,
    );

    write_stdout_for_tests(found.as_deref())?;

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
    let Ok(mode) = std::env::var("TENEX_TEST_CODEX_SESSION_PROBE_STDOUT_FAIL") else {
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
impl<W: io::Write> io::Write for MaybeFailingWriter<W> {
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

fn write_stdout_for_tests(found: Option<&str>) -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    {
        let stdout = io::stdout();
        let stdout = stdout.lock();
        let mode = stdout_fail_mode_for_tests();
        let mut stdout = MaybeFailingWriter::new(stdout, mode);
        write_probe_output(&mut stdout, found)?;
        stdout.flush()?;
        Ok(())
    }

    #[cfg(not(debug_assertions))]
    {
        let mut stdout = io::stdout().lock();
        write_probe_output(&mut stdout, found)?;
        stdout.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_probe_output_propagates_write_errors() {
        #[derive(Default)]
        struct FailingWriteWriter;

        impl io::Write for FailingWriteWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("write failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = FailingWriteWriter::default();
        writer.flush().unwrap();
        let err = write_probe_output(&mut writer, Some("abc")).unwrap_err();
        assert!(err.to_string().contains("write failed"));
    }

    #[test]
    fn test_write_probe_output_propagates_write_errors_for_none() {
        #[derive(Default)]
        struct FailingWriteWriter;

        impl io::Write for FailingWriteWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("write failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = FailingWriteWriter::default();
        writer.flush().unwrap();
        let err = write_probe_output(&mut writer, None).unwrap_err();
        assert!(err.to_string().contains("write failed"));
    }
}
