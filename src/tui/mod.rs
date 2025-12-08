//! Terminal User Interface for Tenex

mod render;

use anyhow::Result;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{
            self as crossterm_event, DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers,
        },
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::Rect,
};
use std::io;
use std::process::Command;
use std::time::Duration;
use tenex::app::{Actions, App, ConfirmAction, Event, Handler, Mode};
use tenex::config::Action;
use uuid::Uuid;

/// Run the TUI application
pub fn run(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let event_handler = Handler::new(app.config.poll_interval_ms);
    let action_handler = Actions::new();

    let result = run_loop(&mut terminal, &mut app, &event_handler, action_handler);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_handler: &Handler,
    action_handler: Actions,
) -> Result<()> {
    // Initialize preview dimensions before first draw
    if app.preview_dimensions.is_none()
        && let Ok(size) = terminal.size()
    {
        let area = Rect::new(0, 0, size.width, size.height);
        let (width, height) = render::calculate_preview_dimensions(area);
        app.set_preview_dimensions(width, height);
        action_handler.resize_agent_windows(app);
    }

    // Track selection to detect changes
    let mut last_selected = app.selected;
    // Force initial preview/diff update
    let mut needs_content_update = true;

    loop {
        // Drain all queued events first (without drawing)
        // This prevents lag when returning focus after being away,
        // since mouse events queue up while the app is unfocused
        let mut needs_tick = false;
        let mut last_resize: Option<(u16, u16)> = None;

        loop {
            match event_handler.next()? {
                Event::Tick => {
                    needs_tick = true;
                    break; // Timeout - exit inner loop
                }
                Event::Key(key) => {
                    handle_key_event(app, action_handler, key.code, key.modifiers)?;
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

        // Apply final resize if any occurred
        if let Some((width, height)) = last_resize {
            let (preview_width, preview_height) =
                render::calculate_preview_dimensions(Rect::new(0, 0, width, height));
            if app.preview_dimensions != Some((preview_width, preview_height)) {
                app.set_preview_dimensions(preview_width, preview_height);
                action_handler.resize_agent_windows(app);
            }
        }

        // Detect selection change
        if app.selected != last_selected {
            last_selected = app.selected;
            needs_content_update = true;
        }

        // Update preview/diff only on tick or selection change
        // This avoids spawning tmux/git subprocesses every frame
        if needs_tick || needs_content_update {
            let _ = action_handler.update_preview(app);
            let _ = action_handler.update_diff(app);
            needs_content_update = false;
        }

        // Draw ONCE after draining all queued events
        terminal.draw(|frame| render::render(frame, app))?;

        // Sync agent status only on tick (less frequent operation)
        if needs_tick {
            let _ = action_handler.sync_agent_status(app);
        }

        // Handle attach request - suspend TUI and attach to tmux session
        if let Some(request) = app.attach_request.take() {
            // Suspend the TUI
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;

            // Build the target: session:window_index or just session
            let target = match request.window_index {
                Some(idx) => format!("{}:{}", request.session, idx),
                None => request.session,
            };

            // Attach to the tmux session/window
            // Unset TMUX env var to allow nested tmux sessions
            let status = Command::new("tmux")
                .args(["attach-session", "-t", &target])
                .env_remove("TMUX")
                .status();

            // Restore the TUI
            enable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                EnterAlternateScreen,
                EnableMouseCapture
            )?;

            // Report any errors
            if let Err(e) = status {
                app.set_error(format!("Failed to attach: {e}"));
            }

            // Force a redraw
            terminal.clear()?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "Key event handler needs to handle all modes"
)]
fn handle_key_event(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    match &app.mode {
        Mode::Creating
        | Mode::Prompting
        | Mode::ChildPrompt
        | Mode::Broadcasting
        | Mode::ReconnectPrompt => {
            match code {
                KeyCode::Enter => {
                    let input = app.input_buffer.clone();
                    if !input.is_empty()
                        || matches!(
                            app.mode,
                            Mode::ReconnectPrompt | Mode::Prompting | Mode::ChildPrompt
                        )
                    {
                        // Remember the original mode before the action
                        let original_mode = app.mode.clone();
                        let result = match app.mode {
                            Mode::Creating => action_handler.create_agent(app, &input, None),
                            Mode::Prompting => {
                                let short_id = &Uuid::new_v4().to_string()[..8];
                                let title = format!("Agent ({short_id})");
                                let prompt = if input.is_empty() {
                                    None
                                } else {
                                    Some(input.as_str())
                                };
                                action_handler.create_agent(app, &title, prompt)
                            }
                            Mode::ChildPrompt => {
                                let prompt = if input.is_empty() {
                                    None
                                } else {
                                    Some(input.as_str())
                                };
                                action_handler.spawn_children(app, prompt)
                            }
                            Mode::Broadcasting => action_handler.broadcast_to_leaves(app, &input),
                            Mode::ReconnectPrompt => {
                                // Update the prompt in the conflict info and reconnect
                                if let Some(ref mut conflict) = app.worktree_conflict {
                                    conflict.prompt =
                                        if input.is_empty() { None } else { Some(input) };
                                }
                                action_handler.reconnect_to_worktree(app)
                            }
                            _ => Ok(()),
                        };
                        if let Err(e) = result {
                            app.set_error(format!("Failed: {e:#}"));
                            // Don't call exit_mode() - set_error already set ErrorModal mode
                            return Ok(());
                        }
                        // Only exit mode if it wasn't changed by the action
                        // (e.g., create_agent might set Confirming mode for worktree conflicts)
                        if app.mode == original_mode {
                            app.exit_mode();
                        }
                        return Ok(());
                    }
                    app.exit_mode();
                }
                KeyCode::Esc => {
                    // For ReconnectPrompt, cancel and clear conflict info
                    if matches!(app.mode, Mode::ReconnectPrompt) {
                        app.worktree_conflict = None;
                    }
                    app.exit_mode();
                }
                KeyCode::Char(c) => app.handle_char(c),
                KeyCode::Backspace => app.handle_backspace(),
                _ => {}
            }
            return Ok(());
        }
        Mode::ChildCount => match code {
            KeyCode::Enter => app.proceed_to_child_prompt(),
            KeyCode::Esc => app.exit_mode(),
            KeyCode::Up | KeyCode::Char('k') => app.increment_child_count(),
            KeyCode::Down | KeyCode::Char('j') => app.decrement_child_count(),
            _ => {}
        },
        Mode::ReviewInfo => {
            // Any key dismisses the info popup
            app.exit_mode();
        }
        Mode::ReviewChildCount => match code {
            KeyCode::Enter => app.proceed_to_branch_selector(),
            KeyCode::Esc => app.exit_mode(),
            KeyCode::Up | KeyCode::Char('k') => app.increment_child_count(),
            KeyCode::Down | KeyCode::Char('j') => app.decrement_child_count(),
            _ => {}
        },
        Mode::BranchSelector => match code {
            KeyCode::Enter => {
                if app.confirm_branch_selection()
                    && let Err(e) = action_handler.spawn_review_agents(app)
                {
                    app.set_error(format!("Failed to spawn review agents: {e:#}"));
                }
                app.exit_mode();
            }
            KeyCode::Esc => {
                app.clear_review_state();
                app.exit_mode();
            }
            KeyCode::Up | KeyCode::Char('k') => app.select_prev_branch(),
            KeyCode::Down | KeyCode::Char('j') => app.select_next_branch(),
            KeyCode::Char(c) => app.handle_branch_filter_char(c),
            KeyCode::Backspace => app.handle_branch_filter_backspace(),
            _ => {}
        },
        Mode::ConfirmPush => match code {
            KeyCode::Char('y' | 'Y') => {
                if let Err(e) = Actions::execute_push(app) {
                    app.set_error(format!("Push failed: {e:#}"));
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.clear_git_op_state();
                app.exit_mode();
            }
            _ => {}
        },
        Mode::RenameBranch => match code {
            KeyCode::Enter => {
                if app.confirm_rename_branch()
                    && let Err(e) = Actions::execute_rename(app)
                {
                    app.set_error(format!("Rename failed: {e:#}"));
                }
                // If rename failed (empty name), stay in mode
            }
            KeyCode::Esc => {
                app.clear_git_op_state();
                app.exit_mode();
            }
            KeyCode::Char(c) => app.handle_char(c),
            KeyCode::Backspace => app.handle_backspace(),
            _ => {}
        },
        Mode::ConfirmPushForPR => match code {
            KeyCode::Char('y' | 'Y') => {
                if let Err(e) = Actions::execute_push_and_open_pr(app) {
                    app.set_error(format!("Failed to push and open PR: {e:#}"));
                }
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.clear_git_op_state();
                app.exit_mode();
            }
            _ => {}
        },
        Mode::Confirming(action) => match action {
            ConfirmAction::WorktreeConflict => match code {
                KeyCode::Char('r' | 'R') => {
                    // Transition to ReconnectPrompt mode to allow editing the prompt
                    // Pre-fill input buffer with existing prompt if available
                    if let Some(ref conflict) = app.worktree_conflict {
                        app.input_buffer = conflict.prompt.clone().unwrap_or_default();
                    }
                    app.enter_mode(Mode::ReconnectPrompt);
                }
                KeyCode::Char('d' | 'D') => {
                    app.exit_mode();
                    action_handler.recreate_worktree(app)?;
                }
                KeyCode::Esc => {
                    app.worktree_conflict = None;
                    app.exit_mode();
                }
                _ => {}
            },
            _ => match code {
                KeyCode::Char('y' | 'Y') => {
                    return action_handler.handle_action(app, Action::Confirm);
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => app.exit_mode(),
                _ => {}
            },
        },
        Mode::Help => app.exit_mode(),
        Mode::ErrorModal(_) => app.dismiss_error(),
        Mode::Normal | Mode::Scrolling => {
            if let Some(action) = tenex::config::get_action(code, modifiers) {
                action_handler.handle_action(app, action)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tenex::agent::Storage;
    use tenex::app::ConfirmAction;
    use tenex::config::Config;

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
                        if wt_name.starts_with(&self.branch_prefix.replace('/', "-")) {
                            let _ = repo.find_worktree(wt_name).map(|wt| {
                                if let Some(path) = wt.path().to_str() {
                                    let _ = std::fs::remove_dir_all(path);
                                }
                                wt.prune(Some(git2::WorktreePruneOptions::new().working_tree(true)))
                            });
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
                            let _ = branch.delete();
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
        App::new(create_test_config(), Storage::default())
    }

    fn create_test_app_with_cleanup() -> (App, TestCleanup) {
        let config = create_test_config();
        let cleanup = TestCleanup::new(&config.branch_prefix);
        (App::new(config, Storage::default()), cleanup)
    }

    #[test]
    fn test_handle_key_event_normal_mode_quit() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'q' should trigger quit (since no running agents)
        handle_key_event(&mut app, handler, KeyCode::Char('q'), KeyModifiers::NONE)?;
        assert!(app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // '?' should open help
        handle_key_event(&mut app, handler, KeyCode::Char('?'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Help);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_help_mode_any_key_exits() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Help);
        handle_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'a' should enter creating mode
        handle_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Creating);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent_with_prompt()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'A' should enter prompting mode
        handle_key_event(&mut app, handler, KeyCode::Char('A'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Prompting);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_char_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        handle_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('b'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('c'), KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "abc");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        app.handle_char('a');
        app.handle_char('b');
        handle_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_escape_cancels() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.input_buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_empty_does_nothing()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        // Enter with empty input should just exit mode
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        // No agent created since input was empty
        assert_eq!(app.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_yes() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter confirming quit mode
        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

        // 'y' should confirm and quit
        handle_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE)?;
        assert!(app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_capital_y() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, handler, KeyCode::Char('Y'), KeyModifiers::NONE)?;
        assert!(app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;

        // Should still be in confirming mode
        assert!(matches!(app.mode, Mode::Confirming(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Navigation keys should work in normal mode
        handle_key_event(&mut app, handler, KeyCode::Char('j'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('k'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;

        // Should still be in normal mode (no state change visible without agents)
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_tab_switch() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        let initial_tab = app.active_tab;
        handle_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;
        assert_ne!(app.active_tab, initial_tab);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_normal_mode_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Scroll commands
        handle_key_event(&mut app, handler, KeyCode::Char('u'), KeyModifiers::CONTROL)?;
        handle_key_event(&mut app, handler, KeyCode::Char('d'), KeyModifiers::CONTROL)?;
        handle_key_event(&mut app, handler, KeyCode::Char('g'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('G'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_unknown_key_does_nothing() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Unknown key should be ignored
        handle_key_event(&mut app, handler, KeyCode::F(12), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Prompting);
        app.handle_char('t');
        app.handle_char('e');

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_cancel_action() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Escape in normal mode triggers cancel action (does nothing but works)
        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter scrolling mode (happens when scroll keys are pressed)
        app.enter_mode(Mode::Scrolling);

        // Should handle scroll keys in scrolling mode
        handle_key_event(&mut app, handler, KeyCode::Char('j'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('k'), KeyModifiers::NONE)?;
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_other_keys() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);

        // Other keys like arrows should be ignored in creating mode
        handle_key_event(&mut app, handler, KeyCode::Left, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Right, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;

        // Should still be in creating mode
        assert_eq!(app.mode, Mode::Creating);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Prompting);

        // Type some characters
        handle_key_event(&mut app, handler, KeyCode::Char('h'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('i'), KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "hi");
        assert_eq!(app.mode, Mode::Prompting);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_kill() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter confirming kill mode (no agents to kill, but mode should change)
        app.enter_mode(Mode::Confirming(ConfirmAction::Kill));

        // 'y' should trigger confirm but no agent to kill
        handle_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE)?;

        // Should exit to normal mode
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_reset() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Reset));

        // 'n' should cancel
        handle_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirming_capital_n() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

        // 'N' should also cancel
        handle_key_event(&mut app, handler, KeyCode::Char('N'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_with_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _cleanup) = create_test_app_with_cleanup();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        // Enter with input tries to create agent (will fail without git repo, but sets error)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Possible outcomes:
        // 1. Error modal (no git repo)
        // 2. Normal mode (agent created successfully)
        // 3. Confirming(WorktreeConflict) if worktree already exists
        assert!(
            matches!(app.mode, Mode::ErrorModal(_))
                || app.mode == Mode::Normal
                || matches!(app.mode, Mode::Confirming(ConfirmAction::WorktreeConflict)),
            "Expected ErrorModal, Normal, or Confirming(WorktreeConflict) mode, got {:?}",
            app.mode
        );
        // One of these should be true:
        // - Error was set (no git repo or other failure)
        // - Agent was created
        // - Worktree conflict detected (waiting for user input)
        assert!(
            app.last_error.is_some() || app.storage.len() == 1 || app.worktree_conflict.is_some()
        );
        // _cleanup will automatically remove test branches/worktrees when dropped
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_enter_with_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _cleanup) = create_test_app_with_cleanup();
        let handler = Actions::new();

        app.enter_mode(Mode::Prompting);
        app.handle_char('f');
        app.handle_char('i');
        app.handle_char('x');

        // Enter with input tries to create agent with prompt (will fail without git repo)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Possible outcomes (same as creating mode test):
        // 1. Error modal (no git repo)
        // 2. Normal mode (agent created successfully)
        // 3. Confirming(WorktreeConflict) if worktree already exists
        assert!(
            matches!(app.mode, Mode::ErrorModal(_))
                || app.mode == Mode::Normal
                || matches!(app.mode, Mode::Confirming(ConfirmAction::WorktreeConflict)),
            "Expected ErrorModal, Normal, or Confirming(WorktreeConflict) mode, got {:?}",
            app.mode
        );
        // One of these should be true:
        // - Error was set (no git repo or other failure)
        // - Agent was created
        // - Worktree conflict detected (waiting for user input)
        assert!(
            app.last_error.is_some() || app.storage.len() == 1 || app.worktree_conflict.is_some()
        );
        // _cleanup will automatically remove test branches/worktrees when dropped
        Ok(())
    }

    #[test]
    fn test_handle_key_event_creating_mode_fallthrough() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);

        // Tab key should fall through to action handling in creating mode
        handle_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;

        // Mode should remain creating (Tab doesn't exit creating mode)
        assert_eq!(app.mode, Mode::Creating);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_scrolling_mode_navigation() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Scrolling);

        // Test scrolling mode handles normal mode keybindings
        handle_key_event(&mut app, handler, KeyCode::Char('g'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('G'), KeyModifiers::NONE)?;

        // Should handle without panic
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Broadcasting);

        // Type some characters
        handle_key_event(&mut app, handler, KeyCode::Char('h'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('l'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('l'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('o'), KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "hello");
        assert_eq!(app.mode, Mode::Broadcasting);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Broadcasting);
        app.handle_char('t');
        app.handle_char('e');

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_backspace() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Broadcasting);
        app.handle_char('a');
        app.handle_char('b');

        handle_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "a");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_enter_no_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Broadcasting);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        // Enter with no agent selected should show error modal
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        assert!(app.last_error.is_some());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode_enter_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Broadcasting);

        // Enter with empty input should just exit mode
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_error_modal_dismiss() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Set an error (this enters ErrorModal mode)
        app.set_error("Test error message");
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        // Any key should dismiss the error modal
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.last_error.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_error_modal_dismiss_with_esc() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.set_error("Test error");
        assert!(matches!(app.mode, Mode::ErrorModal(_)));

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_enter() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ChildCount);

        // Enter should proceed to child prompt
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::ChildPrompt);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ChildCount);

        // Escape should exit mode
        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_count_mode_up_down() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ChildCount);
        let initial_count = app.child_count;

        // Up should increment
        handle_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count + 1);

        // Down should decrement
        handle_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count);

        // 'k' should also increment
        handle_key_event(&mut app, handler, KeyCode::Char('k'), KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count + 1);

        // 'j' should also decrement
        handle_key_event(&mut app, handler, KeyCode::Char('j'), KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count);

        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ChildPrompt);

        // Type some characters
        handle_key_event(&mut app, handler, KeyCode::Char('t'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('s'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('t'), KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "test");
        assert_eq!(app.mode, Mode::ChildPrompt);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ChildPrompt);
        app.handle_char('t');

        // Escape should exit mode
        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.input_buffer.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode_enter_no_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _cleanup) = create_test_app_with_cleanup();
        let handler = Actions::new();

        app.enter_mode(Mode::ChildPrompt);
        app.handle_char('t');
        app.handle_char('a');
        app.handle_char('s');
        app.handle_char('k');

        // Enter with input tries to spawn children (will fail without agent selected)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // On error, should show error modal; on success with no agent, exits normally
        // Could also enter WorktreeConflict mode if the branch already exists
        assert!(
            matches!(app.mode, Mode::ErrorModal(_))
                || app.mode == Mode::Normal
                || matches!(app.mode, Mode::Confirming(ConfirmAction::WorktreeConflict)),
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

        app.enter_mode(Mode::ChildCount);
        let initial_count = app.child_count;

        // Other keys should be ignored
        handle_key_event(&mut app, handler, KeyCode::Left, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Right, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;

        // Should still be in ChildCount mode with same count
        assert_eq!(app.mode, Mode::ChildCount);
        assert_eq!(app.child_count, initial_count);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_info_mode_any_key_exits()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ReviewInfo);

        // Any key should dismiss
        handle_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_info_mode_esc_exits() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ReviewInfo);

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_up_down()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ReviewChildCount);
        let initial_count = app.child_count;

        // Up should increment
        handle_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count + 1);

        // Down should decrement
        handle_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count);

        // 'k' should also increment
        handle_key_event(&mut app, handler, KeyCode::Char('k'), KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count + 1);

        // 'j' should also decrement
        handle_key_event(&mut app, handler, KeyCode::Char('j'), KeyModifiers::NONE)?;
        assert_eq!(app.child_count, initial_count);

        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_enter()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ReviewChildCount);

        // Enter should proceed to branch selector
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::BranchSelector);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode_escape()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ReviewChildCount);

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    fn create_test_branch_info(name: &str, is_remote: bool) -> tenex::git::BranchInfo {
        tenex::git::BranchInfo {
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

        app.review_branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
            create_test_branch_info("develop", false),
        ];
        app.enter_mode(Mode::BranchSelector);

        assert_eq!(app.review_branch_selected, 0);

        // Down should move to next
        handle_key_event(&mut app, handler, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.review_branch_selected, 1);

        // 'j' should also move down
        handle_key_event(&mut app, handler, KeyCode::Char('j'), KeyModifiers::NONE)?;
        assert_eq!(app.review_branch_selected, 2);

        // Up should move to previous
        handle_key_event(&mut app, handler, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.review_branch_selected, 1);

        // 'k' should also move up
        handle_key_event(&mut app, handler, KeyCode::Char('k'), KeyModifiers::NONE)?;
        assert_eq!(app.review_branch_selected, 0);

        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_filter() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.review_branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("feature", false),
        ];
        app.enter_mode(Mode::BranchSelector);

        // Type characters for filter
        handle_key_event(&mut app, handler, KeyCode::Char('m'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('a'), KeyModifiers::NONE)?;

        assert_eq!(app.review_branch_filter, "ma");
        assert_eq!(app.mode, Mode::BranchSelector);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_backspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.review_branches = vec![create_test_branch_info("main", false)];
        app.review_branch_filter = "main".to_string();
        app.enter_mode(Mode::BranchSelector);

        handle_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;
        assert_eq!(app.review_branch_filter, "mai");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_escape() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::BranchSelector);
        app.review_branches = vec![create_test_branch_info("main", false)];
        app.review_branch_filter = "test".to_string();

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        // State should be cleared on escape
        assert!(app.review_branches.is_empty());
        assert!(app.review_branch_filter.is_empty());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode_enter() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.review_branches = vec![
            create_test_branch_info("main", false),
            create_test_branch_info("develop", false),
        ];
        app.review_branch_selected = 1;
        app.spawning_under = Some(uuid::Uuid::new_v4());
        app.enter_mode(Mode::BranchSelector);

        // Enter tries to spawn review agents (will fail without proper agent setup)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Should have set review_base_branch before attempting spawn
        assert!(
            app.review_base_branch.is_some() || matches!(app.mode, Mode::ErrorModal(_)),
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

        app.review_branches = vec![]; // Empty list
        app.enter_mode(Mode::BranchSelector);

        // Enter with empty list exits mode but doesn't set base branch
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.review_base_branch.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_review_swarm_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Press 'R' with no agent selected
        handle_key_event(&mut app, handler, KeyCode::Char('R'), KeyModifiers::NONE)?;

        // Should show ReviewInfo mode
        assert_eq!(app.mode, Mode::ReviewInfo);
        Ok(())
    }

    // === Git Operations Key Event Tests ===

    #[test]
    fn test_handle_key_event_confirm_push_mode_no() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPush);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();

        // 'n' should cancel and exit
        handle_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.git_op_agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_mode_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPush);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();

        // Escape should cancel and exit
        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.git_op_agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_yes() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPush);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();

        // 'y' should try to execute push (will fail, no agent in storage)
        handle_key_event(&mut app, handler, KeyCode::Char('Y'), KeyModifiers::NONE)?;

        // Should show error (no agent in storage)
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_input() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::RenameBranch);
        app.git_op_branch_name = "feature/old".to_string();
        app.input_buffer = "feature/old".to_string();

        // Type some characters
        handle_key_event(&mut app, handler, KeyCode::Char('-'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('e'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Char('w'), KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "feature/old-new");
        assert_eq!(app.mode, Mode::RenameBranch);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_backspace() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::RenameBranch);
        app.input_buffer = "feature/test".to_string();

        handle_key_event(&mut app, handler, KeyCode::Backspace, KeyModifiers::NONE)?;

        assert_eq!(app.input_buffer, "feature/tes");
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_escape() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::RenameBranch);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.input_buffer = "feature/test".to_string();

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.git_op_agent_id.is_none()); // State cleared
        Ok(())
    }

    #[test]
    fn test_handle_key_event_rename_branch_enter() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::RenameBranch);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_original_branch = "feature/old".to_string();
        app.git_op_branch_name = "feature/old".to_string();
        app.input_buffer = "feature/new".to_string();

        // Enter tries to confirm rename and execute (will fail without agent)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Branch name should have been updated before failing
        assert_eq!(app.git_op_branch_name, "feature/new");
        // Should show error (no agent in storage)
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_no() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPushForPR);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();

        // 'n' should cancel and exit
        handle_key_event(&mut app, handler, KeyCode::Char('n'), KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.git_op_agent_id.is_none()); // State cleared
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_escape() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPushForPR);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());

        handle_key_event(&mut app, handler, KeyCode::Esc, KeyModifiers::NONE)?;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.git_op_agent_id.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_yes() -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPushForPR);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();
        app.git_op_base_branch = "main".to_string();

        // 'y' should try to push and open PR (will fail, no agent in storage)
        handle_key_event(&mut app, handler, KeyCode::Char('y'), KeyModifiers::NONE)?;

        // Should show error (no agent in storage)
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_key_event_confirm_push_other_keys_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::ConfirmPush);
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());

        // Other keys should be ignored
        handle_key_event(&mut app, handler, KeyCode::Char('x'), KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Tab, KeyModifiers::NONE)?;
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // Should still be in ConfirmPush mode
        assert_eq!(app.mode, Mode::ConfirmPush);
        Ok(())
    }
}
