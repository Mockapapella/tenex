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
mod tests {}
