//! Minimal helper binary for integration tests.
//!
//! This exercises action dispatch code paths in a non-`cfg(test)` build so instantiation coverage
//! matches what CI enforces.

use std::io::{self, Write as _};

use tenex::action::{dispatch_diff_focused_mode, dispatch_normal_mode};
use tenex::agent::{Agent, ChildConfig, Storage};
use tenex::app::Settings;
use tenex::config::Action;
use tenex::state::DiffFocusedMode;
use tenex::{App, Config, Tab};

fn main() {
    if let Err(err) = run() {
        let _ = writeln!(io::stderr(), "{err:#}");
        std::process::exit(1);
    }
}

fn usage() -> anyhow::Error {
    anyhow::anyhow!("Usage: action_dispatch_probe all")
}

fn make_probe_app(prefix: &str) -> anyhow::Result<App> {
    let worktree_dir = std::env::temp_dir().join(prefix);
    let branch_prefix = format!("{prefix}/");
    let config = Config {
        worktree_dir,
        branch_prefix,
        ..Config::default()
    };

    let state_path = std::env::temp_dir().join(format!("{prefix}-state.json"));
    let mut storage = Storage::with_path(state_path.clone());
    if cfg!(debug_assertions)
        && std::env::var_os("TENEX_TEST_ACTION_DISPATCH_PROBE_STATE_PATH_IS_DIR").is_some()
    {
        let _ = std::fs::create_dir(&state_path);
    }
    storage.save_to(&state_path)?;

    Ok(App::new(config, storage, Settings::default(), false))
}

fn add_agent_with_child(app: &mut App, worktree_path: std::path::PathBuf) {
    let root = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        worktree_path,
    );
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_worktree = root.worktree_path.clone();
    app.data.storage.add(root);

    let child = Agent::new_child(
        "child".to_string(),
        "claude".to_string(),
        root_branch,
        root_worktree,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 1,
            repo_root: None,
        },
    );
    app.data.storage.add(child);
}

fn drive_diff_focused_dispatch(app: &mut App) {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    add_agent_with_child(app, std::env::temp_dir());
    app.data.active_tab = Tab::Diff;
    app.data.ui.set_preview_dimensions(80, 1);
    app.data.ui.set_diff_content("line-0\nline-1\nline-2\n");
    app.enter_mode(DiffFocusedMode.into());

    // Cover both lowercase and uppercase quit variants.
    let _ = dispatch_diff_focused_mode(app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    app.enter_mode(DiffFocusedMode.into());
    let _ = dispatch_diff_focused_mode(app, KeyCode::Char('Q'), KeyModifiers::CONTROL);

    // Cover non-quit flows.
    app.enter_mode(DiffFocusedMode.into());
    let _ = dispatch_diff_focused_mode(app, KeyCode::Tab, KeyModifiers::NONE);
    let _ = dispatch_diff_focused_mode(app, KeyCode::Esc, KeyModifiers::NONE);
    let _ = dispatch_diff_focused_mode(app, KeyCode::Up, KeyModifiers::NONE);
    let _ = dispatch_diff_focused_mode(app, KeyCode::Down, KeyModifiers::NONE);
}

fn drive_normal_mode_dispatch(app: &mut App) {
    add_agent_with_child(app, std::env::temp_dir());

    for action in [
        Action::Help,
        Action::NewAgent,
        Action::NewAgentWithPrompt,
        Action::SelectProjectHeader,
        Action::SelectProjectFirstAgent,
        Action::ScrollUp,
        Action::ScrollDown,
        Action::ScrollTop,
        Action::ScrollBottom,
        Action::SwitchTab,
        Action::CommandPalette,
        Action::Cancel,
    ] {
        let _ = dispatch_normal_mode(app, action);
        app.exit_mode();
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() != ["all"] {
        return Err(usage());
    }

    let pid = std::process::id();
    let suffix = uuid::Uuid::new_v4();
    let prefix = format!("tenex-action-dispatch-probe-{pid}-{suffix}");
    let mut app = make_probe_app(&prefix)?;
    app.data.settings.keyboard_remap_asked = true;

    drive_normal_mode_dispatch(&mut app);
    drive_diff_focused_dispatch(&mut app);

    write_stdout_for_tests(b"ok\n")?;
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
    let Ok(mode) = std::env::var("TENEX_TEST_ACTION_DISPATCH_PROBE_STDOUT_FAIL") else {
        return StdoutFailMode::None;
    };

    match mode.as_str() {
        "write" => StdoutFailMode::Write,
        "flush" => StdoutFailMode::Flush,
        _ => StdoutFailMode::None,
    }
}

fn write_stdout_for_tests(output: &[u8]) -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    {
        let stdout = io::stdout();
        let stdout = stdout.lock();
        let mode = stdout_fail_mode_for_tests();
        let mut stdout = MaybeFailingWriter::new(stdout, mode);
        stdout.write_all(output)?;
        stdout.flush()?;
        Ok(())
    }

    #[cfg(not(debug_assertions))]
    {
        let mut stdout = io::stdout().lock();
        stdout.write_all(output)?;
        stdout.flush()?;
        Ok(())
    }
}
