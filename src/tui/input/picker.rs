//! Picker mode key handling
//!
//! Handles key events for modes that involve picking/selecting:
//! - `ChildCount` (selecting number of child agents)
//! - `ReviewChildCount` (selecting number of review agents)
//! - `ReviewInfo` (info popup before review)
//! - `BranchSelector` (selecting a branch)

use crate::app::{Actions, App};
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `ChildCount` mode
pub fn handle_child_count_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => app.proceed_to_child_prompt(),
        KeyCode::Esc => app.exit_mode(),
        KeyCode::Up => app.increment_child_count(),
        KeyCode::Down => app.decrement_child_count(),
        _ => {}
    }
}

/// Handle key events in `ReviewChildCount` mode
pub fn handle_review_child_count_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => app.proceed_to_branch_selector(),
        KeyCode::Esc => app.exit_mode(),
        KeyCode::Up => app.increment_child_count(),
        KeyCode::Down => app.decrement_child_count(),
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
        KeyCode::Up => app.select_prev_branch(),
        KeyCode::Down => app.select_next_branch(),
        KeyCode::Char(c) => app.handle_branch_filter_char(c),
        KeyCode::Backspace => app.handle_branch_filter_backspace(),
        _ => {}
    }
}

/// Handle key events in `RebaseBranchSelector` mode
pub fn handle_rebase_branch_selector_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            if app.confirm_rebase_merge_branch()
                && let Err(e) = Actions::execute_rebase(app)
            {
                app.set_error(format!("Rebase failed: {e:#}"));
            }
        }
        KeyCode::Esc => {
            app.clear_git_op_state();
            app.clear_review_state();
            app.exit_mode();
        }
        KeyCode::Up => app.select_prev_branch(),
        KeyCode::Down => app.select_next_branch(),
        KeyCode::Char(c) => app.handle_branch_filter_char(c),
        KeyCode::Backspace => app.handle_branch_filter_backspace(),
        _ => {}
    }
}

/// Handle key events in `MergeBranchSelector` mode
pub fn handle_merge_branch_selector_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            if app.confirm_rebase_merge_branch()
                && let Err(e) = Actions::execute_merge(app)
            {
                app.set_error(format!("Merge failed: {e:#}"));
            }
        }
        KeyCode::Esc => {
            app.clear_git_op_state();
            app.clear_review_state();
            app.exit_mode();
        }
        KeyCode::Up => app.select_prev_branch(),
        KeyCode::Down => app.select_next_branch(),
        KeyCode::Char(c) => app.handle_branch_filter_char(c),
        KeyCode::Backspace => app.handle_branch_filter_backspace(),
        _ => {}
    }
}

