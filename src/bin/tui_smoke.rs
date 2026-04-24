//! Minimal driver binary that runs `tenex::tui::run` in a PTY-friendly way.
use std::io::IsTerminal;
use tenex::agent::Storage;
use tenex::app::Settings;
use tenex::{App, Config, Tab};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let force_stdout_not_tty = cfg!(debug_assertions)
        && std::env::var_os("TENEX_TEST_TUI_SMOKE_FORCE_STDOUT_NOT_TTY").is_some();
    let stdin_is_tty = if force_stdout_not_tty {
        true
    } else {
        std::io::stdin().is_terminal()
    };
    let stdout_is_tty = if force_stdout_not_tty {
        false
    } else {
        std::io::stdout().is_terminal()
    };

    if !(stdin_is_tty && stdout_is_tty) {
        anyhow::bail!("tui_smoke requires a TTY");
    }

    let pid = std::process::id();
    let suffix = uuid::Uuid::new_v4();
    let prefix = format!("tenex-tui-smoke-{pid}-{suffix}");

    let worktree_dir = std::env::temp_dir().join(&prefix);
    let branch_prefix = format!("{prefix}/");
    let config = Config {
        worktree_dir,
        branch_prefix,
        ..Config::default()
    };

    let state_path = std::env::temp_dir().join(format!("{prefix}-state.json"));
    if cfg!(debug_assertions)
        && std::env::var_os("TENEX_TEST_TUI_SMOKE_FORCE_STATE_SAVE_ERROR").is_some()
    {
        // Force the subsequent state save to fail in a deterministic way.
        let _ = std::fs::create_dir(&state_path);
    }
    let mut storage = Storage::with_path(state_path.clone());
    storage.save_to(&state_path)?;

    let mut app = App::new(config, storage, Settings::default(), false);
    app.data.settings.keyboard_remap_asked = true;
    app.data.active_tab = Tab::Diff;
    app.data.should_quit = true;
    app.validate_selection();

    let _ = tenex::tui::run(app)?;
    Ok(())
}
