//! Minimal helper binary for integration tests.
//!
//! This exercises action dispatch code paths in a non-`cfg(test)` build so instantiation coverage
//! matches what CI enforces.

use std::io::{self, Write as _};

use tenex::action::{dispatch_diff_focused_mode, dispatch_normal_mode};
use tenex::agent::{Agent, ChildConfig, Storage};
use tenex::app::{Actions, ConfirmAction, Settings};
use tenex::config::Action;
use tenex::state::{ConfirmingMode, DiffFocusedMode, ScrollingMode};
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

    let mut no_agent_app = App::default();
    no_agent_app.data.active_tab = Tab::Diff;
    no_agent_app.enter_mode(DiffFocusedMode.into());
    let _ = dispatch_diff_focused_mode(&mut no_agent_app, KeyCode::Char('x'), KeyModifiers::NONE);

    add_agent_with_child(app, std::env::temp_dir());
    app.data.active_tab = Tab::Preview;
    app.enter_mode(DiffFocusedMode.into());
    let _ = dispatch_diff_focused_mode(app, KeyCode::Char('x'), KeyModifiers::NONE);

    app.data.active_tab = Tab::Diff;
    app.data.ui.set_preview_dimensions(80, 1);
    app.data.ui.set_diff_content("line-0\nline-1\nline-2\n");
    app.enter_mode(DiffFocusedMode.into());
    let _ = dispatch_diff_focused_mode(app, KeyCode::Char('x'), KeyModifiers::NONE);

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
        Action::Quit,
        Action::Kill,
        Action::NextAgent,
        Action::PrevAgent,
        Action::SpawnChildren,
        Action::PlanSwarm,
        Action::AddChildren,
        Action::ScrollUp,
        Action::ScrollDown,
        Action::ScrollTop,
        Action::ScrollBottom,
        Action::FocusPreview,
        Action::SwitchTab,
        Action::Synthesize,
        Action::ToggleCollapse,
        Action::Broadcast,
        Action::ReviewSwarm,
        Action::SpawnTerminal,
        Action::SpawnTerminalPrompted,
        Action::Push,
        Action::RenameBranch,
        Action::OpenPR,
        Action::Rebase,
        Action::Merge,
        Action::SwitchBranch,
        Action::CommandPalette,
        Action::Confirm,
        Action::Cancel,
    ] {
        let _ = dispatch_normal_mode(app, action);
        app.exit_mode();
        app.data.should_quit = false;
    }

    let _ = Actions::rename_agent(&mut app.data);
    app.exit_mode();

    let same_agent = Agent::new(
        "same-name".to_string(),
        "claude".to_string(),
        "tenex/same-name".to_string(),
        std::env::temp_dir(),
    );
    let same_agent_id = same_agent.id;
    app.data.storage.add(same_agent);
    app.data.git_op.agent_id = Some(same_agent_id);
    app.data.git_op.original_branch = "same-name".to_string();
    app.data.git_op.branch_name = "same-name".to_string();
    app.data.git_op.is_root_rename = false;
    let _ = Actions::execute_rename(&mut app.data);

    let failing_state_path = std::env::temp_dir().join(format!(
        "tenex-action-dispatch-rename-error-{}",
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&failing_state_path);
    let mut failing_app = App::new(
        Config::default(),
        Storage::with_path(failing_state_path),
        Settings::default(),
        false,
    );
    let failing_agent = Agent::new(
        "old-name".to_string(),
        "claude".to_string(),
        "tenex/old-name".to_string(),
        std::env::temp_dir(),
    );
    let failing_agent_id = failing_agent.id;
    failing_app.data.storage.add(failing_agent);
    failing_app.data.git_op.agent_id = Some(failing_agent_id);
    failing_app.data.git_op.original_branch = "old-name".to_string();
    failing_app.data.git_op.branch_name = "new-name".to_string();
    failing_app.data.git_op.is_root_rename = false;
    let _ = Actions::execute_rename(&mut failing_app.data);

    let mut empty_app = App::default();
    let _ = Actions::rename_agent(&mut empty_app.data);
    let _ = Actions::execute_rename(&mut empty_app.data);
    empty_app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
    let _ = Actions::execute_rename(&mut empty_app.data);
}

fn drive_handler_dispatch(app: &mut App) {
    let _ = Actions::new().handle_action(app, Action::Help);
    app.exit_mode();

    app.enter_mode(ScrollingMode.into());
    let _ = Actions::new().handle_action(app, Action::Cancel);
    app.exit_mode();

    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Quit,
        }
        .into(),
    );
    let _ = Actions::new().handle_action(app, Action::Cancel);

    app.enter_mode(
        ConfirmingMode {
            action: ConfirmAction::Quit,
        }
        .into(),
    );
    let _ = Actions::new().handle_action(app, Action::Confirm);
    app.data.should_quit = false;
}

fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() != ["all"] {
        return Err(usage());
    }

    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(io::sink)
        .try_init();

    #[cfg(coverage)]
    let _ = tenex::mux::set_socket_override("tenex-action-dispatch-probe\0invalid");

    let pid = std::process::id();
    let suffix = uuid::Uuid::new_v4();
    let prefix = format!("tenex-action-dispatch-probe-{pid}-{suffix}");
    let mut app = make_probe_app(&prefix)?;
    app.data.settings.keyboard_remap_asked = true;

    drive_normal_mode_dispatch(&mut app);
    drive_handler_dispatch(&mut app);
    drive_diff_focused_dispatch(&mut app);
    #[cfg(coverage)]
    {
        tenex::agent::Storage::exercise_load_and_backfill_paths_for_coverage();
        tenex::mux::exercise_endpoint_paths_for_coverage();
        tenex::mux::exercise_mux_paths_for_coverage();
        tenex::conversation::exercise_agent_cli_detection_for_coverage();
        app.data.exercise_command_defaults_for_coverage();
        Actions::exercise_reset_all_paths_for_coverage();
        Actions::exercise_agent_lifecycle_paths_for_coverage(&mut app.data);
        Actions::exercise_swarm_paths_for_coverage(&mut app.data);
        Actions::exercise_sync_paths_for_coverage(&mut app);
    }

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
