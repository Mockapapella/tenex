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

    let result = run_loop(&mut terminal, &mut app, &event_handler, &action_handler);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "consistent reference pattern for handlers"
)]
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    event_handler: &Handler,
    action_handler: &Actions,
) -> Result<()> {
    let mut tick_count = 0;

    loop {
        terminal.draw(|frame| render::render(frame, app))?;

        match event_handler.next()? {
            Event::Tick => {
                tick_count += 1;
                if tick_count % 10 == 0 {
                    let _ = action_handler.update_preview(app);
                    let _ = action_handler.update_diff(app);
                    let _ = action_handler.sync_agent_status(app);
                }
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

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "consistent reference pattern for handlers"
)]
fn handle_key_event(
    app: &mut App,
    action_handler: &Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    match &app.mode {
        Mode::Creating | Mode::Prompting => match code {
            KeyCode::Enter => {
                let input = app.input_buffer.clone();
                if !input.is_empty() {
                    let result = if app.mode == Mode::Creating {
                        action_handler.create_agent(app, &input, None)
                    } else {
                        action_handler.create_agent(app, "prompted-agent", Some(&input))
                    };
                    if let Err(e) = result {
                        app.set_error(format!("Failed to create agent: {e:#}"));
                    }
                }
                app.exit_mode();
                return Ok(());
            }
            KeyCode::Esc => {
                app.exit_mode();
                return Ok(());
            }
            KeyCode::Char(c) => {
                app.handle_char(c);
                return Ok(());
            }
            KeyCode::Backspace => {
                app.handle_backspace();
                return Ok(());
            }
            _ => {}
        },
        Mode::Confirming(_) => match code {
            KeyCode::Char('y' | 'Y') => {
                action_handler.handle_action(app, Action::Confirm)?;
                return Ok(());
            }
            KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                app.exit_mode();
                return Ok(());
            }
            _ => return Ok(()),
        },
        Mode::Help => {
            app.exit_mode();
            return Ok(());
        }
        Mode::Normal | Mode::Scrolling => {}
    }

    if let Some(action) = app.config.keys.get_action(code, modifiers) {
        action_handler.handle_action(app, action)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use muster::agent::Storage;
    use muster::config::Config;
    use muster::app::ConfirmAction;

    fn create_test_app() -> App {
        App::new(Config::default(), Storage::default())
    }

    #[test]
    fn test_handle_key_event_normal_mode_quit() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'q' should trigger quit (since no running agents)
        handle_key_event(&mut app, &handler, KeyCode::Char('q'), KeyModifiers::NONE).unwrap();
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // '?' should open help
        handle_key_event(&mut app, &handler, KeyCode::Char('?'), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Help);
    }

    #[test]
    fn test_handle_key_event_help_mode_any_key_exits() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Help);
        handle_key_event(&mut app, &handler, KeyCode::Char('x'), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'n' should enter creating mode
        handle_key_event(&mut app, &handler, KeyCode::Char('n'), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Creating);
    }

    #[test]
    fn test_handle_key_event_normal_mode_new_agent_with_prompt() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // 'N' should enter prompting mode
        handle_key_event(&mut app, &handler, KeyCode::Char('N'), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Prompting);
    }

    #[test]
    fn test_handle_key_event_creating_mode_char_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        handle_key_event(&mut app, &handler, KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('b'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('c'), KeyModifiers::NONE).unwrap();

        assert_eq!(app.input_buffer, "abc");
    }

    #[test]
    fn test_handle_key_event_creating_mode_backspace() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        app.handle_char('a');
        app.handle_char('b');
        handle_key_event(&mut app, &handler, KeyCode::Backspace, KeyModifiers::NONE).unwrap();

        assert_eq!(app.input_buffer, "a");
    }

    #[test]
    fn test_handle_key_event_creating_mode_escape_cancels() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        app.handle_char('t');
        app.handle_char('e');
        app.handle_char('s');
        app.handle_char('t');

        handle_key_event(&mut app, &handler, KeyCode::Esc, KeyModifiers::NONE).unwrap();

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.input_buffer.is_empty());
    }

    #[test]
    fn test_handle_key_event_creating_mode_enter_empty_does_nothing() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);
        // Enter with empty input should just exit mode
        handle_key_event(&mut app, &handler, KeyCode::Enter, KeyModifiers::NONE).unwrap();

        assert_eq!(app.mode, Mode::Normal);
        // No agent created since input was empty
        assert_eq!(app.storage.len(), 0);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_yes() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter confirming quit mode
        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

        // 'y' should confirm and quit
        handle_key_event(&mut app, &handler, KeyCode::Char('y'), KeyModifiers::NONE).unwrap();
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_capital_y() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, &handler, KeyCode::Char('Y'), KeyModifiers::NONE).unwrap();
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_no() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, &handler, KeyCode::Char('n'), KeyModifiers::NONE).unwrap();

        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, &handler, KeyCode::Esc, KeyModifiers::NONE).unwrap();

        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_handle_key_event_confirming_mode_other_key_ignored() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
        handle_key_event(&mut app, &handler, KeyCode::Char('x'), KeyModifiers::NONE).unwrap();

        // Should still be in confirming mode
        assert!(matches!(app.mode, Mode::Confirming(_)));
    }

    #[test]
    fn test_handle_key_event_normal_mode_navigation() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Navigation keys should work in normal mode
        handle_key_event(&mut app, &handler, KeyCode::Char('j'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('k'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Down, KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Up, KeyModifiers::NONE).unwrap();

        // Should still be in normal mode (no state change visible without agents)
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_normal_mode_tab_switch() {
        let mut app = create_test_app();
        let handler = Actions::new();

        let initial_tab = app.active_tab;
        handle_key_event(&mut app, &handler, KeyCode::Tab, KeyModifiers::NONE).unwrap();
        assert_ne!(app.active_tab, initial_tab);
    }

    #[test]
    fn test_handle_key_event_normal_mode_scroll() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Scroll commands
        handle_key_event(&mut app, &handler, KeyCode::Char('u'), KeyModifiers::CONTROL).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('d'), KeyModifiers::CONTROL).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('g'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('G'), KeyModifiers::NONE).unwrap();

        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_unknown_key_does_nothing() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Unknown key should be ignored
        handle_key_event(&mut app, &handler, KeyCode::F(12), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_handle_key_event_prompting_mode_escape() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Prompting);
        app.handle_char('t');
        app.handle_char('e');

        handle_key_event(&mut app, &handler, KeyCode::Esc, KeyModifiers::NONE).unwrap();

        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_cancel_action() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Escape in normal mode triggers cancel action (does nothing but works)
        handle_key_event(&mut app, &handler, KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter scrolling mode (happens when scroll keys are pressed)
        app.enter_mode(Mode::Scrolling);

        // Should handle scroll keys in scrolling mode
        handle_key_event(&mut app, &handler, KeyCode::Char('j'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('k'), KeyModifiers::NONE).unwrap();
    }

    #[test]
    fn test_handle_key_event_creating_mode_other_keys() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Creating);

        // Other keys like arrows should be ignored in creating mode
        handle_key_event(&mut app, &handler, KeyCode::Left, KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Right, KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Up, KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Down, KeyModifiers::NONE).unwrap();

        // Should still be in creating mode
        assert_eq!(app.mode, Mode::Creating);
    }

    #[test]
    fn test_handle_key_event_prompting_mode_input() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Prompting);

        // Type some characters
        handle_key_event(&mut app, &handler, KeyCode::Char('h'), KeyModifiers::NONE).unwrap();
        handle_key_event(&mut app, &handler, KeyCode::Char('i'), KeyModifiers::NONE).unwrap();

        assert_eq!(app.input_buffer, "hi");
        assert_eq!(app.mode, Mode::Prompting);
    }

    #[test]
    fn test_handle_key_event_confirming_kill() {
        let mut app = create_test_app();
        let handler = Actions::new();

        // Enter confirming kill mode (no agents to kill, but mode should change)
        app.enter_mode(Mode::Confirming(ConfirmAction::Kill));

        // 'y' should trigger confirm but no agent to kill
        handle_key_event(&mut app, &handler, KeyCode::Char('y'), KeyModifiers::NONE).unwrap();

        // Should exit to normal mode
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_confirming_reset() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Reset));

        // 'n' should cancel
        handle_key_event(&mut app, &handler, KeyCode::Char('n'), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_key_event_confirming_capital_n() {
        let mut app = create_test_app();
        let handler = Actions::new();

        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

        // 'N' should also cancel
        handle_key_event(&mut app, &handler, KeyCode::Char('N'), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }
}
