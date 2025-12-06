//! Terminal User Interface for Muster

mod render;

use anyhow::Result;
use muster::app::{Actions, App, Event, Handler, Mode};
use muster::config::Action;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
};
use std::io;
use std::process::Command;

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
    loop {
        terminal.draw(|frame| render::render(frame, app))?;

        match event_handler.next()? {
            Event::Tick => {
                let _ = action_handler.update_preview(app);
                let _ = action_handler.update_diff(app);
                let _ = action_handler.sync_agent_status(app);
            }
            Event::Key(key) => {
                handle_key_event(app, action_handler, key.code, key.modifiers)?;
            }
            Event::Mouse(_mouse) => {}
            Event::Resize(_, _) => {}
        }

        // Handle attach request - suspend TUI and attach to tmux session
        if let Some(session) = app.attach_session.take() {
            // Suspend the TUI
            disable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            )?;
            terminal.show_cursor()?;

            // Attach to the tmux session
            // Unset TMUX env var to allow nested tmux sessions
            let status = Command::new("tmux")
                .args(["attach-session", "-t", &session])
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

fn handle_key_event(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    match &app.mode {
        Mode::Creating | Mode::Prompting | Mode::ChildPrompt | Mode::Broadcasting => {
            match code {
                KeyCode::Enter => {
                    let input = app.input_buffer.clone();
                    if !input.is_empty() {
                        let result = match app.mode {
                            Mode::Creating => action_handler.create_agent(app, &input, None),
                            Mode::Prompting => {
                                action_handler.create_agent(app, "prompted-agent", Some(&input))
                            }
                            Mode::ChildPrompt => action_handler.spawn_children(app, &input),
                            Mode::Broadcasting => action_handler.broadcast_to_leaves(app, &input),
                            _ => Ok(()),
                        };
                        if let Err(e) = result {
                            app.set_error(format!("Failed: {e:#}"));
                            // Don't call exit_mode() - set_error already set ErrorModal mode
                            return Ok(());
                        }
                    }
                    app.exit_mode();
                }
                KeyCode::Esc => app.exit_mode(),
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
        Mode::Confirming(_) => match code {
            KeyCode::Char('y' | 'Y') => return action_handler.handle_action(app, Action::Confirm),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => app.exit_mode(),
            _ => {}
        },
        Mode::Help => app.exit_mode(),
        Mode::ErrorModal(_) => app.dismiss_error(),
        Mode::Normal | Mode::Scrolling => {
            if let Some(action) = muster::config::get_action(code, modifiers) {
                action_handler.handle_action(app, action)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use muster::agent::Storage;
    use muster::app::ConfirmAction;
    use muster::config::Config;

    fn create_test_app() -> App {
        App::new(Config::default(), Storage::default())
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
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        // Enter with input tries to create agent (will fail without git repo, but sets error)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // On error, should show error modal; on success, should be in normal mode
        assert!(
            matches!(app.mode, Mode::ErrorModal(_)) || app.mode == Mode::Normal,
            "Expected ErrorModal or Normal mode"
        );
        // Error should be set since we're not in a git repo, or agent was created
        assert!(app.last_error.is_some() || app.storage.len() == 1);
        Ok(())
    }

    #[test]
    fn test_handle_key_event_prompting_mode_enter_with_input()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Prompting);
        app.handle_char('f');
        app.handle_char('i');
        app.handle_char('x');

        // Enter with input tries to create agent with prompt (will fail without git repo)
        handle_key_event(&mut app, handler, KeyCode::Enter, KeyModifiers::NONE)?;

        // On error, should show error modal; on success, should be in normal mode
        assert!(
            matches!(app.mode, Mode::ErrorModal(_)) || app.mode == Mode::Normal,
            "Expected ErrorModal or Normal mode"
        );
        // Error should be set since we're not in a git repo, or agent was created
        assert!(app.last_error.is_some() || app.storage.len() == 1);
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
}
