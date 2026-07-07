//! Terminal User Interface for Tenex

mod input;
mod render;

use crate::app::{Actions, App, Event, Handler, Tab};
use crate::state::AppMode;
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::Backend,
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

type DrainedEvents = (Vec<String>, Option<(u16, u16)>, bool);

struct DynWrite<'a> {
    inner: &'a mut dyn io::Write,
}

impl io::Write for DynWrite<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

trait EventReader {
    fn next_event(&self) -> Result<Event>;
    fn poll_immediate(&self) -> Result<bool>;
}

fn poll_for_tui(timeout: Duration) -> io::Result<bool> {
    poll_for_tui_with_failpoint(timeout, tui_run_failpoint())
}

fn poll_for_tui_with_failpoint(
    timeout: Duration,
    failpoint: Option<TuiRunFailpoint>,
) -> io::Result<bool> {
    if failpoint == Some(TuiRunFailpoint::PollImmediate) {
        return Err(io::Error::other("Forced poll_immediate error for test"));
    }

    crossterm_event::poll(timeout)
}

impl EventReader for Handler {
    fn next_event(&self) -> Result<Event> {
        self.next()
    }

    fn poll_immediate(&self) -> Result<bool> {
        Ok(poll_for_tui(Duration::ZERO)?)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiRunFailpoint {
    EnableRawMode,
    EnterTuiScreen,
    CreateTerminal,
    DisableRawMode,
    LeaveTuiScreen,
    ShowCursor,
    PollImmediate,
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn tui_run_failpoint() -> Option<TuiRunFailpoint> {
    #[cfg(not(debug_assertions))]
    {
        None
    }

    #[cfg(debug_assertions)]
    {
        let value = std::env::var("TENEX_TEST_TUI_FAILPOINT").ok()?;
        parse_tui_run_failpoint(&value)
    }
}

#[cfg(debug_assertions)]
fn parse_tui_run_failpoint(value: &str) -> Option<TuiRunFailpoint> {
    match value.trim() {
        "enable_raw_mode" => Some(TuiRunFailpoint::EnableRawMode),
        "enter_tui_screen" => Some(TuiRunFailpoint::EnterTuiScreen),
        "create_terminal" => Some(TuiRunFailpoint::CreateTerminal),
        "disable_raw_mode" => Some(TuiRunFailpoint::DisableRawMode),
        "leave_tui_screen" => Some(TuiRunFailpoint::LeaveTuiScreen),
        "show_cursor" => Some(TuiRunFailpoint::ShowCursor),
        "poll_immediate" => Some(TuiRunFailpoint::PollImmediate),
        _ => None,
    }
}

fn enable_raw_mode_for_tui() -> io::Result<()> {
    if tui_run_failpoint() == Some(TuiRunFailpoint::EnableRawMode) {
        return Err(io::Error::other("Forced enable_raw_mode error for test"));
    }

    enable_raw_mode()
}

fn disable_raw_mode_for_tui() -> io::Result<()> {
    if tui_run_failpoint() == Some(TuiRunFailpoint::DisableRawMode) {
        return Err(io::Error::other("Forced disable_raw_mode error for test"));
    }

    disable_raw_mode()
}

fn enter_tui_screen_for_tui(stdout: &mut dyn io::Write, enable_mouse_capture: bool) -> Result<()> {
    if tui_run_failpoint() == Some(TuiRunFailpoint::EnterTuiScreen) {
        anyhow::bail!("Forced enter_tui_screen error for test");
    }

    enter_tui_screen(stdout, enable_mouse_capture)
}

fn create_terminal_for_tui(
    stdout: io::Stdout,
) -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    if tui_run_failpoint() == Some(TuiRunFailpoint::CreateTerminal) {
        return Err(io::Error::other("Forced terminal creation error for test"));
    }

    Terminal::new(CrosstermBackend::new(stdout))
}

fn leave_tui_screen_for_tui(stdout: &mut dyn io::Write) -> io::Result<()> {
    if tui_run_failpoint() == Some(TuiRunFailpoint::LeaveTuiScreen) {
        return Err(io::Error::other("Forced leave_tui_screen error for test"));
    }

    let mut stdout = DynWrite { inner: stdout };
    execute!(&mut stdout, LeaveAlternateScreen, DisableMouseCapture)
}

fn show_cursor_for_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    if tui_run_failpoint() == Some(TuiRunFailpoint::ShowCursor) {
        return Err(io::Error::other("Forced show_cursor error for test"));
    }

    terminal.show_cursor()
}

fn enter_tui_screen(stdout: &mut dyn io::Write, enable_mouse_capture: bool) -> Result<()> {
    let mut stdout = DynWrite { inner: stdout };
    execute!(&mut stdout, EnterAlternateScreen)?;
    if enable_mouse_capture {
        execute!(&mut stdout, EnableMouseCapture)?;
    }
    Ok(())
}

fn flush_pending_clipboard(stdout: &mut dyn io::Write, app: &mut App) {
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

fn write_osc52_clipboard(stdout: &mut dyn io::Write, content: &str) -> io::Result<()> {
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

fn state_file_stamp_from_parts(
    modified: io::Result<SystemTime>,
    len: u64,
) -> Option<StateFileStamp> {
    let modified = modified.ok()?;
    Some(StateFileStamp { modified, len })
}

fn state_file_stamp(path: &Path) -> Option<StateFileStamp> {
    let metadata = fs::metadata(path).ok()?;
    state_file_stamp_from_parts(metadata.modified(), metadata.len())
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
    enable_raw_mode_for_tui()?;
    let mut stdout = io::stdout();
    enter_tui_screen_for_tui(&mut stdout, mouse_capture_enabled())?;

    let keyboard_enhancement_enabled = configure_keyboard_enhancement(&mut stdout);

    // Update the app with keyboard enhancement status
    app.data.keyboard_enhancement_supported = keyboard_enhancement_enabled;

    apply_startup_modals(&mut app);

    let mut terminal = create_terminal_for_tui(stdout)?;

    let event_handler = Handler::new(UI_FRAME_INTERVAL_MS);
    let action_handler = Actions::new();

    let mut clipboard_out = io::stdout();
    let result = run_loop(
        &mut terminal,
        &mut app,
        &event_handler,
        action_handler,
        &mut clipboard_out,
    );

    pop_keyboard_enhancement(terminal.backend_mut(), keyboard_enhancement_enabled);

    disable_raw_mode_for_tui()?;
    leave_tui_screen_for_tui(terminal.backend_mut())?;
    show_cursor_for_tui(&mut terminal)?;

    result
}

fn apply_startup_modals(app: &mut App) {
    if matches!(app.mode, AppMode::Normal(_)) && app.should_show_keyboard_remap_prompt() {
        app.show_keyboard_remap_prompt();
    }

    // If no higher-priority modal is open, show any deferred "What's New" changelog modal.
    if matches!(app.mode, AppMode::Normal(_))
        && let Some(pending) = app.data.pending_changelog.take()
    {
        app.apply_mode(pending.into());
    }
}

fn configure_keyboard_enhancement(stdout: &mut dyn io::Write) -> bool {
    // Enable Kitty keyboard protocol to disambiguate Ctrl+M from Enter
    // This is supported by modern terminals: kitty, foot, WezTerm, alacritty (0.13+)
    enable_keyboard_enhancement_with_support(
        stdout,
        supports_keyboard_enhancement().unwrap_or(false),
    )
}

fn enable_keyboard_enhancement_with_support(stdout: &mut dyn io::Write, supported: bool) -> bool {
    if supported {
        info!("Terminal supports keyboard enhancement protocol - Ctrl+M will work");
        let mut stdout = DynWrite { inner: stdout };
        execute!(
            &mut stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )
        .is_ok()
    } else {
        warn!("Terminal does not support keyboard enhancement protocol - Ctrl+M will act as Enter");
        false
    }
}

fn pop_keyboard_enhancement(stdout: &mut dyn io::Write, enabled: bool) {
    if !enabled {
        return;
    }

    let mut stdout = DynWrite { inner: stdout };
    if let Err(e) = execute!(&mut stdout, PopKeyboardEnhancementFlags) {
        warn!("Failed to pop keyboard enhancement flags: {e}");
    }
}

fn send_batched_keys_to_mux(app: &mut App, batched_keys: &[String]) {
    if batched_keys.is_empty() {
        return;
    }

    if let Some(agent) = app.selected_agent() {
        let target = agent.window_index.map_or_else(
            || agent.mux_session.clone(),
            |idx| format!("{}:{}", agent.mux_session, idx),
        );
        // Use synchronous call so the mux processes keys before we capture.
        if let Err(err) = crate::mux::SessionManager::new().send_keys_batch(&target, batched_keys) {
            app.set_status(format!("Input not sent: {err}"));
        }
    }
}

fn send_keys_and_flush_clipboard(
    stdout: &mut dyn io::Write,
    app: &mut App,
    batched_keys: &[String],
) {
    send_batched_keys_to_mux(app, batched_keys);
    flush_pending_clipboard(stdout, app);
}

fn init_preview_dimensions(terminal: &dyn TerminalInfo, app: &mut App, action_handler: Actions) {
    if app.data.ui.preview_dimensions.is_some() && app.data.ui.terminal_dimensions.is_some() {
        return;
    }

    let Ok(size) = terminal.size() else {
        return;
    };

    app.set_terminal_dimensions(size.width, size.height);

    let area = Rect::new(0, 0, size.width, size.height);
    let (width, height) = render::calculate_preview_dimensions(area);
    if apply_preview_dimensions(app, action_handler, width, height) {
        app.ensure_agent_list_scroll();
    }
}

fn apply_preview_dimensions(
    app: &mut App,
    action_handler: Actions,
    width: u16,
    height: u16,
) -> bool {
    if width == 0 || height == 0 {
        warn!(width, height, "Skipping zero-sized preview dimensions");
        app.set_status(format!(
            "Preview is too small to resize agents: {width}x{height}"
        ));
        return false;
    }

    if action_handler.resize_agent_windows_to_dimensions(app, width, height) {
        app.set_preview_dimensions(width, height);
        return true;
    }

    false
}

fn drain_events(
    terminal: &dyn TerminalInfo,
    app: &mut App,
    event_handler: &dyn EventReader,
) -> Result<DrainedEvents> {
    let mut last_resize: Option<(u16, u16)> = None;
    let mut batched_keys: Vec<String> = Vec::new();
    let mut flushed_batched_keys = false;

    let size = terminal
        .size()
        .unwrap_or_else(|_| ratatui::layout::Size::new(0, 0));
    let mut frame_area = Rect::new(0, 0, size.width, size.height);

    loop {
        match event_handler.next_event()? {
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

                input::handle_mouse_event(app, mouse, frame_area, &mut batched_keys);
            }
            Event::Resize(w, h) => {
                last_resize = Some((w, h));
                frame_area = Rect::new(0, 0, w, h);
            }
        }

        if !event_handler.poll_immediate()? {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffRefreshTarget {
    Diff,
    Digest,
}

fn diff_refresh_target(
    active_tab: Tab,
    needs_content_update: bool,
    diff_due: bool,
    diff_force_refresh: bool,
) -> Option<DiffRefreshTarget> {
    if active_tab == Tab::Diff || diff_force_refresh {
        return (needs_content_update || diff_due || diff_force_refresh)
            .then_some(DiffRefreshTarget::Diff);
    }

    diff_due.then_some(DiffRefreshTarget::Digest)
}

fn should_refresh_commits(active_tab: Tab, needs_content_update: bool, commits_due: bool) -> bool {
    if active_tab == Tab::Commits {
        return needs_content_update || commits_due;
    }

    commits_due
}

fn maybe_finish_preparing_docker(app: &mut App) -> bool {
    if !matches!(&app.mode, AppMode::PreparingDocker(_)) {
        return false;
    }

    let next = app.data.finish_preparing_docker_for_new_roots();
    app.apply_mode(next);
    true
}

fn apply_pending_resize(app: &mut App, action_handler: Actions, last_resize: Option<(u16, u16)>) {
    let Some((width, height)) = last_resize else {
        return;
    };

    app.set_terminal_dimensions(width, height);
    let (preview_width, preview_height) =
        render::calculate_preview_dimensions(Rect::new(0, 0, width, height));
    if app.data.ui.preview_dimensions != Some((preview_width, preview_height))
        && apply_preview_dimensions(app, action_handler, preview_width, preview_height)
    {
        app.ensure_agent_list_scroll();
    }
}

#[inline(never)]
const fn compute_sent_keys_in_preview(
    flushed_batched_keys: bool,
    batched_keys: &[String],
    mode: &AppMode,
) -> bool {
    flushed_batched_keys
        || (!batched_keys.is_empty() && matches!(mode, &AppMode::PreviewFocused(_)))
}

#[expect(
    clippy::too_many_lines,
    reason = "main loop keeps terminal redraw and periodic refresh timing together"
)]
fn run_loop(
    terminal: &mut dyn TerminalOps,
    app: &mut App,
    event_handler: &dyn EventReader,
    action_handler: Actions,
    clipboard_out: &mut dyn io::Write,
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
        let sent_keys_in_preview =
            compute_sent_keys_in_preview(flushed_batched_keys, &batched_keys, &app.mode);
        send_keys_and_flush_clipboard(clipboard_out, app, &batched_keys);

        apply_pending_resize(app, action_handler, last_resize);

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
        if let Some(target) = diff_refresh_target(
            app.data.active_tab,
            needs_content_update,
            diff_due,
            app.data.ui.diff_force_refresh,
        ) {
            match target {
                DiffRefreshTarget::Diff => {
                    let _ = action_handler.update_diff(app);
                }
                DiffRefreshTarget::Digest => {
                    let _ = action_handler.update_diff_digest(app);
                }
            }
            last_diff_update = Instant::now();
        }

        let commits_due = last_commits_update.elapsed() >= commits_refresh_interval;
        if should_refresh_commits(app.data.active_tab, needs_content_update, commits_due) {
            if app.data.active_tab == Tab::Commits {
                let _ = action_handler.update_commits(app);
            } else {
                let _ = action_handler.update_commits_digest(app);
            }
            last_commits_update = Instant::now();
        }

        needs_content_update = false;

        // Draw ONCE after draining all queued events
        terminal.draw(app)?;

        if maybe_finish_preparing_docker(app) {
            continue;
        }

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

trait TerminalInfo {
    fn size(&self) -> Result<ratatui::layout::Size>;
}

trait TerminalOps: TerminalInfo {
    fn draw(&mut self, app: &App) -> Result<()>;
}

impl<B: Backend> TerminalInfo for Terminal<B> {
    fn size(&self) -> Result<ratatui::layout::Size> {
        Self::size(self).map_err(Into::into)
    }
}

impl<B: Backend> TerminalOps for Terminal<B> {
    fn draw(&mut self, app: &App) -> Result<()> {
        Self::draw(self, |frame| render::render(frame, app))
            .map(|_| ())
            .map_err(Into::into)
    }
}

#[cfg(all(feature = "test-support", not(test)))]
/// Integration-test helpers for driving otherwise private TUI code paths.
pub mod test_support {
    use super::App;
    use std::io;
    use std::path::PathBuf;

    /// Mirror of the internal sidebar selection key, exposed for integration tests.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SelectedSidebarKey {
        /// A specific agent row identified by id.
        Agent(uuid::Uuid),
        /// A project header row identified by repo root.
        Project(PathBuf),
    }

    /// Capture the current sidebar selection as a stable key.
    #[must_use]
    pub fn selected_sidebar_key(app: &App) -> Option<SelectedSidebarKey> {
        super::selected_sidebar_key(app).map(|key| match key {
            super::SelectedSidebarKey::Agent(id) => SelectedSidebarKey::Agent(id),
            super::SelectedSidebarKey::Project(root) => SelectedSidebarKey::Project(root),
        })
    }

    /// Restore sidebar selection based on a previously captured key.
    pub fn restore_sidebar_selection(app: &mut App, key: Option<SelectedSidebarKey>) {
        let key = key.map(|key| match key {
            SelectedSidebarKey::Agent(id) => super::SelectedSidebarKey::Agent(id),
            SelectedSidebarKey::Project(root) => super::SelectedSidebarKey::Project(root),
        });
        super::restore_sidebar_selection(app, key);
    }

    /// Flush any pending clipboard payload using the OSC52 protocol.
    pub fn flush_pending_clipboard(stdout: &mut dyn io::Write, app: &mut App) {
        super::flush_pending_clipboard(stdout, app);
    }

    /// Wrapper around the internal state file tracker to allow driving reload paths from
    /// integration tests without depending on private types.
    pub struct StateFileTracker(super::StateFileTracker);

    impl std::fmt::Debug for StateFileTracker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("StateFileTracker").finish()
        }
    }

    impl StateFileTracker {
        /// Create a new tracker for the app's current state file.
        #[must_use]
        pub fn new(app: &App) -> Self {
            Self(super::StateFileTracker::new(app))
        }

        /// Force the next reload check to run immediately.
        pub fn force_due(&mut self) {
            self.0.last_check = super::Instant::now()
                .checked_sub(super::Duration::from_millis(
                    super::STATE_FILE_SYNC_INTERVAL_MS + 1,
                ))
                .unwrap_or_else(super::Instant::now);
        }

        /// Reload app state from disk if the state file has changed.
        pub fn maybe_reload_state(&mut self, app: &mut App) -> bool {
            self.0.maybe_reload_state(app)
        }
    }
}

#[cfg(test)]
mod tests;
