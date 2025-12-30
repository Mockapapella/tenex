//! Picker mode key handling
//!
//! Handles key events for modes that involve picking/selecting:
//! - `ChildCount` (selecting number of child agents)
//! - `ReviewChildCount` (selecting number of review agents)
//! - `ReviewInfo` (info popup before review)
//! - `BranchSelector` (selecting a branch)

use crate::app::App;
use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

/// Handle key events in `ChildCount` mode
pub fn handle_child_count_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_child_count_mode(app, code)
}

/// Handle key events in `ReviewChildCount` mode
pub fn handle_review_child_count_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_review_child_count_mode(app, code)
}

/// Handle key events in `ReviewInfo` mode (any key dismisses)
pub fn handle_review_info_mode(app: &mut App) -> Result<()> {
    crate::action::dispatch_review_info_mode(app)
}

/// Handle key events in `BranchSelector` mode
pub fn handle_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_branch_selector_mode(app, code)
}

/// Handle key events in `RebaseBranchSelector` mode
pub fn handle_rebase_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_rebase_branch_selector_mode(app, code)
}

/// Handle key events in `MergeBranchSelector` mode
pub fn handle_merge_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    crate::action::dispatch_merge_branch_selector_mode(app, code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::git::BranchInfo;
    use crate::state::{
        AppMode, BranchSelectorMode, ChildCountMode, ChildPromptMode, MergeBranchSelectorMode,
        RebaseBranchSelectorMode, ReviewChildCountMode, ReviewInfoMode,
    };
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
        app.apply_mode(ChildCountMode.into());
        app.data.spawn.child_count = 1;
        handle_child_count_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.spawn.child_count, 2);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_down() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ChildCountMode.into());
        app.data.spawn.child_count = 2;
        handle_child_count_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.spawn.child_count, 1);
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ChildCountMode.into());
        handle_child_count_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_enter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ChildCountMode.into());
        app.data.spawn.child_count = 2;
        handle_child_count_mode(&mut app, KeyCode::Enter)?;
        // Should proceed to ChildPrompt mode
        assert_eq!(app.mode, ChildPromptMode.into());
        Ok(())
    }

    #[test]
    fn test_handle_child_count_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ChildCountMode.into());
        app.data.spawn.child_count = 2;
        handle_child_count_mode(&mut app, KeyCode::Tab)?;
        assert_eq!(app.mode, ChildCountMode.into());
        assert_eq!(app.data.spawn.child_count, 2);
        Ok(())
    }

    // ========== ReviewChildCount mode tests ==========

    #[test]
    fn test_handle_review_child_count_mode_up() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReviewChildCountMode.into());
        app.data.spawn.child_count = 1;
        handle_review_child_count_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.spawn.child_count, 2);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_down() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReviewChildCountMode.into());
        app.data.spawn.child_count = 2;
        handle_review_child_count_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.spawn.child_count, 1);
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReviewChildCountMode.into());
        handle_review_child_count_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_enter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReviewChildCountMode.into());
        handle_review_child_count_mode(&mut app, KeyCode::Enter)?;
        // Should proceed to BranchSelector mode
        assert_eq!(app.mode, BranchSelectorMode.into());
        Ok(())
    }

    #[test]
    fn test_handle_review_child_count_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReviewChildCountMode.into());
        app.data.spawn.child_count = 2;
        handle_review_child_count_mode(&mut app, KeyCode::Tab)?;
        assert_eq!(app.mode, ReviewChildCountMode.into());
        assert_eq!(app.data.spawn.child_count, 2);
        Ok(())
    }

    // ========== ReviewInfo mode tests ==========

    #[test]
    fn test_handle_review_info_mode() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(ReviewInfoMode.into());
        handle_review_info_mode(&mut app)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    // ========== BranchSelector mode tests ==========

    #[test]
    fn test_handle_branch_selector_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(BranchSelectorMode.into());
        handle_branch_selector_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_filter_char() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(BranchSelectorMode.into());
        handle_branch_selector_mode(&mut app, KeyCode::Char('m'))?;
        assert_eq!(app.data.review.filter, "m");
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_filter_backspace() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(BranchSelectorMode.into());
        app.data.review.filter = "ma".to_string();
        handle_branch_selector_mode(&mut app, KeyCode::Backspace)?;
        assert_eq!(app.data.review.filter, "m");
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_navigation_up() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(BranchSelectorMode.into());
        app.data.review.branches = test_branches();
        app.data.review.selected = 1;
        handle_branch_selector_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_navigation_down() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(BranchSelectorMode.into());
        app.data.review.branches = test_branches();
        app.data.review.selected = 0;
        handle_branch_selector_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_branch_selector_mode_other_key_ignored() -> Result<(), Box<dyn std::error::Error>>
    {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(BranchSelectorMode.into());
        handle_branch_selector_mode(&mut app, KeyCode::Tab)?;
        assert_eq!(app.mode, BranchSelectorMode.into());
        Ok(())
    }

    // ========== RebaseBranchSelector mode tests ==========

    #[test]
    fn test_handle_rebase_branch_selector_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(RebaseBranchSelectorMode.into());
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_filter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(RebaseBranchSelectorMode.into());
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Char('m'))?;
        assert_eq!(app.data.review.filter, "m");

        handle_rebase_branch_selector_mode(&mut app, KeyCode::Backspace)?;
        assert_eq!(app.data.review.filter, "");
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_navigation_up()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(RebaseBranchSelectorMode.into());
        app.data.review.branches = test_branches();
        app.data.review.selected = 1;
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_navigation_down()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(RebaseBranchSelectorMode.into());
        app.data.review.branches = test_branches();
        app.data.review.selected = 0;
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_rebase_branch_selector_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(RebaseBranchSelectorMode.into());
        handle_rebase_branch_selector_mode(&mut app, KeyCode::Tab)?;
        assert_eq!(app.mode, RebaseBranchSelectorMode.into());
        Ok(())
    }

    // ========== MergeBranchSelector mode tests ==========

    #[test]
    fn test_handle_merge_branch_selector_mode_esc() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(MergeBranchSelectorMode.into());
        handle_merge_branch_selector_mode(&mut app, KeyCode::Esc)?;
        assert_eq!(app.mode, AppMode::normal());
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_filter() -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(MergeBranchSelectorMode.into());
        handle_merge_branch_selector_mode(&mut app, KeyCode::Char('f'))?;
        assert_eq!(app.data.review.filter, "f");

        handle_merge_branch_selector_mode(&mut app, KeyCode::Backspace)?;
        assert_eq!(app.data.review.filter, "");
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_navigation_up()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(MergeBranchSelectorMode.into());
        app.data.review.branches = test_branches();
        app.data.review.selected = 1;
        handle_merge_branch_selector_mode(&mut app, KeyCode::Up)?;
        assert_eq!(app.data.review.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_navigation_down()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(MergeBranchSelectorMode.into());
        app.data.review.branches = test_branches();
        app.data.review.selected = 0;
        handle_merge_branch_selector_mode(&mut app, KeyCode::Down)?;
        assert_eq!(app.data.review.selected, 1);
        Ok(())
    }

    #[test]
    fn test_handle_merge_branch_selector_mode_other_key_ignored()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut app, _temp) = create_test_app()?;
        app.apply_mode(MergeBranchSelectorMode.into());
        handle_merge_branch_selector_mode(&mut app, KeyCode::Tab)?;
        assert_eq!(app.mode, MergeBranchSelectorMode.into());
        Ok(())
    }
}
