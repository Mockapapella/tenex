//! Terminal User Interface for Muster

mod render;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use muster::app::{Actions, App, Event, Handler, Mode};
use muster::config::Action;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

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

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

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
                    if app.mode == Mode::Creating {
                        let _ = action_handler.create_agent(app, &input, None);
                    } else {
                        let _ = action_handler.create_agent(app, "prompted-agent", Some(&input));
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
