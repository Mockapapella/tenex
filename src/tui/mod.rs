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
    app.data.keyboard_enhancement_supported = keyboard_enhancement_enabled;

    // Show keyboard remap prompt if terminal doesn't support enhancement
    // and user hasn't been asked yet
    if matches!(app.mode, AppMode::Normal(_)) && app.should_show_keyboard_remap_prompt() {
        app.show_keyboard_remap_prompt();
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let event_handler = Handler::new(app.data.config.poll_interval_ms);
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
    if app.data.ui.preview_dimensions.is_none()
        && let Ok(size) = terminal.size()
    {
        let area = Rect::new(0, 0, size.width, size.height);
        let (width, height) = render::calculate_preview_dimensions(area);
        app.set_preview_dimensions(width, height);
        action_handler.resize_agent_windows(app);
        app.ensure_agent_list_scroll();
    }

    // Track selection to detect changes
    let mut last_selected = app.data.selected;
    // Track active tab so we can refresh content when switching tabs
    let mut last_tab = app.data.active_tab;
    // Force initial preview/diff update
    let mut needs_content_update = true;
    // Diff refresh is expensive; throttle tick-based updates.
    let diff_refresh_interval = Duration::from_millis(1000);
    let mut last_diff_update = Instant::now();

    loop {
        // If we returned to normal mode and still need to show the keyboard prompt,
        // display it now (e.g., after dismissing another startup modal).
        if matches!(app.mode, AppMode::Normal(_)) && app.should_show_keyboard_remap_prompt() {
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
                        input::handle_key_event(app, key.code, key.modifiers, &mut batched_keys)?;
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
        let sent_keys_in_preview =
            !batched_keys.is_empty() && matches!(app.mode, AppMode::PreviewFocused(_));
        send_batched_keys_to_mux(app, &batched_keys);

        // Apply final resize if any occurred
        if let Some((width, height)) = last_resize {
            let (preview_width, preview_height) =
                render::calculate_preview_dimensions(Rect::new(0, 0, width, height));
            if app.data.ui.preview_dimensions != Some((preview_width, preview_height)) {
                app.set_preview_dimensions(preview_width, preview_height);
                action_handler.resize_agent_windows(app);
                app.ensure_agent_list_scroll();
            }
        }

        // Detect selection change
        if app.data.selected != last_selected {
            last_selected = app.data.selected;
            needs_content_update = true;
        }
        // Detect tab change
        if app.data.active_tab != last_tab {
            last_tab = app.data.active_tab;
            needs_content_update = true;
        }

        // Update preview/diff only on tick, selection change, or after sending keys
        // This avoids spawning mux/git subprocesses every frame
        if needs_tick || needs_content_update || sent_keys_in_preview {
            let _ = action_handler.update_preview(app);
            // Only update diff on tick (it's slow and not needed while typing)
            if (needs_tick || needs_content_update) && app.data.active_tab == Tab::Diff {
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
    use crate::agent::Storage;
    use crate::config::Config;
    use crate::state::*;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use std::path::PathBuf;

    /// Helper struct that cleans up test worktrees and branches on drop
    struct TestCleanup {
        branch_prefix: String,
    }

    impl TestCleanup {
        fn new(branch_prefix: &str) -> Self {
            Self {
                branch_prefix: branch_prefix.to_string(),
            }
        }
    }

    impl Drop for TestCleanup {
        fn drop(&mut self) {
            // Clean up any worktrees/branches created by this test
            if let Ok(repo) = git2::Repository::open(".") {
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
        assert!(
            matches!(&app.mode, AppMode::ErrorModal(_))
                || app.mode == AppMode::normal()
                || matches!(
                    &app.mode,
                    AppMode::Confirming(ConfirmingMode {
                        action: ConfirmAction::WorktreeConflict,
                    })
                ),
            "Expected ErrorModal, Normal, or Confirming(WorktreeConflict) mode, got {:?}",
            app.mode
        );
        // One of these should be true:
        // - Error was set (no git repo or other failure)
        // - Agent was created
        // - Worktree conflict detected (waiting for user input)
        assert!(
            app.data.ui.last_error.is_some()
                || app.data.storage.len() == 1
                || app.data.spawn.worktree_conflict.is_some()
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
        assert!(
            matches!(&app.mode, AppMode::ErrorModal(_))
                || app.mode == AppMode::normal()
                || matches!(
                    &app.mode,
                    AppMode::Confirming(ConfirmingMode {
                        action: ConfirmAction::WorktreeConflict,
                    })
                ),
            "Expected ErrorModal, Normal, or Confirming(WorktreeConflict) mode, got {:?}",
            app.mode
        );
        // One of these should be true:
        // - Error was set (no git repo or other failure)
        // - Agent was created
        // - Worktree conflict detected (waiting for user input)
        assert!(
            app.data.ui.last_error.is_some()
                || app.data.storage.len() == 1
                || app.data.spawn.worktree_conflict.is_some()
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
        assert!(
            matches!(&app.mode, AppMode::ErrorModal(_))
                || app.mode == AppMode::normal()
                || matches!(
                    &app.mode,
                    AppMode::Confirming(ConfirmingMode {
                        action: ConfirmAction::WorktreeConflict,
                    })
                ),
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
    fn test_handle_key_event_focus_preview_action() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Without agent selected, FocusPreview should not change mode
        test_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }
}
