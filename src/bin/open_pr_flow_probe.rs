//! Minimal helper binary for integration tests.
//!
//! This runs the open PR flow for a synthetic agent inside a provided git worktree and prints the
//! resulting mode. Integration tests use it to exercise the non-`cfg(test)` instantiation of the
//! open PR handler without relying on the network or a real `gh` binary.

use std::io::{self, Write as _};

use tenex::agent::Agent;
use tenex::agent::Storage;
use tenex::app::{Actions, App, Settings};
use tenex::config::Config;
use tenex::state::AppMode;

fn main() {
    if let Err(err) = run() {
        let _ = writeln!(io::stderr(), "{err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        anyhow::bail!("Usage: open_pr_flow_probe <worktree-path> <branch-name>");
    }

    let worktree_path = std::path::PathBuf::from(&args[0]);
    let branch_name = args[1].to_string_lossy().to_string();

    let mut app = App::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    );
    let agent = Agent::new(
        "agent".to_string(),
        "claude".to_string(),
        branch_name,
        worktree_path.clone(),
    );
    app.data.storage.add(agent);
    app.set_cwd_project_root(Some(worktree_path));

    let next = Actions::open_pr_flow(&mut app.data)?;
    let output = match next {
        AppMode::ConfirmPushForPR(_) => "confirm-push-for-pr\n".to_string(),
        AppMode::ErrorModal(mode) => format!("error-modal\n{}\n", mode.message),
        other => format!("other-mode\n{other:?}\n"),
    };

    write_stdout_for_tests(&output)?;
    Ok(())
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

#[cfg(debug_assertions)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StdoutFailMode {
    None,
    Write,
    Flush,
}

#[cfg(debug_assertions)]
fn stdout_fail_mode_for_tests() -> StdoutFailMode {
    let Ok(mode) = std::env::var("TENEX_TEST_OPEN_PR_FLOW_PROBE_STDOUT_FAIL") else {
        return StdoutFailMode::None;
    };

    match mode.as_str() {
        "write" => StdoutFailMode::Write,
        "flush" => StdoutFailMode::Flush,
        _ => StdoutFailMode::None,
    }
}

fn write_stdout_for_tests(output: &str) -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    {
        let stdout = io::stdout();
        let stdout = stdout.lock();
        let mode = stdout_fail_mode_for_tests();
        let mut stdout = MaybeFailingWriter::new(stdout, mode);
        stdout.write_all(output.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }

    #[cfg(not(debug_assertions))]
    {
        let mut stdout = io::stdout().lock();
        stdout.write_all(output.as_bytes())?;
        stdout.flush()?;
        Ok(())
    }
}
