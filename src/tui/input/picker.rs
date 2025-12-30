//! Picker mode key handling
//!
//! Handles key events for modes that involve picking/selecting:
//! - `ChildCount` (selecting number of child agents)
//! - `ReviewChildCount` (selecting number of review agents)
//! - `ReviewInfo` (info popup before review)
//! - `BranchSelector` (selecting a branch)

use crate::app::{Actions, App};
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `ChildCount` mode
pub fn handle_child_count_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_child_count_mode(app, action_handler, code)
}

/// Handle key events in `ReviewChildCount` mode
pub fn handle_review_child_count_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_review_child_count_mode(app, action_handler, code)
}

/// Handle key events in `ReviewInfo` mode (any key dismisses)
pub fn handle_review_info_mode(app: &mut App, action_handler: Actions) -> Result<()> {
    crate::action::dispatch_review_info_mode(app, action_handler)
}

/// Handle key events in `BranchSelector` mode
pub fn handle_branch_selector_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_branch_selector_mode(app, action_handler, code)
}

/// Handle key events in `RebaseBranchSelector` mode
pub fn handle_rebase_branch_selector_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_rebase_branch_selector_mode(app, action_handler, code)
}

/// Handle key events in `MergeBranchSelector` mode
pub fn handle_merge_branch_selector_mode(
    app: &mut App,
    action_handler: Actions,
    code: KeyCode,
) -> Result<()> {
    crate::action::dispatch_merge_branch_selector_mode(app, action_handler, code)
}

/// Handle key events in `SuccessModal` mode (any key dismisses)
pub fn handle_success_modal_mode(app: &mut App) {
    app.dismiss_success();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::{Mode, Settings};
    use crate::config::Config;
    use crate::git::BranchInfo;
    use ratatui::crossterm::event::KeyCode;
    use tempfile::NamedTempFile;

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
    fn test_handle_child_count_mode_up() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildCount;
        app.spawn.child_count = 1;
        handle_child_count_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_down() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildCount;
        app.spawn.child_count = 2;
        handle_child_count_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.spawn.child_count, 1);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildCount;
        handle_child_count_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_enter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildCount;
        app.spawn.child_count = 2;
        handle_child_count_mode(&mut app, Actions::new(), KeyCode::Enter)?;
        // Should proceed to ChildPrompt mode
        assert_eq!(app.mode, Mode::ChildPrompt);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ChildCount;
        app.spawn.child_count = 2;
        handle_child_count_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.mode, Mode::ChildCount);
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    // ========== ReviewChildCount mode tests ==========

    #[test]
    fn test_handle_review_child_count_mode_up() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewChildCount;
        app.spawn.child_count = 1;
        handle_review_child_count_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_down() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewChildCount;
        app.spawn.child_count = 2;
        handle_review_child_count_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.spawn.child_count, 1);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewChildCount;
        handle_review_child_count_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_enter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewChildCount;
        handle_review_child_count_mode(&mut app, Actions::new(), KeyCode::Enter)?;
        // Should proceed to BranchSelector mode
        assert_eq!(app.mode, Mode::BranchSelector);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewChildCount;
        app.spawn.child_count = 2;
        handle_review_child_count_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.mode, Mode::ReviewChildCount);
        assert_eq!(app.spawn.child_count, 2);
        Ok(())
    }

    // ========== ReviewInfo mode tests ==========

    #[test]
    fn test_handle_review_info_mode() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::ReviewInfo;
        handle_review_info_mode(&mut app, Actions::new())?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    // ========== BranchSelector mode tests ==========

    #[test]
    fn test_handle_branch_selector_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        handle_branch_selector_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_filter_char() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        handle_branch_selector_mode(&mut app, Actions::new(), KeyCode::Char('m'))?;
        assert_eq!(app.review.filter, "m");
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_filter_backspace() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        app.review.filter = "ma".to_string();
        handle_branch_selector_mode(&mut app, Actions::new(), KeyCode::Backspace)?;
        assert_eq!(app.review.filter, "m");
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_navigation_up() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        app.review.branches = test_branches();
        app.review.selected = 1;
        handle_branch_selector_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_navigation_down() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        app.review.branches = test_branches();
        app.review.selected = 0;
        handle_branch_selector_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::BranchSelector;
        handle_branch_selector_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.mode, Mode::BranchSelector);
        Ok(())
    }

    // ========== RebaseBranchSelector mode tests ==========

    #[test]
    fn test_handle_rebase_branch_selector_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RebaseBranchSelector;
        handle_rebase_branch_selector_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_filter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RebaseBranchSelector;
        handle_rebase_branch_selector_mode(&mut app, Actions::new(), KeyCode::Char('m'))?;
        assert_eq!(app.review.filter, "m");

        handle_rebase_branch_selector_mode(&mut app, Actions::new(), KeyCode::Backspace)?;
        assert_eq!(app.review.filter, "");
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_navigation_up()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RebaseBranchSelector;
        app.review.branches = test_branches();
        app.review.selected = 1;
        handle_rebase_branch_selector_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_navigation_down()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RebaseBranchSelector;
        app.review.branches = test_branches();
        app.review.selected = 0;
        handle_rebase_branch_selector_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::RebaseBranchSelector;
        handle_rebase_branch_selector_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.mode, Mode::RebaseBranchSelector);
        Ok(())
    }

    // ========== MergeBranchSelector mode tests ==========

    #[test]
    fn test_handle_merge_branch_selector_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::MergeBranchSelector;
        handle_merge_branch_selector_mode(&mut app, Actions::new(), KeyCode::Esc)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_filter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::MergeBranchSelector;
        handle_merge_branch_selector_mode(&mut app, Actions::new(), KeyCode::Char('f'))?;
        assert_eq!(app.review.filter, "f");

        handle_merge_branch_selector_mode(&mut app, Actions::new(), KeyCode::Backspace)?;
        assert_eq!(app.review.filter, "");
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_navigation_up()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::MergeBranchSelector;
        app.review.branches = test_branches();
        app.review.selected = 1;
        handle_merge_branch_selector_mode(&mut app, Actions::new(), KeyCode::Up)?;
        assert_eq!(app.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_navigation_down()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::MergeBranchSelector;
        app.review.branches = test_branches();
        app.review.selected = 0;
        handle_merge_branch_selector_mode(&mut app, Actions::new(), KeyCode::Down)?;
        assert_eq!(app.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::MergeBranchSelector;
        handle_merge_branch_selector_mode(&mut app, Actions::new(), KeyCode::Tab)?;
        assert_eq!(app.mode, Mode::MergeBranchSelector);
        Ok(())
    }

    // ========== SuccessModal mode tests ==========

    #[test]
    fn test_handle_success_modal_mode() -> Result<(), std::io::Error> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = Mode::SuccessModal("Test".to_string());
        handle_success_modal_mode(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }
}
