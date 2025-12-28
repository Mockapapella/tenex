//! Terminal User Interface for Tenex

mod input;
mod render;

use crate::app::{Actions, App, Event, Handler, Mode, Tab};
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{
            self as crossterm_event, DisableMouseCapture, EnableMouseCapture, KeyEventKind,
            KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
        },
        execute,
        terminal::{
            EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
            supports_keyboard_enhancement,
        },
    },
    layout::Rect,
};
use std::io;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Run the TUI application
///
/// Returns `Ok(Some(UpdateInfo))` if the user accepted an update prompt and the
/// binary should reinstall and restart. Otherwise returns `Ok(None)` on normal exit.
///
/// # Errors
/// Returns an error if the terminal cannot be initialized or restored (raw mode,
/// alternate screen, mouse capture), or if the main event loop fails to poll input
/// or render frames.
pub fn run(mut app: App) -> Result<Option<UpdateInfo>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

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
    app.keyboard_enhancement_supported = keyboard_enhancement_enabled;

    // Show keyboard remap prompt if terminal doesn't support enhancement
    // and user hasn't been asked yet
    if matches!(app.mode, Mode::Normal) && app.should_show_keyboard_remap_prompt() {
        app.show_keyboard_remap_prompt();
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let event_handler = Handler::new(app.config.poll_interval_ms);
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

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_handler: &Handler,
    action_handler: Actions,
) -> Result<Option<UpdateInfo>> {
    // Initialize preview dimensions before first draw
    if app.ui.preview_dimensions.is_none()
        && let Ok(size) = terminal.size()
    {
        let area = Rect::new(0, 0, size.width, size.height);
        let (width, height) = render::calculate_preview_dimensions(area);
        app.set_preview_dimensions(width, height);
        action_handler.resize_agent_windows(app);
        app.ensure_agent_list_scroll();
    }

    // Track selection to detect changes
    let mut last_selected = app.selected;
    // Track active tab so we can refresh content when switching tabs
    let mut last_tab = app.active_tab;
    // Force initial preview/diff update
    let mut needs_content_update = true;
    // Diff refresh is expensive; throttle tick-based updates.
    let diff_refresh_interval = Duration::from_millis(1000);
    let mut last_diff_update = Instant::now();

    loop {
        // If we returned to normal mode and still need to show the keyboard prompt,
        // display it now (e.g., after dismissing another startup modal).
        if matches!(app.mode, Mode::Normal) && app.should_show_keyboard_remap_prompt() {
            app.show_keyboard_remap_prompt();
        }

        // Drain all queued events first (without drawing)
        // This prevents lag when returning focus after being away,
        // since mouse events queue up while the app is unfocused
        let mut needs_tick = false;
        let mut last_resize: Option<(u16, u16)> = None;
        // Batch keys for PreviewFocused mode to avoid per-keystroke process spawning
        let mut batched_keys: Vec<String> = Vec::new();

        loop {
            match event_handler.next()? {
                Event::Tick => {
                    needs_tick = true;
                    break; // Timeout - exit inner loop
                }
                Event::Key(key) => {
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                        input::handle_key_event(
                            app,
                            action_handler,
                            key.code,
                            key.modifiers,
                            &mut batched_keys,
                        )?;
                    }
                }
                Event::Mouse(_) => {
                    // Ignore mouse events (we don't use them)
                }
                Event::Resize(w, h) => {
                    last_resize = Some((w, h)); // Only keep final resize
                }
            }

            // Check if more events are immediately available
            if !crossterm_event::poll(Duration::ZERO)? {
                break; // Queue empty, exit inner loop
            }
        }

        // Send batched keys to the mux in one command (much faster than per-keystroke)
        let sent_keys_in_preview = !batched_keys.is_empty() && app.mode == Mode::PreviewFocused;
        send_batched_keys_to_mux(app, &batched_keys);

        // Apply final resize if any occurred
        if let Some((width, height)) = last_resize {
            let (preview_width, preview_height) =
                render::calculate_preview_dimensions(Rect::new(0, 0, width, height));
            if app.ui.preview_dimensions != Some((preview_width, preview_height)) {
                app.set_preview_dimensions(preview_width, preview_height);
                action_handler.resize_agent_windows(app);
                app.ensure_agent_list_scroll();
            }
        }

        // Detect selection change
        if app.selected != last_selected {
            last_selected = app.selected;
            needs_content_update = true;
        }
        // Detect tab change
        if app.active_tab != last_tab {
            last_tab = app.active_tab;
            needs_content_update = true;
        }

        // Update preview/diff only on tick, selection change, or after sending keys
        // This avoids spawning mux/git subprocesses every frame
        if needs_tick || needs_content_update || sent_keys_in_preview {
            let _ = action_handler.update_preview(app);
            // Only update diff on tick (it's slow and not needed while typing)
            if (needs_tick || needs_content_update) && app.active_tab == Tab::Diff {
                let should_update_diff =
                    needs_content_update || last_diff_update.elapsed() >= diff_refresh_interval;
                if should_update_diff {
                    let _ = action_handler.update_diff(app);
                    last_diff_update = Instant::now();
                }
            }
            needs_content_update = false;
        }

        // Draw ONCE after draining all queued events
        terminal.draw(|frame| render::render(frame, app))?;

        // Sync agent status only on tick (less frequent operation)
        if needs_tick {
            let _ = action_handler.sync_agent_status(app);
        }

        if let Mode::UpdateRequested(info) = &app.mode {
            return Ok(Some(info.clone()));
        }

        if app.should_quit {
            break;
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests;
