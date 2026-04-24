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
    if tui_run_failpoint() == Some(TuiRunFailpoint::PollImmediate) {
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

fn tui_run_failpoint() -> Option<TuiRunFailpoint> {
    #[cfg(not(debug_assertions))]
    {
        None
    }

    #[cfg(debug_assertions)]
    {
        let value = std::env::var("TENEX_TEST_TUI_FAILPOINT").ok()?;
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

fn enable_keyboard_enhancement_with_support(
    stdout: &mut dyn io::Write,
    supported: bool,
) -> bool {
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
    stdout: &mut dyn io::Write,
    app: &mut App,
    batched_keys: &[String],
) {
    send_batched_keys_to_mux(app, batched_keys);
    flush_pending_clipboard(stdout, app);
}

fn init_preview_dimensions(terminal: &impl TerminalInfo, app: &mut App, action_handler: Actions) {
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
    terminal: &impl TerminalInfo,
    app: &mut App,
    event_handler: &impl EventReader,
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

fn should_refresh_diff(
    active_tab: Tab,
    needs_content_update: bool,
    diff_due: bool,
    diff_force_refresh: bool,
) -> bool {
    if active_tab == Tab::Diff || diff_force_refresh {
        return needs_content_update || diff_due || diff_force_refresh;
    }

    diff_due
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
    if app.data.ui.preview_dimensions != Some((preview_width, preview_height)) {
        app.set_preview_dimensions(preview_width, preview_height);
        action_handler.resize_agent_windows(app);
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

fn run_loop(
    terminal: &mut impl TerminalOps,
    app: &mut App,
    event_handler: &impl EventReader,
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
        if should_refresh_diff(
            app.data.active_tab,
            needs_content_update,
            diff_due,
            app.data.ui.diff_force_refresh,
        ) {
            if app.data.active_tab == Tab::Diff || app.data.ui.diff_force_refresh {
                let _ = action_handler.update_diff(app);
            } else {
                let _ = action_handler.update_diff_digest(app);
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
                .checked_sub(super::Duration::from_millis(super::STATE_FILE_SYNC_INTERVAL_MS + 1))
                .unwrap_or_else(super::Instant::now);
        }

        /// Reload app state from disk if the state file has changed.
        pub fn maybe_reload_state(&mut self, app: &mut App) -> bool {
            self.0.maybe_reload_state(app)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::keycode_to_input_sequence;
    use crate::agent::{Agent, Storage};
    use crate::config::Config;
    use crate::state::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use ratatui::text::Line;
    use semver::Version;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn create_test_config() -> Config {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

        // Use a unique temp directory for each test to avoid conflicts and prevent tests from
        // creating worktrees in the real instance directory.
        let pid = std::process::id();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let prefix = format!("tenex-test-{pid}-{id}");
        Config {
            worktree_dir: std::env::temp_dir().join(&prefix),
            branch_prefix: format!("{prefix}/"),
            ..Config::default()
        }
    }

    fn create_test_app() -> App {
        let config = create_test_config();
        let state_path = config.worktree_dir.join("state.json");
        let storage = Storage::with_path(state_path);
        App::new(config, storage, crate::app::Settings::default(), false)
    }

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
    }

    fn selected_agent_id(app: &App) -> Option<uuid::Uuid> {
        app.data
            .selected_sidebar_item()
            .and_then(|item| match item {
                crate::app::SidebarItem::Agent(agent) => Some(agent.info.agent.id),
                crate::app::SidebarItem::Project(_) => None,
            })
    }

    #[inline(never)]
    fn is_normal_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::Normal(_))
    }

    #[inline(never)]
    fn is_keyboard_remap_prompt_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::KeyboardRemapPrompt(_))
    }

    #[inline(never)]
    fn is_changelog_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::Changelog(_))
    }

    #[inline(never)]
    fn is_help_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::Help(_))
    }

    #[inline(never)]
    fn is_preparing_docker_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::PreparingDocker(_))
    }

    #[inline(never)]
    fn is_confirming_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::Confirming(_))
    }

    #[inline(never)]
    fn is_error_modal_mode(mode: &AppMode) -> bool {
        matches!(mode, &AppMode::ErrorModal(_))
    }

    struct FakeEventReader {
        events: RefCell<VecDeque<Event>>,
    }

    impl FakeEventReader {
        fn new(events: Vec<Event>) -> Self {
            Self {
                events: RefCell::new(events.into_iter().collect()),
            }
        }
    }

    impl EventReader for FakeEventReader {
        fn next_event(&self) -> Result<Event> {
            Ok(self.events.borrow_mut().pop_front().unwrap_or(Event::Tick))
        }

        fn poll_immediate(&self) -> Result<bool> {
            Ok(!self.events.borrow().is_empty())
        }
    }

    fn create_test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(width, height)).expect("expected test terminal")
    }

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("boom"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("boom"))
        }
    }

    #[test]
    fn test_failing_writer_flush_errors() {
        let mut out = FailingWriter;
        assert!(std::io::Write::flush(&mut out).is_err());
    }

    #[test]
    fn test_env_var_truthy_accepts_expected_values() {
        assert!(!env_var_truthy(None));
        assert!(env_var_truthy(Some("1")));
        assert!(env_var_truthy(Some("true")));
        assert!(env_var_truthy(Some("TRUE")));
        assert!(env_var_truthy(Some("yes")));
        assert!(env_var_truthy(Some("on")));
        assert!(env_var_truthy(Some("  true ")));
        assert!(!env_var_truthy(Some("0")));
        assert!(!env_var_truthy(Some("no")));
        assert!(!env_var_truthy(Some("off")));
        assert!(!env_var_truthy(Some("")));
        assert!(!env_var_truthy(Some("  ")));
    }

    #[test]
    fn test_app_mode_helpers_cover_both_outcomes() {
        let normal = AppMode::normal();
        let help = HelpMode.into();
        let keyboard_remap = KeyboardRemapPromptMode.into();
        let confirming = ConfirmingMode {
            action: ConfirmAction::Quit,
        }
        .into();
        let error = ErrorModalMode {
            message: "message".to_string(),
        }
        .into();
        let changelog = AppMode::Changelog(ChangelogMode {
            title: "changelog".to_string(),
            lines: Vec::new(),
            mark_seen_version: None,
        });
        let preparing_docker = AppMode::PreparingDocker(PreparingDockerMode {
            message: "preparing".to_string(),
        });

        assert!(is_normal_mode(&normal));
        assert!(!is_normal_mode(&help));

        assert!(is_help_mode(&help));
        assert!(!is_help_mode(&normal));

        assert!(is_keyboard_remap_prompt_mode(&keyboard_remap));
        assert!(!is_keyboard_remap_prompt_mode(&normal));

        assert!(is_changelog_mode(&changelog));
        assert!(!is_changelog_mode(&normal));

        assert!(is_preparing_docker_mode(&preparing_docker));
        assert!(!is_preparing_docker_mode(&normal));

        assert!(is_confirming_mode(&confirming));
        assert!(!is_confirming_mode(&normal));

        assert!(is_error_modal_mode(&error));
        assert!(!is_error_modal_mode(&normal));
    }

    #[test]
    fn test_flush_pending_clipboard_noops_when_none_or_empty() {
        let mut app = create_test_app();

        let mut out = Vec::new();
        flush_pending_clipboard(&mut out, &mut app);
        assert!(out.is_empty());
        assert!(app.data.ui.pending_clipboard.is_none());

        app.data.ui.pending_clipboard = Some(String::new());
        flush_pending_clipboard(&mut out, &mut app);
        assert!(out.is_empty());
        assert!(app.data.ui.pending_clipboard.is_none());
        assert!(app.data.ui.status_message.is_none());
    }

    #[test]
    fn test_flush_pending_clipboard_rejects_oversized_payload() {
        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("x".repeat(OSC52_MAX_BYTES + 1));

        let mut out = Vec::new();
        flush_pending_clipboard(&mut out, &mut app);

        assert!(out.is_empty());
        let message = app.data.ui.status_message.unwrap_or_default();
        assert!(message.contains("Selection too large to copy"));
        assert!(message.contains("max"));
    }

    #[test]
    fn test_flush_pending_clipboard_writes_osc52_and_sets_status() {
        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("hello".to_string());

        let mut out = Vec::new();
        flush_pending_clipboard(&mut out, &mut app);

        let output = String::from_utf8(out).expect("expected utf8 output");
        assert!(output.starts_with("\u{1b}]52;c;"));
        assert!(output.ends_with('\u{7}'));
        assert!(output.contains("aGVsbG8="));
        assert_eq!(app.data.ui.status_message.as_deref(), Some("Copied 1 line"));

        app.data.ui.pending_clipboard = Some("a\nb".to_string());
        let mut out = Vec::new();
        flush_pending_clipboard(&mut out, &mut app);
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Copied 2 lines")
        );
    }

    #[test]
    fn test_flush_pending_clipboard_sets_error_status_on_write_failure() {
        let mut app = create_test_app();
        app.data.ui.pending_clipboard = Some("hello".to_string());

        let mut out = FailingWriter;
        flush_pending_clipboard(&mut out, &mut app);

        let message = app.data.ui.status_message.unwrap_or_default();
        assert!(message.contains("Copy failed"));
    }

    #[test]
    fn test_keyboard_enhancement_helpers_cover_supported_and_error_paths() {
        let mut out = Vec::new();
        with_tracing_dispatch(|| {
            assert!(!enable_keyboard_enhancement_with_support(&mut out, false));
            assert!(out.is_empty());

            assert!(enable_keyboard_enhancement_with_support(&mut out, true));
            assert!(!out.is_empty());

            pop_keyboard_enhancement(&mut out, false);

            let mut failing = FailingWriter;
            pop_keyboard_enhancement(&mut failing, true);
        });
    }

    #[test]
    fn test_apply_startup_modals_shows_keyboard_remap_prompt() {
        let mut app = create_test_app();
        assert!(is_normal_mode(&app.mode));

        apply_startup_modals(&mut app);
        assert!(is_keyboard_remap_prompt_mode(&app.mode));
    }

    #[test]
    fn test_apply_startup_modals_consumes_pending_changelog() {
        let mut app = create_test_app();
        app.data.keyboard_enhancement_supported = true;
        app.data.pending_changelog = Some(ChangelogMode {
            title: "What's New".to_string(),
            lines: vec!["hello".to_string()],
            mark_seen_version: None,
        });

        apply_startup_modals(&mut app);
        assert!(is_changelog_mode(&app.mode));
        assert!(app.data.pending_changelog.is_none());
    }

    #[test]
    fn test_state_file_stamp_from_parts_covers_ok_and_error_paths() {
        let now = SystemTime::now();
        let stamp = state_file_stamp_from_parts(Ok(now), 123);
        assert_eq!(
            stamp,
            Some(StateFileStamp {
                modified: now,
                len: 123
            })
        );

        let stamp = state_file_stamp_from_parts(Err(std::io::Error::other("boom")), 321);
        assert!(stamp.is_none());
    }

    #[test]
    fn test_state_file_tracker_reloads_when_stamp_changes() {
        let mut app = create_test_app();
        let state_path = app.data.storage.resolved_state_path();

        let mut agent_one = Agent::new(
            "one".to_string(),
            "codex".to_string(),
            "tui/state-file-tracker/one".to_string(),
            PathBuf::from("/tmp/tui-state-tracker-one"),
        );
        agent_one.collapsed = true;
        let agent_one_id = agent_one.id;
        let agent_two_id = {
            let agent_two = Agent::new(
                "two".to_string(),
                "codex".to_string(),
                "tui/state-file-tracker/two".to_string(),
                PathBuf::from("/tmp/tui-state-tracker-two"),
            );
            let id = agent_two.id;
            app.data.storage.add(agent_one);
            app.data.storage.add(agent_two);
            id
        };
        let items = app.data.sidebar_items();
        let agent_two_index = items
            .iter()
            .position(|item| match item {
                crate::app::SidebarItem::Agent(agent) => agent.info.agent.id == agent_two_id,
                crate::app::SidebarItem::Project(_) => false,
            })
            .expect("Expected sidebar to contain agent two");
        app.data.selected = agent_two_index;
        app.data.storage.save_to(&state_path).unwrap();

        let mut tracker = StateFileTracker::new(&app);

        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap();
        assert!(!tracker.maybe_reload_state(&mut app));

        let mut disk = crate::agent::Storage::with_path(state_path.clone());
        let extra = Agent::new(
            "extra".to_string(),
            "codex".to_string(),
            "tui/state-file-tracker/extra".to_string(),
            PathBuf::from("/tmp/tui-state-tracker-extra"),
        );
        let mut agent_one_disk = app
            .data
            .storage
            .get(agent_one_id)
            .expect("missing agent one")
            .clone();
        agent_one_disk.collapsed = false;
        let agent_two_disk = app
            .data
            .storage
            .get(agent_two_id)
            .expect("missing agent two")
            .clone();

        disk.add(agent_two_disk);
        disk.add(extra);
        disk.add(agent_one_disk);
        disk.save_to(&state_path).unwrap();

        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap();
        assert!(tracker.maybe_reload_state(&mut app));

        assert_eq!(
            app.data.storage.get(agent_one_id).map(|a| a.collapsed),
            Some(true)
        );
        let selected = selected_agent_id(&app);
        assert_eq!(selected, Some(agent_two_id));

        app.data.select_project_header();
        let selected_project = selected_agent_id(&app);
        assert!(selected_project.is_none());
    }

    #[test]
    fn test_init_preview_dimensions_returns_early_when_terminal_size_errors() {
        struct FailingTerminal;

        impl TerminalInfo for FailingTerminal {
            fn size(&self) -> Result<ratatui::layout::Size> {
                Err(anyhow::anyhow!("boom"))
            }
        }

        let mut app = create_test_app();
        let terminal = FailingTerminal;

        init_preview_dimensions(&terminal, &mut app, Actions::new());

        assert!(app.data.ui.preview_dimensions.is_none());
        assert!(app.data.ui.terminal_dimensions.is_none());
    }

    #[test]
    fn test_init_preview_dimensions_runs_when_only_one_dimension_is_set() {
        struct FixedTerminal;

        impl TerminalInfo for FixedTerminal {
            fn size(&self) -> Result<ratatui::layout::Size> {
                Ok(ratatui::layout::Size::new(100, 40))
            }
        }

        let mut app = create_test_app();
        app.data.ui.preview_dimensions = Some((10, 10));
        app.data.ui.terminal_dimensions = None;

        init_preview_dimensions(&FixedTerminal, &mut app, Actions::new());

        assert_eq!(app.data.ui.terminal_dimensions, Some((100, 40)));
        assert!(app.data.ui.preview_dimensions.is_some());
    }

    #[test]
    fn test_apply_pending_resize_updates_preview_dimensions_and_noops_when_unchanged() {
        let mut app = create_test_app();

        apply_pending_resize(&mut app, Actions::new(), Some((80, 24)));
        assert_eq!(app.data.ui.terminal_dimensions, Some((80, 24)));
        let first_preview = app.data.ui.preview_dimensions;
        assert!(first_preview.is_some());

        apply_pending_resize(&mut app, Actions::new(), Some((80, 24)));
        assert_eq!(app.data.ui.preview_dimensions, first_preview);
    }

    #[test]
    fn test_env_var_truthy_defaults_to_false() {
        assert!(!env_var_truthy(None));
    }

    #[test]
    fn test_enable_keyboard_enhancement_with_support_returns_false_when_unsupported() {
        let mut buffer: Vec<u8> = Vec::new();
        assert!(!enable_keyboard_enhancement_with_support(
            &mut buffer,
            false
        ));
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_enable_keyboard_enhancement_with_support_writes_output_when_supported() {
        let mut buffer: Vec<u8> = Vec::new();
        assert!(enable_keyboard_enhancement_with_support(&mut buffer, true));
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_enable_keyboard_enhancement_with_support_returns_false_when_execute_fails() {
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

        let mut writer = FlushFails { buffer: Vec::new() };
        assert!(!enable_keyboard_enhancement_with_support(&mut writer, true));
        assert!(!writer.buffer.is_empty());
    }

    #[test]
    fn test_pop_keyboard_enhancement_writes_when_enabled() {
        let mut buffer: Vec<u8> = Vec::new();
        pop_keyboard_enhancement(&mut buffer, true);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_pop_keyboard_enhancement_is_noop_when_disabled() {
        let mut buffer: Vec<u8> = Vec::new();
        pop_keyboard_enhancement(&mut buffer, false);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_pop_keyboard_enhancement_warns_when_execute_fails() {
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

        let mut writer = FlushFails { buffer: Vec::new() };
        pop_keyboard_enhancement(&mut writer, true);
        assert!(!writer.buffer.is_empty());
    }

    #[test]
    fn test_env_var_truthy_accepts_truthy_values() {
        for value in ["1", "true", "TRUE", " yes ", "On"] {
            assert_env_var_truthy(value);
        }
    }

    #[test]
    fn test_env_var_truthy_rejects_falsy_values() {
        for value in ["", "0", "false", "no", "off"] {
            assert_env_var_falsy(value);
        }
    }

    fn assert_env_var_truthy(value: &str) {
        assert!(env_var_truthy(Some(value)), "expected truthy for {value:?}");
    }

    fn assert_env_var_falsy(value: &str) {
        assert!(!env_var_truthy(Some(value)), "expected falsy for {value:?}");
    }

    #[test]
    #[should_panic(expected = "expected truthy")]
    fn test_assert_env_var_truthy_panics_on_falsy_value() {
        assert_env_var_truthy("0");
    }

    #[test]
    #[should_panic(expected = "expected falsy")]
    fn test_assert_env_var_falsy_panics_on_truthy_value() {
        assert_env_var_falsy("1");
    }

    #[test]
    fn test_compute_sent_keys_in_preview_covers_both_outcomes() {
        let mut keys: Vec<String> = Vec::new();

        let normal = AppMode::normal();
        let preview = PreviewFocusedMode.into();

        assert!(!compute_sent_keys_in_preview(false, &keys, &normal));

        keys.push("a".to_string());
        assert!(!compute_sent_keys_in_preview(false, &keys, &normal));
        assert!(compute_sent_keys_in_preview(false, &keys, &preview));

        keys.clear();
        assert!(compute_sent_keys_in_preview(true, &keys, &normal));
    }

    #[test]
    fn test_event_reader_impl_for_handler_smoke_does_not_block() {
        let handler = Handler::new(0);
        let _ = EventReader::poll_immediate(&handler);
        let _ = EventReader::next_event(&handler);
    }

    #[test]
    fn test_enter_tui_screen_writes_output() {
        let mut buffer: Vec<u8> = Vec::new();
        enter_tui_screen(&mut buffer, false).unwrap();
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_enter_tui_screen_mouse_capture_writes_more_output() {
        let mut without_mouse: Vec<u8> = Vec::new();
        enter_tui_screen(&mut without_mouse, false).unwrap();

        let mut with_mouse: Vec<u8> = Vec::new();
        enter_tui_screen(&mut with_mouse, true).unwrap();

        assert!(with_mouse.len() >= without_mouse.len());
        #[cfg(not(windows))]
        assert!(with_mouse.len() > without_mouse.len());
    }

    #[test]
    fn test_enter_tui_screen_errors_when_writer_fails_immediately() {
        let mut out = FailingWriter;
        assert!(enter_tui_screen(&mut out, false).is_err());
    }

    #[test]
    fn test_enter_tui_screen_errors_when_mouse_capture_execute_fails() {
        struct FlushFailsOnSecondCall {
            flush_calls: usize,
        }

        impl io::Write for FlushFailsOnSecondCall {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                Ok(buf.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                self.flush_calls = self.flush_calls.saturating_add(1);
                if self.flush_calls >= 2 {
                    return Err(io::Error::other("boom"));
                }
                Ok(())
            }
        }

        let mut writer = FlushFailsOnSecondCall { flush_calls: 0 };
        assert!(enter_tui_screen(&mut writer, true).is_err());
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

        let mut writer = FailingWriter;
        assert!(std::io::Write::flush(&mut writer).is_err());
        flush_pending_clipboard(&mut writer, &mut app);

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
    fn test_write_osc52_clipboard_roundtrips_base64() {
        use base64::Engine as _;
        use base64::engine::general_purpose::STANDARD;

        let content = "line1\nλ line2\n";
        let mut buffer: Vec<u8> = Vec::new();
        write_osc52_clipboard(&mut buffer, content).unwrap();

        let prefix = b"\x1b]52;c;";
        assert!(buffer.starts_with(prefix));
        assert_eq!(buffer.last().copied(), Some(b'\x07'));

        let encoded = &buffer[prefix.len()..buffer.len().saturating_sub(1)];
        let encoded = std::str::from_utf8(encoded).unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(decoded, content.as_bytes());
    }

    #[test]
    fn test_flush_pending_clipboard_payload_decodes_to_original() {
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
        let encoded = std::str::from_utf8(encoded).unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(decoded, content.as_bytes());
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Copied 2 lines")
        );
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
    fn test_state_file_stamp_returns_none_for_missing_file() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("missing.json");
        assert!(state_file_stamp(&missing).is_none());
    }

    #[test]
    fn test_state_file_tracker_reload_restores_sidebar_selection() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let repo_a = dir.path().join("repo-a");
        let repo_b = dir.path().join("repo-b");

        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        let mut storage = Storage::with_path(state_path.clone());
        let agent_a = agent_in_repo("agent-a", repo_a.clone());
        let agent_b = agent_in_repo("agent-b", repo_b.clone());
        let selected_agent_id = agent_b.id;
        storage.add(agent_a);
        storage.add(agent_b);
        storage.save().unwrap();

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
            .expect("Expected agent-b to be present in the sidebar");
        app.data.selected = agent_b_index;
        assert_eq!(
            selected_sidebar_key(&app),
            Some(SelectedSidebarKey::Agent(selected_agent_id))
        );

        let mut tracker = StateFileTracker::new(&app);

        // Another client writes a new agent to disk.
        let mut disk = Storage::load_from(&state_path).unwrap();
        let agent_c = agent_in_repo("agent-c", repo_a);
        let added_agent_id = agent_c.id;
        disk.add(agent_c);
        disk.save_to(&state_path).unwrap();

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
            .expect("Expected repo-b project header to be present in the sidebar");
        app.data.selected = repo_b_header_index;
        let selected_key = selected_sidebar_key(&app).expect("Expected sidebar selection");
        assert_eq!(selected_key, SelectedSidebarKey::Project(repo_b.clone()));

        let mut disk = Storage::load_from(&state_path).unwrap();
        let agent_d = agent_in_repo("agent-d", repo_b.clone());
        disk.add(agent_d);
        disk.save_to(&state_path).unwrap();

        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap_or_else(Instant::now);
        assert!(tracker.maybe_reload_state(&mut app));
        let selected_key = selected_sidebar_key(&app).expect("Expected sidebar selection");
        assert_eq!(selected_key, SelectedSidebarKey::Project(repo_b));
    }

    #[test]
    fn test_state_file_tracker_does_not_reload_when_interval_not_elapsed() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("state.json");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut storage = Storage::with_path(state_path);
        storage.add(agent_in_repo("agent", repo));
        storage.save().unwrap();

        let config = create_test_config();
        let mut app = App::new(config, storage, crate::app::Settings::default(), false);

        let mut tracker = StateFileTracker::new(&app);
        tracker.last_check = Instant::now();
        assert!(!tracker.maybe_reload_state(&mut app));
    }

    #[test]
    fn test_state_file_tracker_does_not_reload_when_stamp_is_unchanged() {
        use tempfile::TempDir;

        let dir = TempDir::new().expect("expected temp dir");
        let state_path = dir.path().join("state.json");

        let mut storage = Storage::with_path(state_path.clone());
        storage
            .save_to(&state_path)
            .expect("expected save to succeed");

        let config = create_test_config();
        let mut app = App::new(config, storage, crate::app::Settings::default(), false);

        let mut tracker = StateFileTracker::new(&app);
        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap_or_else(Instant::now);
        assert!(!tracker.maybe_reload_state(&mut app));
    }

    #[test]
    fn test_state_file_tracker_returns_false_when_state_file_missing() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("missing-state.json");

        let mut storage = Storage::with_path(state_path.clone());
        storage.save_to(&state_path).unwrap();

        let config = create_test_config();
        let mut app = App::new(config, storage, crate::app::Settings::default(), false);

        let mut tracker = StateFileTracker::new(&app);
        std::fs::remove_file(&state_path).unwrap();
        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap_or_else(Instant::now);
        assert!(!tracker.maybe_reload_state(&mut app));
    }

    #[test]
    fn test_state_file_tracker_returns_false_when_state_file_is_corrupt() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let state_path = dir.path().join("corrupt-state.json");

        let storage = Storage::with_path(state_path.clone());
        let config = create_test_config();
        let mut app = App::new(config, storage, crate::app::Settings::default(), false);

        let mut tracker = StateFileTracker::new(&app);

        std::fs::write(&state_path, "{ not valid json").unwrap();
        tracker.last_check = Instant::now()
            .checked_sub(Duration::from_millis(STATE_FILE_SYNC_INTERVAL_MS + 1))
            .unwrap_or_else(Instant::now);
        assert!(!tracker.maybe_reload_state(&mut app));
    }

    #[test]
    fn test_restore_sidebar_selection_none_validates_selection_when_sidebar_empty() {
        let mut app = create_test_app();
        assert_eq!(app.data.sidebar_len(), 0);
        app.data.selected = 10;

        restore_sidebar_selection(&mut app, None);

        assert_eq!(app.data.selected, 0);
    }

    #[test]
    fn test_restore_sidebar_selection_none_validates_selection_when_sidebar_non_empty() {
        let mut app = create_test_app();
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            std::env::temp_dir().join("tenex-test-restore-selection"),
        ));

        let visible_count = app.data.sidebar_len();
        assert!(visible_count > 0);
        app.data.selected = visible_count.saturating_add(10);

        restore_sidebar_selection(&mut app, None);

        assert_eq!(app.data.selected, visible_count.saturating_sub(1));
    }

    #[test]
    fn test_restore_sidebar_selection_key_not_found_keeps_current_selection() {
        let mut app = create_test_app();
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "echo".to_string(),
            "branch".to_string(),
            std::env::temp_dir().join("tenex-test-restore-selection-missing"),
        ));

        let visible_count = app.data.sidebar_len();
        assert!(visible_count > 0);
        app.data.selected = 0;

        restore_sidebar_selection(
            &mut app,
            Some(SelectedSidebarKey::Agent(uuid::Uuid::new_v4())),
        );

        assert_eq!(app.data.selected, 0);
    }

    #[test]
    fn test_apply_startup_modals_shows_keyboard_remap_prompt_when_due() {
        let mut app = create_test_app();
        app.data.keyboard_enhancement_supported = false;
        app.data.settings.keyboard_remap_asked = false;

        apply_startup_modals(&mut app);

        assert!(is_keyboard_remap_prompt_mode(&app.mode));
    }

    #[test]
    fn test_apply_startup_modals_applies_pending_changelog() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.pending_changelog = Some(ChangelogMode {
            title: "What's New".to_string(),
            lines: vec!["hi".to_string()],
            mark_seen_version: None,
        });

        apply_startup_modals(&mut app);

        assert!(is_changelog_mode(&app.mode));
        assert!(app.data.pending_changelog.is_none());
    }

    #[test]
    fn test_apply_startup_modals_does_not_apply_changelog_when_none() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.pending_changelog = None;

        apply_startup_modals(&mut app);

        assert!(is_normal_mode(&app.mode));
    }

    #[test]
    fn test_apply_startup_modals_does_not_apply_changelog_when_modal_open() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.pending_changelog = Some(ChangelogMode {
            title: "What's New".to_string(),
            lines: vec!["hi".to_string()],
            mark_seen_version: None,
        });
        app.enter_mode(HelpMode.into());

        apply_startup_modals(&mut app);

        assert!(is_help_mode(&app.mode));
        assert!(app.data.pending_changelog.is_some());
    }

    /// Test helper that wraps `input::handle_key_event` with an empty `batched_keys` vec
    fn test_key_event(app: &mut App, _handler: Actions, code: KeyCode, modifiers: KeyModifiers) {
        let mut keys = Vec::new();
        input::handle_key_event(app, code, modifiers, &mut keys).expect("expected key event");
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
    fn test_maybe_refresh_preview_updates_timestamp_when_needs_content_update() {
        let mut app = create_test_app();
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = true;

        let mut last_preview_update = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("expected instant subtraction");
        let before = last_preview_update;

        maybe_refresh_preview(
            &mut app,
            Actions::new(),
            true,
            false,
            &mut last_preview_update,
        );

        assert_ne!(last_preview_update, before);
    }

    #[test]
    fn test_maybe_refresh_preview_updates_timestamp_when_sent_keys_in_preview() {
        let mut app = create_test_app();
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = true;

        let mut last_preview_update = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("expected instant subtraction");
        let before = last_preview_update;

        maybe_refresh_preview(
            &mut app,
            Actions::new(),
            false,
            true,
            &mut last_preview_update,
        );

        assert_ne!(last_preview_update, before);
    }

    #[test]
    fn test_maybe_refresh_preview_updates_timestamp_when_preview_due() {
        let mut app = create_test_app();
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = true;

        let mut last_preview_update = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("expected instant subtraction");
        let before = last_preview_update;

        maybe_refresh_preview(
            &mut app,
            Actions::new(),
            false,
            false,
            &mut last_preview_update,
        );

        assert_ne!(last_preview_update, before);
    }

    #[test]
    fn test_maybe_refresh_preview_noops_when_preview_visible_but_not_due() {
        let mut app = create_test_app();
        app.enter_mode(PreviewFocusedMode.into());
        app.data.active_tab = Tab::Diff;
        app.data.ui.preview_follow = true;
        app.data.config.poll_interval_ms = 60_000;

        let mut last_preview_update = Instant::now();
        let before = last_preview_update;

        maybe_refresh_preview(
            &mut app,
            Actions::new(),
            false,
            false,
            &mut last_preview_update,
        );

        assert_eq!(last_preview_update, before);
    }

    #[test]
    fn test_maybe_refresh_preview_noops_when_not_following() {
        let mut app = create_test_app();
        app.data.active_tab = Tab::Preview;
        app.data.ui.preview_follow = false;

        let mut last_preview_update = Instant::now();
        let before = last_preview_update;

        maybe_refresh_preview(
            &mut app,
            Actions::new(),
            false,
            true,
            &mut last_preview_update,
        );

        assert_eq!(last_preview_update, before);
    }

    #[test]
    fn test_should_refresh_diff_skips_selection_updates_when_diff_tab_inactive() {
        assert!(!should_refresh_diff(Tab::Preview, true, false, false));
        assert!(should_refresh_diff(Tab::Preview, false, true, false));
    }

    #[test]
    fn test_should_refresh_diff_still_updates_when_diff_tab_active_or_forced() {
        assert!(should_refresh_diff(Tab::Diff, true, false, false));
        assert!(should_refresh_diff(Tab::Diff, false, true, false));
        assert!(should_refresh_diff(Tab::Preview, false, false, true));
    }

    #[test]
    fn test_should_refresh_commits_skips_selection_updates_when_commits_tab_inactive() {
        assert!(!should_refresh_commits(Tab::Preview, true, false));
        assert!(should_refresh_commits(Tab::Preview, false, true));
    }

    #[test]
    fn test_should_refresh_commits_still_updates_when_commits_tab_active() {
        assert!(should_refresh_commits(Tab::Commits, true, false));
        assert!(should_refresh_commits(Tab::Commits, false, true));
    }

    fn select_first_agent(app: &mut App) {
        let index = app
            .data
            .sidebar_items()
            .iter()
            .position(|item| matches!(item, crate::app::SidebarItem::Agent(_)))
            .expect("expected agent in sidebar");
        app.data.selected = index;
    }

    #[test]
    fn test_send_batched_keys_to_mux_with_selected_agent_without_window_index() {
        let mut app = create_test_app();
        let mut agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.mux_session = "tenex-test-session".to_string();
        app.data.storage.add(agent);
        select_first_agent(&mut app);

        send_batched_keys_to_mux(&app, &[String::from("a")]);
    }

    #[test]
    fn test_send_batched_keys_to_mux_with_selected_agent_with_window_index() {
        let mut app = create_test_app();
        let mut agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.mux_session = "tenex-test-session".to_string();
        agent.window_index = Some(2);
        app.data.storage.add(agent);
        select_first_agent(&mut app);

        send_batched_keys_to_mux(&app, &[String::from("a")]);
    }

    #[test]
    fn test_handle_key_event_normal_mode_quit() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Ctrl+q should trigger quit (since no running agents)
        test_key_event(&mut app, handler, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // '?' should open help
        test_key_event(&mut app, handler, KeyCode::Char('?'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Help(HelpMode));
    }

    #[test]
    fn test_handle_key_event_help_mode_any_key_exits() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(HelpMode.into());
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'a' should enter creating mode
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent_with_prompt() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'A' should enter prompting mode
        test_key_event(&mut app, handler, KeyCode::Char('A'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
    }

    #[test]
    fn test_handle_key_event_creating_mode_char_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('b'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('c'), KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "abc");
    }

    #[test]
    fn test_handle_key_event_creating_mode_backspace() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        app.handle_char('a');
        app.handle_char('b');
        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "a");
    }

    #[test]
    fn test_handle_key_event_creating_mode_escape_cancels() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.input.buffer.is_empty());
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_empty_does_nothing() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        // Enter with empty input should just exit mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        // No agent created since input was empty
        assert_eq!(app.data.storage.len(), 0);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_yes() {
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
        test_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_capital_y() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Char('Y'), KeyModifiers::NONE);
        assert!(app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_no() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_other_key_ignored() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE);

        // Should still be in confirming mode
        assert!(is_confirming_mode(&app.mode));
    }

    #[test]
    fn test_handle_key_event_normal_mode_navigation() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Navigation keys should work in normal mode
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);

        // Should still be in normal mode (no state change visible without agents)
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_normal_mode_tab_switch() {
        let mut app = create_test_app();
        let handler = Actions::new();

        let initial_tab = app.data.active_tab;
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE);
        assert_ne!(app.data.active_tab, initial_tab);
    }

    #[test]
    fn test_handle_key_event_normal_mode_scroll() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Scroll commands
        test_key_event(&mut app, handler, KeyCode::Char('u'), KeyModifiers::CONTROL);
        test_key_event(&mut app, handler, KeyCode::Char('d'), KeyModifiers::CONTROL);
        test_key_event(&mut app, handler, KeyCode::Char('g'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('G'), KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));
    }

    #[test]
    fn test_handle_key_event_unknown_key_does_nothing() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Unknown key should be ignored
        test_key_event(&mut app, handler, KeyCode::F(12), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_prompting_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(PromptingMode.into());
        app.handle_char('t');
        app.handle_char('e');

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_cancel_action() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Escape in normal mode triggers cancel action (does nothing but works)
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter scrolling mode (happens when scroll keys are pressed)
        app.enter_mode(ScrollingMode.into());

        // Should handle navigation keys in scrolling mode
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);
    }

    #[test]
    fn test_handle_key_event_creating_mode_other_keys() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());

        // Other keys like arrows should be ignored in creating mode
        test_key_event(&mut app, handler, KeyCode::Left, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Right, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);

        // Should still be in creating mode
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
    }

    #[test]
    fn test_handle_key_event_prompting_mode_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(PromptingMode.into());

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('h'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('i'), KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "hi");
        assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
    }

    #[test]
    fn test_handle_key_event_confirming_kill() {
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
        test_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE);

        // Should exit to normal mode
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_confirming_reset() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Reset,
            }
            .into(),
        );

        // 'n' should cancel
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_confirming_capital_n() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(
            ConfirmingMode {
                action: ConfirmAction::Quit,
            }
            .into(),
        );

        // 'N' should also cancel
        test_key_event(&mut app, handler, KeyCode::Char('N'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_with_input() {
        use tempfile::TempDir;

        let _guard = crate::test_support::lock_mux_test_environment();
        let non_git_dir = TempDir::new().expect("expected temp dir");

        let mut app = create_test_app();
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());
        for ch in ['t', 'e', 's', 't'] {
            app.handle_char(ch);
        }

        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert_eq!(app.data.storage.len(), 1);
        let agent = &app.data.storage.agents[0];
        assert_eq!(agent.workspace_kind, crate::agent::WorkspaceKind::PlainDir);
        let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
    }

    #[test]
    fn test_handle_key_event_prompting_mode_enter_with_input() {
        use tempfile::TempDir;

        let _guard = crate::test_support::lock_mux_test_environment();
        let non_git_dir = TempDir::new().expect("expected temp dir");

        let mut app = create_test_app();
        app.set_cwd_project_root(Some(non_git_dir.path().to_path_buf()));
        let handler = Actions::new();

        app.enter_mode(PromptingMode.into());
        for ch in ['f', 'i', 'x'] {
            app.handle_char(ch);
        }

        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert_eq!(app.data.storage.len(), 1);
        let agent = &app.data.storage.agents[0];
        assert_eq!(agent.workspace_kind, crate::agent::WorkspaceKind::PlainDir);
        let _ = crate::mux::SessionManager::new().kill(&agent.mux_session);
    }

    #[test]
    fn test_handle_key_event_creating_mode_fallthrough() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(CreatingMode.into());

        // Tab key should fall through to action handling in creating mode
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE);

        // Mode should remain creating (Tab doesn't exit creating mode)
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
    }

    #[test]
    fn test_handle_key_event_scrolling_mode_navigation() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ScrollingMode.into());

        // Test scrolling mode handles normal mode keybindings
        test_key_event(&mut app, handler, KeyCode::Char('g'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('G'), KeyModifiers::NONE);
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('h'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('l'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('l'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('o'), KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "hello");
        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());
        app.handle_char('t');
        app.handle_char('e');

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_backspace() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());
        app.handle_char('a');
        app.handle_char('b');

        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "a");
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_enter_no_agent() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        // Enter with no agent selected should show error modal
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert!(is_error_modal_mode(&app.mode));
        assert!(app.data.ui.last_error.is_some());
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_enter_empty() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BroadcastingMode.into());

        // Enter with empty input should just exit mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_error_modal_dismiss() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Set an error (this enters ErrorModal mode)
        app.set_error("Test error message");
        assert!(is_error_modal_mode(&app.mode));

        // Any key should dismiss the error modal
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.ui.last_error.is_none());
    }

    #[test]
    fn test_handle_key_event_error_modal_dismiss_with_esc() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.set_error("Test error");
        assert!(is_error_modal_mode(&app.mode));

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_child_count_mode_enter() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());

        // Enter should proceed to child prompt
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::ChildPrompt(ChildPromptMode));
    }

    #[test]
    fn test_handle_key_event_child_count_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());

        // Escape should exit mode
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_child_count_mode_up_down() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());
        let initial_count = app.data.spawn.child_count;

        // Up should increment
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.data.spawn.child_count, initial_count + 1);

        // Down should decrement
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.data.spawn.child_count, initial_count);
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildPromptMode.into());

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('t'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('s'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('t'), KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "test");
        assert_eq!(app.mode, AppMode::ChildPrompt(ChildPromptMode));
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildPromptMode.into());
        app.handle_char('t');

        // Escape should exit mode
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.input.buffer.is_empty());
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_enter_no_agent() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.spawn.spawning_under = Some(uuid::Uuid::new_v4());
        app.enter_mode(ChildPromptMode.into());
        for ch in ['t', 'a', 's', 'k'] {
            app.handle_char(ch);
        }

        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert!(is_error_modal_mode(&app.mode));
    }

    #[test]
    fn test_handle_key_event_child_count_mode_other_keys() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ChildCountMode.into());
        let initial_count = app.data.spawn.child_count;

        // Other keys should be ignored
        test_key_event(&mut app, handler, KeyCode::Left, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Right, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE);

        // Should still be in ChildCount mode with same count
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        assert_eq!(app.data.spawn.child_count, initial_count);
    }

    #[test]
    fn test_handle_key_event_review_info_mode_any_key_exits() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewInfoMode.into());

        // Any key should dismiss
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_review_info_mode_esc_exits() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewInfoMode.into());

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_up_down() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewChildCountMode.into());
        let initial_count = app.data.spawn.child_count;

        // Up should increment
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.data.spawn.child_count, initial_count + 1);

        // Down should decrement
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.data.spawn.child_count, initial_count);
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_enter() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewChildCountMode.into());

        // Enter should proceed to branch selector
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::BranchSelector(BranchSelectorMode));
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ReviewChildCountMode.into());

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
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
    fn test_handle_key_event_branch_selector_mode_navigation() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("develop", false),
            create_test_branch_info("remote", true),
        ];
        app.enter_mode(BranchSelectorMode.into());

        assert_eq!(app.data.review.selected, 0);

        // Down should move to next
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.data.review.selected, 1);

        // Down should move to next
        test_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.data.review.selected, 2);

        // Up should move to previous
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.data.review.selected, 1);

        // Up should move to previous
        test_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.data.review.selected, 0);
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_filter() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
        ];
        app.enter_mode(BranchSelectorMode.into());

        // Type characters for filter
        test_key_event(&mut app, handler, KeyCode::Char('m'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE);

        assert_eq!(app.data.review.filter, "ma");
        assert_eq!(app.mode, AppMode::BranchSelector(BranchSelectorMode));
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_backspace() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![create_test_branch_info("main", false)];
        app.data.review.filter = "main".to_string();
        app.enter_mode(BranchSelectorMode.into());

        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.data.review.filter, "mai");
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(BranchSelectorMode.into());
        app.data.review.branches = vec![create_test_branch_info("main", false)];
        app.data.review.filter = "test".to_string();

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        // State should be cleared on escape
        assert!(app.data.review.branches.is_empty());
        assert!(app.data.review.filter.is_empty());
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_enter() {
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
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        // Should have set review_base_branch before attempting spawn
        assert!(app.data.review.base_branch.is_some());
    }

    #[test]
    fn test_handle_key_event_branch_selector_enter_empty() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.data.review.branches = vec![]; // Empty list
        app.enter_mode(BranchSelectorMode.into());

        // Enter with empty list exits mode but doesn't set base branch
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.review.base_branch.is_none());
    }

    #[test]
    fn test_handle_key_event_review_swarm_no_agent() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Press 'R' with no agent selected
        test_key_event(&mut app, handler, KeyCode::Char('R'), KeyModifiers::NONE);

        // Should show ReviewInfo mode
        assert_eq!(app.mode, AppMode::ReviewInfo(ReviewInfoMode));
    }

    // === Git Operations Key Event Tests ===

    #[test]
    fn test_handle_key_event_confirm_push_mode_no() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // 'n' should cancel and exit
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_handle_key_event_confirm_push_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // Escape should cancel and exit
        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_handle_key_event_confirm_push_yes() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // 'y' should try to execute push (will fail, no agent in storage)
        test_key_event(&mut app, handler, KeyCode::Char('Y'), KeyModifiers::NONE);

        // Should show error (no agent in storage)
        assert!(is_error_modal_mode(&app.mode));
    }

    #[test]
    fn test_handle_key_event_rename_branch_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.git_op.branch_name = "feature/old".to_string();
        app.data.input.buffer = "feature/old".to_string();
        app.data.input.cursor = app.data.input.buffer.len(); // Cursor at end

        // Type some characters
        test_key_event(&mut app, handler, KeyCode::Char('-'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Char('w'), KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "feature/old-new");
        assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    }

    #[test]
    fn test_handle_key_event_rename_branch_backspace() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.input.buffer = "feature/test".to_string();
        app.data.input.cursor = app.data.input.buffer.len(); // Cursor at end

        test_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE);

        assert_eq!(app.data.input.buffer, "feature/tes");
    }

    #[test]
    fn test_handle_key_event_rename_branch_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.input.buffer = "feature/test".to_string();

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none()); // State cleared
    }

    #[test]
    fn test_handle_key_event_rename_branch_enter() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(RenameBranchMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.original_branch = "feature/old".to_string();
        app.data.git_op.branch_name = "feature/old".to_string();
        app.data.input.buffer = "feature/new".to_string();

        // Enter tries to confirm rename and execute (will fail without agent)
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        // Branch name should have been updated before failing
        assert_eq!(app.data.git_op.branch_name, "feature/new");
        // Should show error (no agent in storage)
        assert!(is_error_modal_mode(&app.mode));
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_no() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushForPRMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();

        // 'n' should cancel and exit
        test_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none()); // State cleared
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushForPRMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());

        test_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE);

        assert_eq!(app.mode, AppMode::normal());
        assert!(app.data.git_op.agent_id.is_none());
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_yes() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushForPRMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());
        app.data.git_op.branch_name = "test".to_string();
        app.data.git_op.base_branch = "main".to_string();

        // 'y' should try to push and open PR (will fail, no agent in storage)
        test_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE);

        // Should show error (no agent in storage)
        assert!(is_error_modal_mode(&app.mode));
    }

    #[test]
    fn test_handle_key_event_confirm_push_other_keys_ignored() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(ConfirmPushMode.into());
        app.data.git_op.agent_id = Some(uuid::Uuid::new_v4());

        // Other keys should be ignored
        test_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE);
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);

        // Should still be in ConfirmPush mode
        assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
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
    fn test_handle_key_event_preview_focused_ctrl_q_exits() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(PreviewFocusedMode.into());
        assert_eq!(app.mode, AppMode::PreviewFocused(PreviewFocusedMode));

        // Ctrl+q should exit preview focus mode
        test_key_event(&mut app, handler, KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_preview_focused_collects_keys() {
        let mut app = create_test_app();

        app.enter_mode(PreviewFocusedMode.into());

        // Regular keys should be collected for batching (not change mode)
        let mut keys = Vec::new();
        input::handle_key_event(&mut app, KeyCode::Char('a'), KeyModifiers::NONE, &mut keys)
            .expect("expected key event");
        assert_eq!(app.mode, AppMode::PreviewFocused(PreviewFocusedMode));
        assert_eq!(keys, vec!["a".to_string()]);

        // Special keys also collected
        input::handle_key_event(&mut app, KeyCode::Enter, KeyModifiers::NONE, &mut keys)
            .expect("expected key event");
        assert_eq!(keys, vec!["a".to_string(), "\r".to_string()]);
    }

    #[test]
    fn test_handle_key_event_preview_focused_ctrl_c_prompts_for_non_terminal_agent() {
        let mut app = create_test_app();
        app.data.storage.add(Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        ));
        select_first_agent(&mut app);
        app.enter_mode(PreviewFocusedMode.into());

        let mut keys = Vec::new();
        input::handle_key_event(
            &mut app,
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            &mut keys,
        )
        .expect("expected ctrl-c dispatch");

        assert!(keys.is_empty());
        assert_eq!(
            app.mode,
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::InterruptAgent
            })
        );
    }

    #[test]
    fn test_handle_key_event_focus_preview_action() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Without agent selected, FocusPreview should not change mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_drain_events_tracks_resize() {
        let mut app = create_test_app();
        let terminal = create_test_terminal(80, 24);
        let events = FakeEventReader::new(vec![Event::Resize(120, 40)]);

        let (batched_keys, last_resize, flushed_batched_keys) =
            drain_events(&terminal, &mut app, &events).expect("expected drain events");

        assert!(batched_keys.is_empty());
        assert_eq!(last_resize, Some((120, 40)));
        assert!(!flushed_batched_keys);
    }

    #[test]
    fn test_drain_events_uses_fallback_dimensions_when_terminal_size_errors() {
        struct SizeFails;

        impl TerminalInfo for SizeFails {
            fn size(&self) -> Result<ratatui::layout::Size> {
                Err(anyhow::anyhow!("boom"))
            }
        }

        let mut app = create_test_app();
        let terminal = SizeFails;
        let events = FakeEventReader::new(vec![Event::Tick]);

        let (batched_keys, last_resize, flushed_batched_keys) =
            drain_events(&terminal, &mut app, &events).expect("expected drain events");

        assert!(batched_keys.is_empty());
        assert!(last_resize.is_none());
        assert!(!flushed_batched_keys);
    }

    #[test]
    fn test_drain_events_propagates_next_event_errors() {
        struct NextEventFails;

        impl EventReader for NextEventFails {
            fn next_event(&self) -> Result<Event> {
                let calls = NEXT_EVENT_CALLS.get();
                NEXT_EVENT_CALLS.set(calls.saturating_add(1));
                if calls == 0 {
                    Ok(Event::Resize(80, 24))
                } else {
                    Err(anyhow::anyhow!("boom"))
                }
            }

            fn poll_immediate(&self) -> Result<bool> {
                let calls = NEXT_EVENT_POLL_CALLS.get();
                NEXT_EVENT_POLL_CALLS.set(calls.saturating_add(1));
                Ok(calls == 0)
            }
        }

        thread_local! {
            static NEXT_EVENT_CALLS: Cell<usize> = const { Cell::new(0) };
            static NEXT_EVENT_POLL_CALLS: Cell<usize> = const { Cell::new(0) };
        }

        let mut app = create_test_app();
        let terminal = create_test_terminal(80, 24);
        let events = NextEventFails;

        let err = drain_events(&terminal, &mut app, &events).expect_err("expected drain error");
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_drain_events_propagates_poll_immediate_errors() {
        struct PollFails;

        impl EventReader for PollFails {
            fn next_event(&self) -> Result<Event> {
                Ok(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))
            }

            fn poll_immediate(&self) -> Result<bool> {
                Err(anyhow::anyhow!("boom"))
            }
        }

        let mut app = create_test_app();
        let terminal = create_test_terminal(80, 24);
        let events = PollFails;

        let err = drain_events(&terminal, &mut app, &events).expect_err("expected drain error");
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_drain_events_propagates_handle_key_event_errors() {
        let mut app = create_test_app();
        app.data.storage.add(Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "branch".to_string(),
            PathBuf::from("/tmp"),
        ));
        select_first_agent(&mut app);
        app.apply_mode(PreviewFocusedMode.into());

        let terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let events = FakeEventReader::new(vec![Event::Key(key)]);

        let err = crate::action::with_forced_infallible_action_error_for_tests(|| {
            drain_events(&terminal, &mut app, &events).expect_err("expected drain error")
        });

        assert!(err.to_string().contains("forced infallible action error"));
    }

    #[test]
    fn test_drain_events_ignores_key_release() {
        let mut app = create_test_app();
        app.apply_mode(PreviewFocusedMode.into());

        let terminal = create_test_terminal(80, 24);
        let key = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        let events = FakeEventReader::new(vec![Event::Key(key)]);

        let (batched_keys, last_resize, flushed_batched_keys) =
            drain_events(&terminal, &mut app, &events).expect("expected drain events");

        assert!(batched_keys.is_empty());
        assert!(last_resize.is_none());
        assert!(!flushed_batched_keys);
    }

    #[test]
    fn test_drain_events_flushes_batched_keys_before_left_click() {
        let mut app = create_test_app();
        app.apply_mode(PreviewFocusedMode.into());

        let terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 70,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        let events = FakeEventReader::new(vec![Event::Key(key), Event::Mouse(click)]);

        let (_batched_keys, _last_resize, flushed_batched_keys) =
            drain_events(&terminal, &mut app, &events).expect("expected drain events");

        assert!(flushed_batched_keys);
    }

    #[test]
    fn test_drain_events_does_not_flush_batched_keys_without_left_click() {
        let mut app = create_test_app();
        app.apply_mode(PreviewFocusedMode.into());

        let terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: 70,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        let events = FakeEventReader::new(vec![Event::Key(key), Event::Mouse(click)]);

        let (batched_keys, _last_resize, flushed_batched_keys) =
            drain_events(&terminal, &mut app, &events).expect("expected drain events");

        assert_eq!(batched_keys, vec!["a".to_string()]);
        assert!(!flushed_batched_keys);
    }

    #[test]
    fn test_run_loop_propagates_drain_events_errors() {
        struct DrainFails;

        impl EventReader for DrainFails {
            fn next_event(&self) -> Result<Event> {
                let calls = DRAIN_EVENT_CALLS.get();
                DRAIN_EVENT_CALLS.set(calls.saturating_add(1));
                if calls == 0 {
                    Ok(Event::Resize(80, 24))
                } else {
                    Err(anyhow::anyhow!("boom"))
                }
            }

            fn poll_immediate(&self) -> Result<bool> {
                let calls = DRAIN_POLL_CALLS.get();
                DRAIN_POLL_CALLS.set(calls.saturating_add(1));
                Ok(calls == 0)
            }
        }

        thread_local! {
            static DRAIN_EVENT_CALLS: Cell<usize> = const { Cell::new(0) };
            static DRAIN_POLL_CALLS: Cell<usize> = const { Cell::new(0) };
        }

        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;

        let mut terminal = create_test_terminal(80, 24);
        let events = DrainFails;
        let mut clipboard_out: Vec<u8> = Vec::new();

        let err = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect_err("expected run loop to error");
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_run_loop_propagates_terminal_draw_errors() {
        struct DrawFails;

        impl TerminalInfo for DrawFails {
            fn size(&self) -> Result<ratatui::layout::Size> {
                Ok(ratatui::layout::Size::new(80, 24))
            }
        }

        impl TerminalOps for DrawFails {
            fn draw(&mut self, _app: &App) -> Result<()> {
                Err(anyhow::anyhow!("boom"))
            }
        }

        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.should_quit = true;

        let mut terminal = DrawFails;
        let events = FakeEventReader::new(vec![Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let err = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect_err("expected run loop to error");
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_run_loop_sets_needs_content_update_when_state_file_changes() {
        struct ScriptedEventReader {
            events: RefCell<VecDeque<(Option<Duration>, Event)>>,
        }

        impl ScriptedEventReader {
            fn new(events: Vec<(Option<Duration>, Event)>) -> Self {
                Self {
                    events: RefCell::new(events.into_iter().collect()),
                }
            }
        }

        impl EventReader for ScriptedEventReader {
            fn next_event(&self) -> Result<Event> {
                if let Some((delay, event)) = self.events.borrow_mut().pop_front() {
                    if let Some(delay) = delay {
                        std::thread::sleep(delay);
                    }
                    Ok(event)
                } else {
                    Ok(Event::Tick)
                }
            }

            fn poll_immediate(&self) -> Result<bool> {
                Ok(true)
            }
        }

        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;

        let state_path = app.data.storage.resolved_state_path();
        app.data
            .storage
            .save()
            .expect("expected initial state file write");

        let mut disk_agent = Agent::new(
            "disk-agent".to_string(),
            "echo".to_string(),
            format!("{}disk-agent", app.data.config.branch_prefix),
            std::env::temp_dir().join("tenex-test-state-reload"),
        );
        disk_agent.repo_root = Some(std::env::temp_dir().join("tenex-test-state-reload-repo"));
        let disk_agent_id = disk_agent.id;

        let writer_handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut disk =
                Storage::load_from(&state_path).expect("expected storage to load from disk");
            disk.add(disk_agent);
            disk.save_to(&state_path)
                .expect("expected state file to be writable");
        });

        let mut terminal = create_test_terminal(80, 24);
        let key_quit = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let events = ScriptedEventReader::new(vec![
            (None, Event::Tick),
            (Some(Duration::from_millis(300)), Event::Key(key_quit)),
        ]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
        writer_handle.join().expect("disk writer thread panicked");
        assert!(app.data.storage.get(disk_agent_id).is_some());
    }

    #[test]
    fn test_run_loop_exits_when_should_quit_is_set() {
        let mut app = create_test_app();
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let events = FakeEventReader::new(vec![Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
    }

    #[test]
    fn test_run_loop_marks_sent_keys_in_preview_when_flushed_before_mouse_click() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.apply_mode(PreviewFocusedMode.into());
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 70,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        let events = FakeEventReader::new(vec![Event::Key(key), Event::Mouse(click), Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
    }

    #[test]
    fn test_run_loop_marks_sent_keys_in_preview_when_batched_keys_pending_in_preview_focus() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.apply_mode(PreviewFocusedMode.into());
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let events = FakeEventReader::new(vec![Event::Key(key), Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
    }

    #[test]
    fn test_run_loop_updates_diff_when_force_refresh_true_outside_diff_tab() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.active_tab = Tab::Preview;
        app.data.ui.diff_force_refresh = true;
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let events = FakeEventReader::new(vec![Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
    }

    #[test]
    fn test_run_loop_detects_tab_change_and_refreshes_commits_tab() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        let events = FakeEventReader::new(vec![Event::Key(tab), Event::Key(tab), Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
        assert_eq!(app.data.active_tab, Tab::Commits);
    }

    #[test]
    fn test_run_loop_continues_after_finishing_preparing_docker() {
        let mut app = create_test_app();
        app.apply_mode(
            PreparingDockerMode {
                message: "Preparing Docker".to_string(),
            }
            .into(),
        );
        app.data.should_quit = true;

        let missing = PathBuf::from("/definitely/missing/tenex-docker");
        crate::runtime::with_docker_program_override_for_tests(missing, || {
            let mut terminal = create_test_terminal(80, 24);
            let events = FakeEventReader::new(vec![Event::Tick]);
            let mut clipboard_out: Vec<u8> = Vec::new();

            let result = run_loop(
                &mut terminal,
                &mut app,
                &events,
                Actions::new(),
                &mut clipboard_out,
            )
            .expect("expected run loop to exit");

            assert!(result.is_none());
        });

        assert!(!is_preparing_docker_mode(&app.mode));
    }

    #[test]
    fn test_run_loop_returns_update_info_when_update_requested() {
        let mut app = create_test_app();
        let info = UpdateInfo {
            current_version: Version::parse("1.0.0").unwrap(),
            latest_version: Version::parse("2.0.0").unwrap(),
        };
        app.apply_mode(UpdateRequestedMode { info: info.clone() }.into());

        let mut terminal = create_test_terminal(80, 24);
        let events = FakeEventReader::new(vec![Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert_eq!(result, Some(info));
    }

    #[test]
    fn test_run_loop_shows_keyboard_remap_prompt_when_due() {
        let mut app = create_test_app();
        app.data.keyboard_enhancement_supported = false;
        app.data.settings.keyboard_remap_asked = false;
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let events = FakeEventReader::new(vec![Event::Tick]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
        assert!(is_keyboard_remap_prompt_mode(&app.mode));
    }

    #[test]
    fn test_run_loop_marks_selected_agent_pane_seen_on_selection_change() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        let mut agent = Agent::new(
            "agent".to_string(),
            "echo".to_string(),
            "tenex/test-branch".to_string(),
            std::env::temp_dir().join("tenex-test-selection-change"),
        );
        agent.repo_root = Some(std::env::temp_dir().join("tenex-test-selection-change-repo"));
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.ui.observe_agent_pane_digest(agent_id, 123);
        app.data.selected = 0;
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let events = FakeEventReader::new(vec![Event::Key(key)]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
        assert_eq!(app.data.selected, 1);
        assert_eq!(
            app.data.ui.pane_last_seen_hash_by_agent.get(&agent_id),
            Some(&123)
        );
    }

    #[test]
    fn test_run_loop_selection_change_skips_marking_pane_seen_when_not_on_agent() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        let mut agent = Agent::new(
            "agent".to_string(),
            "echo".to_string(),
            "tenex/test-branch".to_string(),
            std::env::temp_dir().join("tenex-test-selection-change-header"),
        );
        agent.repo_root = Some(std::env::temp_dir().join("tenex-test-selection-change-repo"));
        let agent_id = agent.id;
        app.data.storage.add(agent);
        app.data.ui.observe_agent_pane_digest(agent_id, 123);
        app.data.selected = 1;
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let events = FakeEventReader::new(vec![Event::Key(key)]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
        assert_eq!(app.data.selected, 0);
        assert!(
            !app.data
                .ui
                .pane_last_seen_hash_by_agent
                .contains_key(&agent_id)
        );
    }

    #[test]
    fn test_run_loop_detects_preview_follow_changes() {
        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.ui.preview_dimensions = Some((70, 5));
        app.data.ui.terminal_dimensions = Some((80, 24));
        app.data.ui.preview_follow = true;
        app.data.ui.preview_scroll = 15;
        app.data
            .ui
            .preview_text
            .lines
            .extend((0..20).map(|_| Line::from("x")));
        app.data.should_quit = true;

        let mut terminal = create_test_terminal(80, 24);
        let scroll = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 70,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        let events = FakeEventReader::new(vec![Event::Mouse(scroll)]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
        assert!(!app.data.ui.preview_follow);
    }

    #[test]
    fn test_run_loop_triggers_periodic_refresh_after_time_passes() {
        struct ScriptedEventReader {
            events: RefCell<VecDeque<(Option<Duration>, Event)>>,
        }

        impl ScriptedEventReader {
            fn new(events: Vec<(Option<Duration>, Event)>) -> Self {
                Self {
                    events: RefCell::new(events.into_iter().collect()),
                }
            }
        }

        impl EventReader for ScriptedEventReader {
            fn next_event(&self) -> Result<Event> {
                if let Some((delay, event)) = self.events.borrow_mut().pop_front() {
                    if let Some(delay) = delay {
                        std::thread::sleep(delay);
                    }
                    Ok(event)
                } else {
                    Ok(Event::Tick)
                }
            }

            fn poll_immediate(&self) -> Result<bool> {
                Ok(true)
            }
        }

        let mut app = create_test_app();
        app.data.settings.keyboard_remap_asked = true;
        app.data.ui.preview_dimensions = Some((70, 5));
        app.data.ui.terminal_dimensions = Some((80, 24));
        app.data.ui.preview_follow = true;
        app.data.ui.preview_scroll = 15;
        app.data
            .ui
            .preview_text
            .lines
            .extend((0..20).map(|_| Line::from("x")));

        let mut terminal = create_test_terminal(80, 24);
        let key_quit = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let events = ScriptedEventReader::new(vec![
            (None, Event::Tick),
            (Some(Duration::from_millis(1200)), Event::Tick),
            (None, Event::Key(key_quit)),
        ]);
        let mut clipboard_out: Vec<u8> = Vec::new();

        let result = run_loop(
            &mut terminal,
            &mut app,
            &events,
            Actions::new(),
            &mut clipboard_out,
        )
        .expect("expected run loop to exit");

        assert!(result.is_none());
    }
}