/// Handle key events in `SuccessModal` mode (any key dismisses)
pub fn handle_success_modal_mode(app: &mut App) {
    app.dismiss_success();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{
        BranchPickerKind, CountPickerKind, Mode, OverlayMode, Settings, TextInputKind,
    };
    use crate::config::Config;
    use crate::git::BranchInfo;
    use ratatui::crossterm::event::KeyCode;
    use tempfile::NamedTempFile;

    fn count_picker_mode(kind: CountPickerKind) -> Mode {
        Mode::Overlay(OverlayMode::CountPicker(kind))
    }

    fn branch_picker_mode(kind: BranchPickerKind) -> Mode {
        Mode::Overlay(OverlayMode::BranchPicker(kind))
    }

    fn text_input_mode(kind: TextInputKind) -> Mode {
        Mode::Overlay(OverlayMode::TextInput(kind))
    }

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    fn test_branches() -> Vec<BranchInfo> {
        vec![
            BranchInfo {
                name: "main".to_string(),
                full_name: "refs/heads/main".to_string(),
                is_remote: false,
                remote: None,
                last_commit_time: None,
            },
            BranchInfo {
                name: "dev".to_string(),
                full_name: "refs/heads/dev".to_string(),
                is_remote: false,
                remote: None,
                last_commit_time: None,
            },
        ]
    }

    // ========== ChildCount mode tests ==========

    #[test]
    fn test_handle_child_count_mode_up() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.spawn.child_count = 1;
        handle_child_count_mode(&mut app, KeyCode::Up);
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_down() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.spawn.child_count = 2;
        handle_child_count_mode(&mut app, KeyCode::Down);
        assert_eq!(app.spawn.child_count, 1);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = count_picker_mode(CountPickerKind::ChildCount);
        handle_child_count_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_enter() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = count_picker_mode(CountPickerKind::ChildCount);
        app.spawn.child_count = 2;
        handle_child_count_mode(&mut app, KeyCode::Enter);
        // Should proceed to ChildPrompt mode
        assert_eq!(app.mode, text_input_mode(TextInputKind::ChildPrompt));
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = count_picker_mode(CountPickerKind::ChildCount);
        app.spawn.child_count = 2;
        handle_child_count_mode(&mut app, KeyCode::Tab);
        assert_eq!(app.mode, count_picker_mode(CountPickerKind::ChildCount));
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    // ========== ReviewChildCount mode tests ==========

    #[test]
    fn test_handle_review_child_count_mode_up() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.spawn.child_count = 1;
        handle_review_child_count_mode(&mut app, KeyCode::Up);
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_down() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.spawn.child_count = 2;
        handle_review_child_count_mode(&mut app, KeyCode::Down);
        assert_eq!(app.spawn.child_count, 1);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = count_picker_mode(CountPickerKind::ReviewChildCount);
        handle_review_child_count_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_enter() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = count_picker_mode(CountPickerKind::ReviewChildCount);
        handle_review_child_count_mode(&mut app, KeyCode::Enter);
        // Should proceed to BranchSelector mode
        assert_eq!(
            app.mode,
            branch_picker_mode(BranchPickerKind::ReviewBaseBranch)
        );
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = count_picker_mode(CountPickerKind::ReviewChildCount);
        app.spawn.child_count = 2;
        handle_review_child_count_mode(&mut app, KeyCode::Tab);
        assert_eq!(
            app.mode,
            count_picker_mode(CountPickerKind::ReviewChildCount)
        );
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    // ========== ReviewInfo mode tests ==========

    #[test]
    fn test_handle_review_info_mode() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::ReviewInfo);
        handle_review_info_mode(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    // ========== BranchSelector mode tests ==========

    #[test]
    fn test_handle_branch_selector_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = branch_picker_mode(BranchPickerKind::ReviewBaseBranch);
        let action_handler = Actions::new();
        handle_branch_selector_mode(&mut app, action_handler, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_filter_char() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        let action_handler = Actions::new();
        handle_branch_selector_mode(&mut app, action_handler, KeyCode::Char('m'));
        assert_eq!(app.review.filter, "m");
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_filter_backspace() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.filter = "ma".to_string();
        let action_handler = Actions::new();
        handle_branch_selector_mode(&mut app, action_handler, KeyCode::Backspace);
        assert_eq!(app.review.filter, "m");
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_navigation_up() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.branches = test_branches();
        app.review.selected = 1;
        let action_handler = Actions::new();
        handle_branch_selector_mode(&mut app, action_handler, KeyCode::Up);
        assert_eq!(app.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_navigation_down() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.branches = test_branches();
        app.review.selected = 0;
        let action_handler = Actions::new();
        handle_branch_selector_mode(&mut app, action_handler, KeyCode::Down);
        assert_eq!(app.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = branch_picker_mode(BranchPickerKind::ReviewBaseBranch);
        let action_handler = Actions::new();
        handle_branch_selector_mode(&mut app, action_handler, KeyCode::Tab);
        assert_eq!(
            app.mode,
            branch_picker_mode(BranchPickerKind::ReviewBaseBranch)
        );
        Ok(())
    }

    // ========== RebaseBranchSelector mode tests ==========

    #[test]
    fn test_handle_rebase_branch_selector_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = branch_picker_mode(BranchPickerKind::RebaseTargetBranch);
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_filter() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Char('m'));
        assert_eq!(app.review.filter, "m");

        handle_rebase_branch_selector_mode(&mut app, KeyCode::Backspace);
        assert_eq!(app.review.filter, "");
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_navigation_up() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.branches = test_branches();
        app.review.selected = 1;
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Up);
        assert_eq!(app.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_navigation_down() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.branches = test_branches();
        app.review.selected = 0;
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Down);
        assert_eq!(app.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = branch_picker_mode(BranchPickerKind::RebaseTargetBranch);
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Tab);
        assert_eq!(
            app.mode,
            branch_picker_mode(BranchPickerKind::RebaseTargetBranch)
        );
        Ok(())
    }

    // ========== MergeBranchSelector mode tests ==========

    #[test]
    fn test_handle_merge_branch_selector_mode_esc() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = branch_picker_mode(BranchPickerKind::MergeFromBranch);
        handle_merge_branch_selector_mode(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_filter() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        handle_merge_branch_selector_mode(&mut app, KeyCode::Char('f'));
        assert_eq!(app.review.filter, "f");

        handle_merge_branch_selector_mode(&mut app, KeyCode::Backspace);
        assert_eq!(app.review.filter, "");
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_navigation_up() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.branches = test_branches();
        app.review.selected = 1;
        handle_merge_branch_selector_mode(&mut app, KeyCode::Up);
        assert_eq!(app.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_navigation_down() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.review.branches = test_branches();
        app.review.selected = 0;
        handle_merge_branch_selector_mode(&mut app, KeyCode::Down);
        assert_eq!(app.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_other_key_ignored() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = branch_picker_mode(BranchPickerKind::MergeFromBranch);
        handle_merge_branch_selector_mode(&mut app, KeyCode::Tab);
        assert_eq!(
            app.mode,
            branch_picker_mode(BranchPickerKind::MergeFromBranch)
        );
        Ok(())
    }

    // ========== SuccessModal mode tests ==========

    #[test]
    fn test_handle_success_modal_mode() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::Overlay(OverlayMode::Success("Test".to_string()));
        handle_success_modal_mode(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }
}
