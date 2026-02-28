//! Terminal User Interface for Tenex

mod input;
mod render;

use crate::app::{Actions, App, Event, Handler, Tab};
use crate::state::AppMode;
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{
            self as crossterm_event, DisableMouseCapture, EnableMouseCapture, KeyEventKind,
            KeyboardEnhancementFlags, MouseButton, MouseEventKind, PopKeyboardEnhancementFlags,
            PushKeyboardEnhancementFlags,
        },
        execute,
        terminal::{
            EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
            supports_keyboard_enhancement,
        },
    },
    layout::Rect,
};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tracing::{info, warn};

const UI_FRAME_INTERVAL_MS: u64 = 33;
const PREVIEW_SMOOTH_REFRESH_MS: u64 = 33;
const AGENT_STATUS_SYNC_INTERVAL_MS: u64 = 500;
const MIN_OUTPUT_REFRESH_MS: u64 = 16;
const MIN_PANE_ACTIVITY_SYNC_MS: u64 = 500;
const STATE_FILE_SYNC_INTERVAL_MS: u64 = 250;
const OSC52_MAX_BYTES: usize = 100_000;

type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;
type DrainedEvents = (Vec<String>, Option<(u16, u16)>, bool);

fn mouse_capture_enabled() -> bool {
    !env_var_truthy(std::env::var("TENEX_DISABLE_MOUSE").ok().as_deref())
}

fn env_var_truthy(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };

    let value = value.trim();
    value == "1"
        || value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("yes")
        || value.eq_ignore_ascii_case("on")
}

fn enter_tui_screen(stdout: &mut impl io::Write, enable_mouse_capture: bool) -> Result<()> {
    execute!(stdout, EnterAlternateScreen)?;
    if enable_mouse_capture {
        execute!(stdout, EnableMouseCapture)?;
    }
    Ok(())
}

fn flush_pending_clipboard(stdout: &mut impl io::Write, app: &mut App) {
    let Some(text) = app.data.ui.pending_clipboard.take() else {
        return;
    };

    if text.is_empty() {
        return;
    }

    if text.len() > OSC52_MAX_BYTES {
        app.set_status(format!(
            "Selection too large to copy ({} bytes; max {OSC52_MAX_BYTES})",
            text.len()
        ));
        return;
    }

    match write_osc52_clipboard(stdout, &text) {
        Ok(()) => {
            let line_count = text.lines().count().max(1);
            let suffix = if line_count == 1 { "" } else { "s" };
            app.set_status(format!("Copied {line_count} line{suffix}"));
        }
        Err(err) => {
            app.set_status(format!("Copy failed: {err}"));
        }
    }
}

fn write_osc52_clipboard(stdout: &mut impl io::Write, content: &str) -> io::Result<()> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    let encoded = STANDARD.encode(content.as_bytes());
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StateFileStamp {
    modified: SystemTime,
    len: u64,
}

