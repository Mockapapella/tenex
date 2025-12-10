//! Picker mode key handling
//!
//! Handles key events for modes that involve picking/selecting:
//! - `ChildCount` (selecting number of child agents)
//! - `ReviewChildCount` (selecting number of review agents)
//! - `ReviewInfo` (info popup before review)
//! - `BranchSelector` (selecting a branch)

use ratatui::crossterm::event::KeyCode;
use tenex::app::{Actions, App};

/// Handle key events in `ChildCount` mode
pub fn handle_child_count_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => app.proceed_to_child_prompt(),
        KeyCode::Esc => app.exit_mode(),
        KeyCode::Up | KeyCode::Char('k') => app.increment_child_count(),
        KeyCode::Down | KeyCode::Char('j') => app.decrement_child_count(),
        _ => {}
    }
}

/// Handle key events in `ReviewChildCount` mode
pub fn handle_review_child_count_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => app.proceed_to_branch_selector(),
        KeyCode::Esc => app.exit_mode(),
        KeyCode::Up | KeyCode::Char('k') => app.increment_child_count(),
        KeyCode::Down | KeyCode::Char('j') => app.decrement_child_count(),
        _ => {}
    }
}

/// Handle key events in `ReviewInfo` mode (any key dismisses)
pub fn handle_review_info_mode(app: &mut App) {
    app.exit_mode();
}

/// Handle key events in `BranchSelector` mode
pub fn handle_branch_selector_mode(app: &mut App, action_handler: Actions, code: KeyCode) {
    match code {
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
    }
}