fn state_file_stamp(path: &Path) -> Option<StateFileStamp> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    Some(StateFileStamp {
        modified,
        len: metadata.len(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectedSidebarKey {
    Agent(uuid::Uuid),
    Project(PathBuf),
}

fn selected_sidebar_key(app: &App) -> Option<SelectedSidebarKey> {
    match app.data.selected_sidebar_item()? {
        crate::app::SidebarItem::Agent(agent) => {
            Some(SelectedSidebarKey::Agent(agent.info.agent.id))
        }
        crate::app::SidebarItem::Project(project) => {
            Some(SelectedSidebarKey::Project(project.root))
        }
    }
}

fn restore_sidebar_selection(app: &mut App, key: Option<SelectedSidebarKey>) {
    let Some(key) = key else {
        app.validate_selection();
        return;
    };

    let items = app.data.sidebar_items();
    let index = items.iter().position(|item| match (item, &key) {
        (crate::app::SidebarItem::Agent(agent), SelectedSidebarKey::Agent(id)) => {
            agent.info.agent.id == *id
        }
        (crate::app::SidebarItem::Project(project), SelectedSidebarKey::Project(root)) => {
            &project.root == root
        }
        _ => false,
    });

    if let Some(index) = index {
        app.data.selected = index;
    }

    app.validate_selection();
}

struct StateFileTracker {
    path: PathBuf,
    last_stamp: Option<StateFileStamp>,
    last_check: Instant,
}

impl StateFileTracker {
    fn new(app: &App) -> Self {
        let path = app.data.storage.resolved_state_path();
        let last_stamp = state_file_stamp(&path);
        Self {
            path,
            last_stamp,
            last_check: Instant::now(),
        }
    }

    fn maybe_reload_state(&mut self, app: &mut App) -> bool {
        if self.last_check.elapsed() < Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS) {
            return false;
        }
        self.last_check = Instant::now();

        let Some(stamp) = state_file_stamp(&self.path) else {
            return false;
        };

        if self.last_stamp.is_some_and(|last| last == stamp) {
            return false;
        }

        let previous_key = selected_sidebar_key(app);
        let previous_custom_path = app.data.storage.state_path.clone();

        let Ok(mut storage) = crate::agent::Storage::load_from(&self.path) else {
            return false;
        };

        storage.state_path = previous_custom_path;
        storage.apply_local_agent_fields_from(&app.data.storage);
        app.data.storage = storage;
        restore_sidebar_selection(app, previous_key);

        self.last_stamp = Some(stamp);
        true
    }
}

/// Run the TUI application
///
/// Returns `Ok(Some(UpdateInfo))` if the user accepted an update prompt and the
/// binary should reinstall and restart. Otherwise returns `Ok(None)` on normal exit.
///
/// # Errors
/// Returns an error if the terminal cannot be initialized or restored (raw mode,
/// alternate screen), or if the main event loop fails to poll input
/// or render frames.
pub fn run(mut app: App) -> Result<Option<UpdateInfo>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    enter_tui_screen(&mut stdout, mouse_capture_enabled())?;

    // Enable Kitty keyboard protocol to disambiguate Ctrl+M from Enter
    // This is supported by modern terminals: kitty, foot, WezTerm, alacritty (0.13+)
    let keyboard_enhancement_enabled = if supports_keyboard_enhancement().unwrap_or(false) {
        info!("Terminal supports keyboard enhancement protocol - Ctrl+M will work");
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )
        .is_ok()
    } else {
        warn!("Terminal does not support keyboard enhancement protocol - Ctrl+M will act as Enter");
        false
    };

    // Update the app with keyboard enhancement status
    app.data.keyboard_enhancement_supported = keyboard_enhancement_enabled;

    // Show keyboard remap prompt if terminal doesn't support enhancement
    // and user hasn't been asked yet
    if matches!(app.mode, AppMode::Normal(_)) && app.should_show_keyboard_remap_prompt() {
        app.show_keyboard_remap_prompt();
    }

    // If no higher-priority modal is open, show any deferred "What's New" changelog modal.
    if matches!(app.mode, AppMode::Normal(_))
        && let Some(pending) = app.data.pending_changelog.take()
    {
        app.apply_mode(pending.into());
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let event_handler = Handler::new(UI_FRAME_INTERVAL_MS);
    let action_handler = Actions::new();

    let result = run_loop(&mut terminal, &mut app, &event_handler, action_handler);

    // Pop keyboard enhancement before cleanup (only if we enabled it)
    if keyboard_enhancement_enabled
        && let Err(e) = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)
    {
        warn!("Failed to pop keyboard enhancement flags: {e}");
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn send_batched_keys_to_mux(app: &App, batched_keys: &[String]) {
    if batched_keys.is_empty() {
        return;
    }

    if let Some(agent) = app.selected_agent() {
        let target = agent.window_index.map_or_else(
            || agent.mux_session.clone(),
            |idx| format!("{}:{}", agent.mux_session, idx),
        );
        // Use synchronous call so the mux processes keys before we capture.
        let _ = crate::mux::SessionManager::new().send_keys_batch(&target, batched_keys);
    }
}

fn send_keys_and_flush_clipboard(
    terminal: &mut TuiTerminal,
    app: &mut App,
    batched_keys: &[String],
) {
    send_batched_keys_to_mux(app, batched_keys);
    flush_pending_clipboard(terminal.backend_mut(), app);
}

fn init_preview_dimensions(terminal: &TuiTerminal, app: &mut App, action_handler: Actions) {
    if app.data.ui.preview_dimensions.is_some() && app.data.ui.terminal_dimensions.is_some() {
        return;
    }

    let Ok(size) = terminal.size() else {
        return;
    };

    app.set_terminal_dimensions(size.width, size.height);

    let area = Rect::new(0, 0, size.width, size.height);
    let (width, height) = render::calculate_preview_dimensions(area);
    app.set_preview_dimensions(width, height);
    action_handler.resize_agent_windows(app);
    app.ensure_agent_list_scroll();
}

fn drain_events(
    terminal: &TuiTerminal,
    app: &mut App,
    event_handler: &Handler,
) -> Result<DrainedEvents> {
    let mut last_resize: Option<(u16, u16)> = None;
    let mut batched_keys: Vec<String> = Vec::new();
    let mut flushed_batched_keys = false;

    let size = terminal
        .size()
        .unwrap_or_else(|_| ratatui::layout::Size::new(0, 0));
    let mut frame_area = Rect::new(0, 0, size.width, size.height);

    loop {
        match event_handler.next()? {
            Event::Tick => {
                break;
            }
            Event::Key(key) => {
                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    input::handle_key_event(app, key.code, key.modifiers, &mut batched_keys)?;
                }
            }
            Event::Mouse(mouse) => {
                // If we're attached and have batched keys, flush them before applying any
                // click-driven selection changes so keys go to the intended agent.
                if !batched_keys.is_empty() && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                {
                    send_batched_keys_to_mux(app, &batched_keys);
                    batched_keys.clear();
                    flushed_batched_keys = true;
                }

                input::handle_mouse_event(app, mouse, frame_area, &mut batched_keys)?;
            }
            Event::Resize(w, h) => {
                last_resize = Some((w, h));
                frame_area = Rect::new(0, 0, w, h);
            }
        }

        if !crossterm_event::poll(Duration::ZERO)? {
            break;
        }
    }

    Ok((batched_keys, last_resize, flushed_batched_keys))
}

fn compute_preview_refresh_interval(
    config_poll_interval_ms: u64,
    active_tab: Tab,
    preview_follow: bool,
) -> Duration {
    let base_refresh = Duration::from_millis(config_poll_interval_ms.max(MIN_OUTPUT_REFRESH_MS));
    if active_tab == Tab::Preview && preview_follow {
        base_refresh.min(Duration::from_millis(PREVIEW_SMOOTH_REFRESH_MS))
    } else {
        base_refresh
    }
}

fn maybe_refresh_preview(
    app: &mut App,
    action_handler: Actions,
    needs_content_update: bool,
    sent_keys_in_preview: bool,
    last_preview_update: &mut Instant,
) {
    let preview_refresh_interval = compute_preview_refresh_interval(
        app.data.config.poll_interval_ms,
        app.data.active_tab,
        app.data.ui.preview_follow,
    );

    let preview_visible =
        app.data.active_tab == Tab::Preview || matches!(&app.mode, AppMode::PreviewFocused(_));

    let preview_due = last_preview_update.elapsed() >= preview_refresh_interval;
    // When scrolled up (preview_follow = false), keep the captured buffer stable.
    if preview_visible
        && (needs_content_update
            || (app.data.ui.preview_follow && (sent_keys_in_preview || preview_due)))
    {
        let _ = action_handler.update_preview(app);
        *last_preview_update = Instant::now();
    }
}

fn run_loop(
    terminal: &mut TuiTerminal,
    app: &mut App,
    event_handler: &Handler,
    action_handler: Actions,
) -> Result<Option<UpdateInfo>> {
    init_preview_dimensions(terminal, app, action_handler);

    let mut state_tracker = StateFileTracker::new(app);
    // Track selection to detect changes
    let mut last_selected = app.data.selected;
    // Track active tab so we can refresh content when switching tabs
    let mut last_tab = app.data.active_tab;
    // Force initial preview/diff update
    let mut needs_content_update = true;
    let mut last_preview_follow = app.data.ui.preview_follow;
    let mut last_preview_update = Instant::now();
    // Diff refresh is expensive; throttle tick-based updates.
    let diff_refresh_interval = Duration::from_millis(1000);
    let mut last_diff_update = Instant::now();
    // Commits refresh is cheap; still throttle tick-based updates.
    let commits_refresh_interval = Duration::from_millis(1000);
    let mut last_commits_update = Instant::now();
    let mut last_status_sync = Instant::now();
    let mut last_pane_activity_sync = Instant::now();

    loop {
        // If we returned to normal mode and still need to show the keyboard prompt,
        // display it now (e.g., after dismissing another startup modal).
        if matches!(app.mode, AppMode::Normal(_)) && app.should_show_keyboard_remap_prompt() {
            app.show_keyboard_remap_prompt();
        }

        let (batched_keys, last_resize, flushed_batched_keys) =
            drain_events(terminal, app, event_handler)?;

        // Send batched keys to the mux in one command (much faster than per-keystroke)
        let sent_keys_in_preview = flushed_batched_keys
            || (!batched_keys.is_empty() && matches!(app.mode, AppMode::PreviewFocused(_)));
        send_keys_and_flush_clipboard(terminal, app, &batched_keys);

        // Apply final resize if any occurred
        if let Some((width, height)) = last_resize {
            app.set_terminal_dimensions(width, height);
            let (preview_width, preview_height) =
                render::calculate_preview_dimensions(Rect::new(0, 0, width, height));
            if app.data.ui.preview_dimensions != Some((preview_width, preview_height)) {
                app.set_preview_dimensions(preview_width, preview_height);
                action_handler.resize_agent_windows(app);
                app.ensure_agent_list_scroll();
            }
        }

        if state_tracker.maybe_reload_state(app) {
            needs_content_update = true;
            last_selected = app.data.selected;
        }

        // Detect selection change
        if app.data.selected != last_selected {
            last_selected = app.data.selected;
            needs_content_update = true;

            // Treat selecting an agent as "checking" its output for the unseen-waiting indicator.
            if let Some(agent_id) = app.selected_agent().map(|agent| agent.id) {
                app.data.ui.mark_agent_pane_seen(agent_id);
            }
        }
        // Detect tab change
        if app.data.active_tab != last_tab {
            last_tab = app.data.active_tab;
            needs_content_update = true;
        }
        // Detect follow-mode changes so we can adjust refresh strategy immediately.
        if app.data.ui.preview_follow != last_preview_follow {
            last_preview_follow = app.data.ui.preview_follow;
            needs_content_update = true;
        }

        maybe_refresh_preview(
            app,
            action_handler,
            needs_content_update,
            sent_keys_in_preview,
            &mut last_preview_update,
        );

        // Diff refresh is expensive; throttle it while still updating promptly on selection/tab changes.
        let diff_due = last_diff_update.elapsed() >= diff_refresh_interval;
        if needs_content_update || diff_due || app.data.ui.diff_force_refresh {
            if app.data.active_tab == Tab::Diff || app.data.ui.diff_force_refresh {
                let _ = action_handler.update_diff(app);
            } else {
                let _ = action_handler.update_diff_digest(app);
            }
            last_diff_update = Instant::now();
        }

        let commits_due = last_commits_update.elapsed() >= commits_refresh_interval;
        if needs_content_update || commits_due {
            if app.data.active_tab == Tab::Commits {
                let _ = action_handler.update_commits(app);
            } else {
                let _ = action_handler.update_commits_digest(app);
            }
            last_commits_update = Instant::now();
        }

        needs_content_update = false;

        // Draw ONCE after draining all queued events
        terminal.draw(|frame| render::render(frame, app))?;

        // Diff-check each pane less frequently than the UI frame rate.
        let pane_activity_interval = Duration::from_millis(
            app.data
                .config
                .poll_interval_ms
                .max(MIN_PANE_ACTIVITY_SYNC_MS),
        );
        if last_pane_activity_sync.elapsed() >= pane_activity_interval {
            let _ = action_handler.sync_agent_pane_activity(app);
            last_pane_activity_sync = Instant::now();
        }

        // Sync agent status less frequently (session listing is relatively expensive).
        if last_status_sync.elapsed() >= Duration::from_millis(AGENT_STATUS_SYNC_INTERVAL_MS) {
            let _ = action_handler.sync_agent_status(app);
            last_status_sync = Instant::now();
        }

        if let AppMode::UpdateRequested(state) = &app.mode {
            return Ok(Some(state.info.clone()));
        }

        if app.data.should_quit {
            break;
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::keycode_to_input_sequence;
    use crate::agent::Agent;
    use crate::agent::Storage;
    use crate::config::Config;
    use crate::state::*;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use std::path::PathBuf;

    /// Helper struct that cleans up test worktrees and branches on drop
    struct TestCleanup {
        repo_path: PathBuf,
        branch_prefix: String,
    }

    impl TestCleanup {
        fn new(branch_prefix: &str) -> Self {
            Self {
                repo_path: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                branch_prefix: branch_prefix.to_string(),
            }
        }
    }

    impl Drop for TestCleanup {
        fn drop(&mut self) {
            // Clean up any worktrees/branches created by this test
            if let Ok(repo) = git2::Repository::open(&self.repo_path) {
                // Remove worktrees with our prefix
                if let Ok(worktrees) = repo.worktrees() {
                    for wt_name in worktrees.iter().flatten() {
                        if wt_name.starts_with(&self.branch_prefix.replace('/', "-"))
                            && let Ok(wt) = repo.find_worktree(wt_name)
                        {
                            if let Some(path) = wt.path().to_str()
                                && let Err(e) = std::fs::remove_dir_all(path)
                            {
                                eprintln!(
                                    "Warning: Failed to remove test worktree dir {path}: {e}"
                                );
                            }
                            if let Err(e) =
                                wt.prune(Some(git2::WorktreePruneOptions::new().working_tree(true)))
                            {
                                eprintln!("Warning: Failed to prune test worktree {wt_name}: {e}");
                            }
                        }
                    }
                }

                // Remove branches with our prefix
                if let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) {
                    for branch_result in branches {
                        if let Ok((mut branch, _)) = branch_result
                            && let Some(name) = branch.name().ok().flatten()
                            && name.starts_with(&self.branch_prefix)
                        {
                            let branch_name = name.to_string();
                            if let Err(e) = branch.delete() {
                                eprintln!(
                                    "Warning: Failed to delete test branch {branch_name}: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn create_test_config() -> Config {
        // Use a unique temp directory for each test process to avoid conflicts
        // and prevent tests from creating worktrees in the real ~/.tenex directory
        let pid = std::process::id();
        Config {
            worktree_dir: PathBuf::from(format!("/tmp/tenex-test-{pid}")),
            branch_prefix: format!("tenex-test-{pid}/"),
            ..Config::default()
        }
    }

    fn create_test_app() -> App {
        App::new(
            create_test_config(),
            Storage::default(),
            crate::app::Settings::default(),
            false,
        )
    }

    #[test]
    fn test_env_var_truthy_defaults_to_false() {
        assert!(!env_var_truthy(None));
    }

    #[test]
    fn test_env_var_truthy_accepts_truthy_values() {
        for value in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(env_var_truthy(Some(value)), "expected truthy for {value:?}");
        }
    }

    #[test]
    fn test_env_var_truthy_rejects_falsy_values() {
        for value in ["", "0", "false", "no", "off"] {
            assert!(!env_var_truthy(Some(value)), "expected falsy for {value:?}");
        }
    }

    #[test]
    fn test_enter_tui_screen_writes_output() -> Result<()> {
        let mut buffer: Vec<u8> = Vec::new();
        enter_tui_screen(&mut buffer, false)?;
        assert!(!buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_enter_tui_screen_mouse_capture_writes_more_output() -> Result<()> {
        let mut without_mouse: Vec<u8> = Vec::new();
        enter_tui_screen(&mut without_mouse, false)?;

        let mut with_mouse: Vec<u8> = Vec::new();
        enter_tui_screen(&mut with_mouse, true)?;

        assert!(with_mouse.len() >= without_mouse.len());
        #[cfg(not(windows))]
        assert!(with_mouse.len() > without_mouse.len());
        Ok(())
    }

    #[test]
    fn test_flush_pending_clipboard_writes_osc52_and_sets_status_for_multiline() {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD;

        let mut app = create_test_app();
        let content = "line1\nline2";
        app.data.ui.pending_clipboard = Some(content.to_string());

        let mut buffer: Vec<u8> = Vec::new();
        flush_pending_clipboard(&mut buffer, &mut app);

        let expected = format!("\x1b]52;c;{}\x07", STANDARD.encode(content.as_bytes()));
        assert_eq!(buffer.as_slice(), expected.as_bytes());
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Copied 2 lines")
        );
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn test_flush_pending_clipboard_sets_status_for_single_line() {
        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("line1".to_string());

        let mut buffer: Vec<u8> = Vec::new();
        flush_pending_clipboard(&mut buffer, &mut app);

        assert_eq!(app.data.ui.status_message.as_deref(), Some("Copied 1 line"));
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn test_flush_pending_clipboard_rejects_oversize_selection() {
        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("a".repeat(OSC52_MAX_BYTES.saturating_add(1)));

        let mut buffer: Vec<u8> = Vec::new();
        flush_pending_clipboard(&mut buffer, &mut app);

        assert!(buffer.is_empty());
        assert!(
            app.data
                .ui
                .status_message
                .as_deref()
                .is_some_and(|message| message.contains("Selection too large"))
        );
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn test_flush_pending_clipboard_reports_write_errors() {
        struct FailingWriter;

        impl std::io::Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("nope"))
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("nope"))
            }
        }

        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("line1".to_string());

        flush_pending_clipboard(&mut FailingWriter, &mut app);

        assert!(
            app.data
                .ui
                .status_message
                .as_deref()
                .is_some_and(|message| message.starts_with("Copy failed:"))
        );
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn test_flush_pending_clipboard_noops_when_none() {
        let mut app = create_test_app();
        app.data.ui.set_status("existing");

        let mut buffer: Vec<u8> = Vec::new();
        flush_pending_clipboard(&mut buffer, &mut app);

        assert!(buffer.is_empty());
        assert_eq!(app.data.ui.status_message.as_deref(), Some("existing"));
    }

    #[test]
    fn test_flush_pending_clipboard_noops_when_empty() {
        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some(String::new());

        let mut buffer: Vec<u8> = Vec::new();
        flush_pending_clipboard(&mut buffer, &mut app);

        assert!(buffer.is_empty());
        assert!(app.data.ui.pending_clipboard.is_none());
        assert!(app.data.ui.status_message.is_none());
    }

    #[test]
    fn test_flush_pending_clipboard_reports_flush_errors() {
        struct FlushFails {
            buffer: Vec<u8>,
        }

        impl std::io::Write for FlushFails {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.buffer.extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("nope"))
            }
        }

        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("line1".to_string());

        flush_pending_clipboard(&mut FlushFails { buffer: Vec::new() }, &mut app);

        assert!(
            app.data
                .ui
                .status_message
                .as_deref()
                .is_some_and(|message| message.starts_with("Copy failed:"))
        );
        assert!(app.data.ui.pending_clipboard.is_none());
    }

    #[test]
    fn test_write_osc52_clipboard_roundtrips_base64() -> Result<()> {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD;

        let content = "line1\nλ line2\n";
        let mut buffer: Vec<u8> = Vec::new();
        write_osc52_clipboard(&mut buffer, content)?;

        let prefix = b"\x1b]52;c;";
        assert!(buffer.starts_with(prefix));
        assert_eq!(buffer.last().copied(), Some(b'\x07'));

        let encoded = &buffer[prefix.len()..buffer.len().saturating_sub(1)];
        let encoded = std::str::from_utf8(encoded)?;
        let decoded = STANDARD.decode(encoded)?;
        assert_eq!(decoded, content.as_bytes());
        Ok(())
    }

    #[test]
    fn test_flush_pending_clipboard_payload_decodes_to_original() -> Result<()> {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD;

        let mut app = create_test_app();
        let content = "echo 1\nprintf '%s' \"hi\"";
        app.data.ui.pending_clipboard = Some(content.to_string());

        let mut buffer: Vec<u8> = Vec::new();
        flush_pending_clipboard(&mut buffer, &mut app);

        let prefix = b"\x1b]52;c;";
        assert!(buffer.starts_with(prefix));
        assert_eq!(buffer.last().copied(), Some(b'\x07'));
        let encoded = &buffer[prefix.len()..buffer.len().saturating_sub(1)];
        let encoded = std::str::from_utf8(encoded)?;
        let decoded = STANDARD.decode(encoded)?;
        assert_eq!(decoded, content.as_bytes());
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Copied 2 lines")
        );
        Ok(())
    }

    fn create_test_app_with_cleanup() -> (App, TestCleanup) {
        let config = create_test_config();
        let cleanup = TestCleanup::new(&config.branch_prefix);
        (
            App::new(
                config,
                Storage::default(),
                crate::app::Settings::default(),
                false,
            ),
            cleanup,
        )
    }

    fn agent_in_repo(title: &str, repo_root: PathBuf) -> Agent {
        let mut agent = Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("branch/{title}"),
            repo_root.join("worktree"),
        );
        agent.repo_root = Some(repo_root);
        agent
    }

    #[test]
    fn test_state_file_stamp_returns_none_for_missing_file()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let dir = TempDir::new()?;
        let missing = dir.path().join("missing.json");
        assert!(state_file_stamp(&missing).is_none());
        Ok(())
    }

    #[test]
    fn test_state_file_tracker_reload_restores_sidebar_selection()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let dir = TempDir::new()?;
        let state_path = dir.path().join("state.json");
        let repo_a = dir.path().join("repo-a");
        let repo_b = dir.path().join("repo-b");

        std::fs::create_dir_all(&repo_a)?;
        std::fs::create_dir_all(&repo_b)?;

        let mut storage = Storage::with_path(state_path.clone());
        let agent_a = agent_in_repo("agent-a", repo_a.clone());
        let agent_b = agent_in_repo("agent-b", repo_b.clone());
        let selected_agent_id = agent_b.id;
        storage.add(agent_a);
        storage.add(agent_b);
        storage.save()?;

        let config = create_test_config();
        let mut app = App::new(config, storage, crate::app::Settings::default(), false);

        // Select agent-b.
        let agent_b_index = app
            .data
            .sidebar_items()
            .iter()
            .position(|item| match item {
                crate::app::SidebarItem::Agent(agent) => agent.info.agent.id == selected_agent_id,
                crate::app::SidebarItem::Project(_) => false,
            })
            .ok_or("Expected agent-b to be present in the sidebar")?;
        app.data.selected = agent_b_index;
        assert_eq!(
            selected_sidebar_key(&app),
            Some(SelectedSidebarKey::Agent(selected_agent_id))
        );

        let mut tracker = StateFileTracker::new(&app);

        // Another client writes a new agent to disk.
        let mut disk = Storage::load_from(&state_path)?;
        let agent_c = agent_in_repo("agent-c", repo_a);
        let added_agent_id = agent_c.id;
        disk.add(agent_c);
        disk.save_to(&state_path)?;

        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap_or_else(Instant::now);
        assert!(tracker.maybe_reload_state(&mut app));
        assert!(app.data.storage.get(added_agent_id).is_some());
        assert_eq!(
            selected_sidebar_key(&app),
            Some(SelectedSidebarKey::Agent(selected_agent_id))
        );
        assert_eq!(app.data.storage.state_path, Some(state_path.clone()));

        // Select repo-b project header and reload again.
        let repo_b_header_index = app
            .data
            .sidebar_items()
            .iter()
            .position(|item| match item {
                crate::app::SidebarItem::Project(project) => project.root == repo_b,
                crate::app::SidebarItem::Agent(_) => false,
            })
            .ok_or("Expected repo-b project header to be present in the sidebar")?;
        app.data.selected = repo_b_header_index;
        let selected_key = selected_sidebar_key(&app).ok_or("Expected sidebar selection")?;
        match selected_key {
            SelectedSidebarKey::Project(root) => assert_eq!(root, repo_b),
            SelectedSidebarKey::Agent(_) => return Err("Expected project selection".into()),
        }

        let mut disk = Storage::load_from(&state_path)?;
        let agent_d = agent_in_repo("agent-d", repo_b.clone());
        disk.add(agent_d);
        disk.save_to(&state_path)?;

        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap_or_else(Instant::now);
        assert!(tracker.maybe_reload_state(&mut app));
        let selected_key = selected_sidebar_key(&app).ok_or("Expected sidebar selection")?;
        match selected_key {
            SelectedSidebarKey::Project(root) => assert_eq!(root, repo_b),
            SelectedSidebarKey::Agent(_) => return Err("Expected project selection".into()),
        }

        Ok(())
    }

    #[test]
    fn test_state_file_tracker_does_not_reload_when_interval_not_elapsed()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let dir = TempDir::new()?;
        let state_path = dir.path().join("state.json");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo)?;

        let mut storage = Storage::with_path(state_path);
        storage.add(agent_in_repo("agent", repo));
        storage.save()?;

        let config = create_test_config();
        let mut app = App::new(config, storage, crate::app::Settings::default(), false);

        let mut tracker = StateFileTracker::new(&app);
        tracker.last_check = Instant::now();
        assert!(!tracker.maybe_reload_state(&mut app));

        Ok(())
    }

    #[test]
    fn test_testcleanup_removes_matching_worktree_and_branch()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        fn git(repo: &std::path::Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_OBJECT_DIRECTORY")
                .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
                .status()?;
            if !status.success() {
                return Err(format!("git {args:?} failed with {status}").into());
            }
            Ok(())
        }

        let repo_dir = TempDir::new()?;
        git(repo_dir.path(), &["init", "-q"])?;
        git(
            repo_dir.path(),
            &["config", "user.email", "test@example.com"],
        )?;
        git(repo_dir.path(), &["config", "user.name", "Test"])?;
        std::fs::write(repo_dir.path().join("README.md"), "test")?;
        git(repo_dir.path(), &["add", "."])?;
        git(
            repo_dir.path(),
            &["commit", "-q", "--no-verify", "-m", "init"],
        )?;

        let worktree_parent = TempDir::new()?;
        let branch_prefix = "tenex-test-cleanup/";
        let branch_name = format!("{branch_prefix}branch");
        let worktree_path = worktree_parent
            .path()
            .join(branch_prefix.replace('/', "-") + "branch");
        git(
            repo_dir.path(),
            &[
                "worktree",
                "add",
                "-b",
                &branch_name,
                worktree_path.to_str().ok_or("worktree path is not utf-8")?,
            ],
        )?;

        // Add a second worktree that should NOT match our prefix, so cleanup exercises the
        // non-matching path deterministically (CI repos typically have no extra worktrees).
        let other_branch = "other-branch";
        let other_worktree_path = worktree_parent.path().join("other-worktree");
        git(
            repo_dir.path(),
            &[
                "worktree",
                "add",
                "-b",
                other_branch,
                other_worktree_path
                    .to_str()
                    .ok_or("other worktree path is not utf-8")?,
            ],
        )?;

        let original_dir = std::env::current_dir()?;
        std::env::set_current_dir(repo_dir.path())?;
        let cleanup = TestCleanup::new(branch_prefix);
        std::env::set_current_dir(original_dir)?;

        drop(cleanup);

        assert!(
            !worktree_path.exists(),
            "Expected cleanup to remove worktree at {}",
            worktree_path.display()
        );

        let repo = git2::Repository::open(repo_dir.path())?;
        assert!(
            repo.find_branch(&branch_name, git2::BranchType::Local)
                .is_err(),
            "Expected cleanup to delete branch {branch_name}"
        );

        Ok(())
    }

    /// Test helper that wraps `input::handle_key_event` with an empty `batched_keys` vec
    fn test_key_event(
        app: &mut App,
        _handler: Actions,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        let mut keys = Vec::new();
        input::handle_key_event(app, code, modifiers, &mut keys)
    }

    #[test]
    fn test_compute_preview_refresh_interval() {
        assert_eq!(
            compute_preview_refresh_interval(100, Tab::Preview, true),
            Duration::from_millis(PREVIEW_SMOOTH_REFRESH_MS)
        );
        assert_eq!(
            compute_preview_refresh_interval(10, Tab::Preview, true),
            Duration::from_millis(MIN_OUTPUT_REFRESH_MS)
        );
        assert_eq!(
            compute_preview_refresh_interval(100, Tab::Diff, true),
            Duration::from_millis(100)
        );
        assert_eq!(
            compute_preview_refresh_interval(100, Tab::Preview, false),
            Duration::from_millis(100)
        );
    }

    #[test]
    fn test_send_batched_keys_to_mux_with_selected_agent() {
        let mut app = create_test_app();
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        ));
        app.data.selected = 0;

        send_batched_keys_to_mux(&app, &[String::from("a")]);
    }

    #[test]
    fn test_handle_key_event_normal_mode_quit() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Ctrl+q should trigger quit (since no running agents)
        test_key_event(&mut app, handler, KeyCode::Char('q'), KeyModifiers::CONTROL)?;
        assert!(app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // '?' should open help
        test_key_event(&mut app, handler, KeyCode::Char('?'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::Help(HelpMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_any_key_exits() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(HelpMode.into());
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'a' should enter creating mode
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent_with_prompt()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'A' should enter prompting mode
        test_key_event(&mut app, handler, KeyCode::Char('A'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_char_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('b'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('c'), KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "abc");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        app.handle_char('a');
        app.handle_char('b');
        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_escape_cancels() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.input.buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_empty_does_nothing()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        // Enter with empty input should just exit mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        // No agent created since input was empty
        assert_eq!(app.data.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_yes() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter confirming quit mode
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        // 'y' should confirm and quit
        test_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE)?;
        assert!(app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_capital_y() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Char('Y'), KeyModifiers::NONE)?;
        assert!(app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;

        // Should still be in confirming mode
        assert!(matches!(&app.mode, AppMode::Confirming(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Navigation keys should work in normal mode
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;

        // Should still be in normal mode (no state change visible without agents)
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_tab_switch() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        let initial_tab = app.data.active_tab;
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;
        assert_ne!(app.data.active_tab, initial_tab);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Scroll commands
        test_key_event(&mut app, handler, KeyCode::Char('u'), KeyModifiers::CONTROL)?;
        test_key_event(&mut app, handler, KeyCode::Char('d'), KeyModifiers::CONTROL)?;
        test_key_event(&mut app, handler, KeyCode::Char('g'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('G'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_unknown_key_does_nothing() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Unknown key should be ignored
        test_key_event(&mut app, handler, KeyCode::F(12), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(PromptingMode.into());
        app.handle_char('t');
        app.handle_char('e');

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_cancel_action() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Escape in normal mode triggers cancel action (does nothing but works)
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter scrolling mode (happens when scroll keys are pressed)
        app.enter_mode(ScrollingMode.into());

        // Should handle navigation keys in scrolling mode
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_other_keys() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());

        // Other keys like arrows should be ignored in creating mode
        test_key_event(&mut app, handler, KeyCode::Left, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Right, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;

        // Should still be in creating mode
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(PromptingMode.into());

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('h'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('i'), KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "hi");
        assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_kill() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter confirming kill mode (no agents to kill, but mode should change)
        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into(),
        );

        // 'y' should trigger confirm but no agent to kill
        test_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE)?;

        // Should exit to normal mode
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_reset() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Reset,
            }
            .into(),
        );

        // 'n' should cancel
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_capital_n() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        // 'N' should also cancel
        test_key_event(&mut app, handler, KeyCode::Char('N'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_with_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _cleanup) = create_test_app_with_cleanup();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        // Enter with input tries to create agent (will fail without git repo, but sets error)
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Possible outcomes:
        // 1. Error modal (no git repo)
        // 2. Normal mode (agent created successfully)
        // 3. Confirming(WorktreeConflict) if worktree already exists
        let is_error_modal = matches!(&app.mode, AppMode::ErrorModal(_));
        let is_normal = app.mode == AppMode::normal();
        let is_worktree_conflict = matches!(
            &app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            })
        );
        assert!(
            is_error_modal || is_normal || is_worktree_conflict,
            "Expected ErrorModal, Normal, or Confirming(WorktreeConflict) mode, got {:?}",
            app.mode
        );
        // One of these should be true:
        // - Error was set (no git repo or other failure)
        // - Agent was created
        // - Worktree conflict detected (waiting for user input)
        let has_error = app.data.ui.last_error.is_some();
        let has_agent = app.data.storage.len() == 1;
        let has_worktree_conflict = app.data.spawn.worktree_conflict.is_some();
        assert!(
            has_error || has_agent || has_worktree_conflict,
            "Expected error, created agent, or worktree conflict. mode={:?} error={:?} agents={} conflict={:?}",
            app.mode,
            app.data.ui.last_error,
            app.data.storage.len(),
            app.data.spawn.worktree_conflict
        );
        // _cleanup will automatically remove test branches/worktrees when dropped
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_enter_with_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _cleanup) = create_test_app_with_cleanup();
        let handler = Actions::new();

        app.enter_mode(PromptingMode.into());
        app.handle_char('f');
        app.handle_char('i');
        app.handle_char('x');

        // Enter with input tries to create agent with prompt (will fail without git repo)
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Possible outcomes (same as creating mode test):
        // 1. Error modal (no git repo)
        // 2. Normal mode (agent created successfully)
        // 3. Confirming(WorktreeConflict) if worktree already exists
        let is_error_modal = matches!(&app.mode, AppMode::ErrorModal(_));
        let is_normal = app.mode == AppMode::normal();
        let is_worktree_conflict = matches!(
            &app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            })
        );
        assert!(
            is_error_modal || is_normal || is_worktree_conflict,
            "Expected ErrorModal, Normal, or Confirming(WorktreeConflict) mode, got {:?}",
            app.mode
        );
        // One of these should be true:
        // - Error was set (no git repo or other failure)
        // - Agent was created
        // - Worktree conflict detected (waiting for user input)
        let has_error = app.data.ui.last_error.is_some();
        let has_agent = app.data.storage.len() == 1;
        let has_worktree_conflict = app.data.spawn.worktree_conflict.is_some();
        assert!(
            has_error || has_agent || has_worktree_conflict,
            "Expected error, created agent, or worktree conflict. mode={:?} error={:?} agents={} conflict={:?}",
            app.mode,
            app.data.ui.last_error,
            app.data.storage.len(),
            app.data.spawn.worktree_conflict
        );
        // _cleanup will automatically remove test branches/worktrees when dropped
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_fallthrough() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());

        // Tab key should fall through to action handling in creating mode
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;

        // Mode should remain creating (Tab doesn't exit creating mode)
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_scrolling_mode_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ScrollingMode.into());

        // Test scrolling mode handles normal mode keybindings
        test_key_event(&mut app, handler, KeyCode::Char('g'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('G'), KeyModifiers::NONE)?;

        // Should handle without panic
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('h'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('l'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('l'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('o'), KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "hello");
        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());
        app.handle_char('t');
        app.handle_char('e');

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_backspace() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());
        app.handle_char('a');
        app.handle_char('b');

        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_enter_no_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        // Enter with no agent selected should show error modal
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
        assert!(app.data.ui.last_error.is_some());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_enter_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());

        // Enter with empty input should just exit mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_error_modal_dismiss() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Set an error (this enters ErrorModal mode)
        app.set_error("Test error message");
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));

        // Any key should dismiss the error modal
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.ui.last_error.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_error_modal_dismiss_with_esc() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.set_error("Test error");
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_enter() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());

        // Enter should proceed to child prompt
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::ChildPrompt(ChildPromptMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());

        // Escape should exit mode
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_up_down() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());
        let initial_count = app.data.spawn.child_count;

        // Up should increment
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.data.spawn.child_count, initial_count + 1);

        // Down should decrement
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.data.spawn.child_count, initial_count);

        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildPromptMode.into());

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('t'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('s'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('t'), KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "test");
        assert_eq!(app.mode, AppMode::ChildPrompt(ChildPromptMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildPromptMode.into());
        app.handle_char('t');

        // Escape should exit mode
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.input.buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_enter_no_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _cleanup) = create_test_app_with_cleanup();
        let handler = Actions::new();

        app.enter_mode(ChildPromptMode.into());
        app.handle_char('t');
        app.handle_char('a');
        app.handle_char('s');
        app.handle_char('k');

        // Enter with input tries to spawn children (will fail without agent selected)
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // On error, should show error modal; on success with no agent, exits normally
        // Could also enter WorktreeConflict mode if the branch already exists
        let is_error_modal = matches!(&app.mode, AppMode::ErrorModal(_));
        let is_normal = app.mode == AppMode::normal();
        let is_worktree_conflict = matches!(
            &app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::WorktreeConflict,
            })
        );
        assert!(
            is_error_modal || is_normal || is_worktree_conflict,
            "Expected ErrorModal, Normal, or WorktreeConflict mode, got {:?}",
            app.mode
        );
        // _cleanup will automatically remove test branches/worktrees when dropped
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_other_keys() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());
        let initial_count = app.data.spawn.child_count;

        // Other keys should be ignored
        test_key_event(&mut app, handler, KeyCode::Left, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Right, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;

        // Should still be in ChildCount mode with same count
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        assert_eq!(app.data.spawn.child_count, initial_count);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_info_mode_any_key_exits()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewInfoMode.into());

        // Any key should dismiss
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_info_mode_esc_exits() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewInfoMode.into());

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_up_down()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewChildCountMode.into());
        let initial_count = app.data.spawn.child_count;

        // Up should increment
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.data.spawn.child_count, initial_count + 1);

        // Down should decrement
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.data.spawn.child_count, initial_count);

        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_enter()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewChildCountMode.into());

        // Enter should proceed to branch selector
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::BranchSelector(BranchSelectorMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_escape()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewChildCountMode.into());

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    fn create_test_branch_info(name: &str, is_remote: bool) -> crate::git::BranchInfo {
        crate::git::BranchInfo {
            name: name.to_string(),
            full_name: if is_remote {
                format!("refs/remotes/origin/{name}")
            } else {
                format!("refs/heads/{name}")
            },
            is_remote,
            remote: if is_remote {
                Some("origin".to_string())
            } else {
                None
            },
            last_commit_time: None,
        }
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_navigation()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("develop", false),
        ];
        app.enter_mode(BranchSelectorMode.into());

        assert_eq!(app.data.review.selected, 0);

        // Down should move to next
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.data.review.selected, 1);

        // Down should move to next
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.data.review.selected, 2);

        // Up should move to previous
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.data.review.selected, 1);

        // Up should move to previous
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.data.review.selected, 0);

        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_filter() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
        ];
        app.enter_mode(BranchSelectorMode.into());

        // Type characters for filter
        test_key_event(&mut app, handler, KeyCode::Char('m'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;

        assert_eq!(app.data.review.filter, "ma");
        assert_eq!(app.mode, AppMode::BranchSelector(BranchSelectorMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_backspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![create_test_branch_info("main", false)];
        app.data.review.filter = "main".to_string();
        app.enter_mode(BranchSelectorMode.into());

        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;
        assert_eq!(app.data.review.filter, "mai");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_escape() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BranchSelectorMode.into());
        app.data.review.branches = vec![create_test_branch_info("main", false)];
        app.data.review.filter = "test".to_string();

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        // State should be cleared on escape
        assert!(app.data.review.branches.is_empty());
        assert!(app.data.review.filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_enter() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("develop", false),
        ];
        app.data.review.selected = 1;
        app.data.spawn.spawning_under = Some(uuid::Uuid::new_v4());
        app.enter_mode(BranchSelectorMode.into());

        // Enter tries to spawn review agents (will fail without proper agent setup)
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Should have set review_base_branch before attempting spawn
        assert!(
            app.data.review.base_branch.is_some() || matches!(&app.mode, AppMode::ErrorModal(_)),
            "Expected review_base_branch to be set or error modal, got {:?}",
            app.mode
        );
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_enter_empty() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![]; // Empty list
        app.enter_mode(BranchSelectorMode.into());

        // Enter with empty list exits mode but doesn't set base branch
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.review.base_branch.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_swarm_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Press 'R' with no agent selected
        test_key_event(&mut app, handler, KeyCode::Char('R'), KeyModifiers::NONE)?;

        // Should show ReviewInfo mode
        assert_eq!(app.mode, AppMode::ReviewInfo(ReviewInfoMode));
        Ok(())
    }

    // === Git Operations Key Event Tests ===

    #[test]
    fn test_handle_key_event_confirm_push_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // 'n' should cancel and exit
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // Escape should cancel and exit
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_yes() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // 'y' should try to execute push (will fail, no agent in storage)
        test_key_event(&mut app, handler, KeyCode::Char('Y'), KeyModifiers::NONE)?;

        // Should show error (no agent in storage)
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.git_op.branch_name = "feature/old".to_string();
        app.data.input.buffer = "feature/old".to_string();
        app.data.input.cursor = app.data.input.buffer.len(); // Cursor at end

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('-'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Char('w'), KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "feature/old-new");
        assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.input.buffer = "feature/test".to_string();
        app.data.input.cursor = app.data.input.buffer.len(); // Cursor at end

        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;

        assert_eq!(app.data.input.buffer, "feature/tes");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.input.buffer = "feature/test".to_string();

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none()); // State cleared
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_enter() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.original_branch = "feature/old".to_string();
        app.data.git_op.branch_name = "feature/old".to_string();
        app.data.input.buffer = "feature/new".to_string();

        // Enter tries to confirm rename and execute (will fail without agent)
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Branch name should have been updated before failing
        assert_eq!(app.data.git_op.branch_name, "feature/new");
        // Should show error (no agent in storage)
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_no() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushForPRMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // 'n' should cancel and exit
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none()); // State cleared
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_escape() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushForPRMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_yes() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushForPRMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();
        app.data.git_op.base_branch = "main".to_string();

        // 'y' should try to push and open PR (will fail, no agent in storage)
        test_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE)?;

        // Should show error (no agent in storage)
        assert!(matches!(&app.mode, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_other_keys_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());

        // Other keys should be ignored
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Should still be in ConfirmPush mode
        assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
        Ok(())
    }

    // === keycode_to_input_sequence Tests ===

    #[test]
    fn test_keycode_to_input_sequence_char() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Char('a'), KeyModifiers::NONE),
            Some("a".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Char('Z'), KeyModifiers::NONE),
            Some("Z".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_ctrl_char() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some("\u{3}".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Char('x'), KeyModifiers::CONTROL),
            Some("\u{18}".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_alt_char() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Char('a'), KeyModifiers::ALT),
            Some("\u{1b}a".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_special_keys() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Enter, KeyModifiers::NONE),
            Some("\r".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Enter, KeyModifiers::ALT),
            Some("\u{1b}\r".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Esc, KeyModifiers::NONE),
            Some("\u{1b}".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Backspace, KeyModifiers::NONE),
            Some("\u{7f}".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Tab, KeyModifiers::NONE),
            Some("\t".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_arrows() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Up, KeyModifiers::NONE),
            Some("\u{1b}[A".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Down, KeyModifiers::NONE),
            Some("\u{1b}[B".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Left, KeyModifiers::NONE),
            Some("\u{1b}[D".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Right, KeyModifiers::NONE),
            Some("\u{1b}[C".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_navigation() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Home, KeyModifiers::NONE),
            Some("\u{1b}[H".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::End, KeyModifiers::NONE),
            Some("\u{1b}[F".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::PageUp, KeyModifiers::NONE),
            Some("\u{1b}[5~".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::PageDown, KeyModifiers::NONE),
            Some("\u{1b}[6~".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Delete, KeyModifiers::NONE),
            Some("\u{1b}[3~".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Insert, KeyModifiers::NONE),
            Some("\u{1b}[2~".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_function_keys() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::F(1), KeyModifiers::NONE),
            Some("\u{1b}OP".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::F(12), KeyModifiers::NONE),
            Some("\u{1b}[24~".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_ctrl_special() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Up, KeyModifiers::CONTROL),
            Some("\u{1b}[1;5A".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Home, KeyModifiers::CONTROL),
            Some("\u{1b}[1;5H".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_alt_special() {
        assert_eq!(
            keycode_to_input_sequence(KeyCode::Down, KeyModifiers::ALT),
            Some("\u{1b}[1;3B".to_string())
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::End, KeyModifiers::ALT),
            Some("\u{1b}[1;3F".to_string())
        );
    }

    #[test]
    fn test_keycode_to_input_sequence_unsupported() {
        // CapsLock and other unsupported keys return None
        assert_eq!(
            keycode_to_input_sequence(KeyCode::CapsLock, KeyModifiers::NONE),
            None
        );
        assert_eq!(
            keycode_to_input_sequence(KeyCode::NumLock, KeyModifiers::NONE),
            None
        );
    }

    // === PreviewFocused Mode Tests ===

    #[test]
    fn test_handle_key_event_preview_focused_ctrl_q_exits() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(PreviewFocusedMode.into());
        assert_eq!(app.mode, AppMode::PreviewFocused(PreviewFocusedMode));

        // Ctrl+q should exit preview focus mode
        test_key_event(&mut app, handler, KeyCode::Char('q'), KeyModifiers::CONTROL)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_preview_focused_collects_keys()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();

        app.enter_mode(PreviewFocusedMode.into());

        // Regular keys should be collected for batching (not change mode)
        let mut keys = Vec::new();
        input::handle_key_event(&mut app, KeyCode::Char('a'), KeyModifiers::NONE, &mut keys)?;
        assert_eq!(app.mode, AppMode::PreviewFocused(PreviewFocusedMode));
        assert_eq!(keys, vec!["a".to_string()]);

        // Special keys also collected
        input::handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut keys)?;
        assert_eq!(keys, vec!["a".to_string(), "\r".to_string()]);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_preview_focused_ctrl_c_prompts_for_non_terminal_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        app.data.storage.add(Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        ));
        app.data.selected = 1;
        app.enter_mode(PreviewFocusedMode.into());

        let mut keys = Vec::new();
        input::handle_key_event(
            &mut app,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            &mut keys,
        )?;

        assert!(keys.is_empty());
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::InterruptAgent
            })
        );
        Ok(())
    }

    #[test]
    fn test_handle_key_event_focus_preview_action() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Without agent selected, FocusPreview should not change mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }
}
