use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::config::Config;
use crate::git::BranchInfo;
use crate::state::{
    AppMode, BranchSelectorMode, ChildCountMode, ChildPromptMode, MergeBranchSelectorMode,
    RebaseBranchSelectorMode, ReviewChildCountMode, ReviewInfoMode, SwitchBranchSelectorMode,
};
use ratatui::crossterm::event::KeyCode;
use tempfile::NamedTempFile;

fn create_test_app() -> (App, NamedTempFile) {
    let temp_file = NamedTempFile::new().unwrap();
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
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
fn test_handle_child_count_mode_up() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ChildCountMode.into());
    app.data.spawn.child_count = 1;
    handle_child_count_mode(&mut app, KeyCode::Up).unwrap();
    assert_eq!(app.data.spawn.child_count, 2);
}

#[test]
fn test_handle_child_count_mode_down() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ChildCountMode.into());
    app.data.spawn.child_count = 2;
    handle_child_count_mode(&mut app, KeyCode::Down).unwrap();
    assert_eq!(app.data.spawn.child_count, 1);
}

#[test]
fn test_handle_child_count_mode_esc() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ChildCountMode.into());
    handle_child_count_mode(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_child_count_mode_enter() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ChildCountMode.into());
    app.data.spawn.child_count = 2;
    handle_child_count_mode(&mut app, KeyCode::Enter).unwrap();
    // Should proceed to ChildPrompt mode
    assert_eq!(app.mode, ChildPromptMode.into());
}

#[test]
fn test_handle_child_count_mode_other_key_ignored() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ChildCountMode.into());
    app.data.spawn.child_count = 2;
    handle_child_count_mode(&mut app, KeyCode::Tab).unwrap();
    assert_eq!(app.mode, ChildCountMode.into());
    assert_eq!(app.data.spawn.child_count, 2);
}

// ========== ReviewChildCount mode tests ==========

#[test]
fn test_handle_review_child_count_mode_up() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReviewChildCountMode.into());
    app.data.spawn.child_count = 1;
    handle_review_child_count_mode(&mut app, KeyCode::Up).unwrap();
    assert_eq!(app.data.spawn.child_count, 2);
}

#[test]
fn test_handle_review_child_count_mode_down() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReviewChildCountMode.into());
    app.data.spawn.child_count = 2;
    handle_review_child_count_mode(&mut app, KeyCode::Down).unwrap();
    assert_eq!(app.data.spawn.child_count, 1);
}

#[test]
fn test_handle_review_child_count_mode_esc() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReviewChildCountMode.into());
    handle_review_child_count_mode(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_review_child_count_mode_enter() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReviewChildCountMode.into());
    handle_review_child_count_mode(&mut app, KeyCode::Enter).unwrap();
    // Should proceed to BranchSelector mode
    assert_eq!(app.mode, BranchSelectorMode.into());
}

#[test]
fn test_handle_review_child_count_mode_other_key_ignored() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReviewChildCountMode.into());
    app.data.spawn.child_count = 2;
    handle_review_child_count_mode(&mut app, KeyCode::Tab).unwrap();
    assert_eq!(app.mode, ReviewChildCountMode.into());
    assert_eq!(app.data.spawn.child_count, 2);
}

// ========== ReviewInfo mode tests ==========

#[test]
fn test_handle_review_info_mode() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(ReviewInfoMode.into());
    handle_review_info_mode(&mut app).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

// ========== BranchSelector mode tests ==========

#[test]
fn test_handle_branch_selector_mode_esc() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(BranchSelectorMode.into());
    handle_branch_selector_mode(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_branch_selector_mode_filter_char() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(BranchSelectorMode.into());
    handle_branch_selector_mode(&mut app, KeyCode::Char('m')).unwrap();
    assert_eq!(app.data.review.filter, "m");
}

#[test]
fn test_handle_branch_selector_mode_filter_backspace() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(BranchSelectorMode.into());
    app.data.review.filter = "ma".to_string();
    handle_branch_selector_mode(&mut app, KeyCode::Backspace).unwrap();
    assert_eq!(app.data.review.filter, "m");
}

#[test]
fn test_handle_branch_selector_mode_navigation_up() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(BranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 1;
    handle_branch_selector_mode(&mut app, KeyCode::Up).unwrap();
    assert_eq!(app.data.review.selected, 0);
}

#[test]
fn test_handle_branch_selector_mode_navigation_down() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(BranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 0;
    handle_branch_selector_mode(&mut app, KeyCode::Down).unwrap();
    assert_eq!(app.data.review.selected, 1);
}

#[test]
fn test_handle_branch_selector_mode_other_key_ignored() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(BranchSelectorMode.into());
    handle_branch_selector_mode(&mut app, KeyCode::Tab).unwrap();
    assert_eq!(app.mode, BranchSelectorMode.into());
}

// ========== RebaseBranchSelector mode tests ==========

#[test]
fn test_handle_rebase_branch_selector_mode_esc() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(RebaseBranchSelectorMode.into());
    handle_rebase_branch_selector_mode(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_rebase_branch_selector_mode_filter() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(RebaseBranchSelectorMode.into());
    handle_rebase_branch_selector_mode(&mut app, KeyCode::Char('m')).unwrap();
    assert_eq!(app.data.review.filter, "m");

    handle_rebase_branch_selector_mode(&mut app, KeyCode::Backspace).unwrap();
    assert_eq!(app.data.review.filter, "");
}

#[test]
fn test_handle_rebase_branch_selector_mode_navigation_up() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(RebaseBranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 1;
    handle_rebase_branch_selector_mode(&mut app, KeyCode::Up).unwrap();
    assert_eq!(app.data.review.selected, 0);
}

#[test]
fn test_handle_rebase_branch_selector_mode_navigation_down() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(RebaseBranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 0;
    handle_rebase_branch_selector_mode(&mut app, KeyCode::Down).unwrap();
    assert_eq!(app.data.review.selected, 1);
}

#[test]
fn test_handle_rebase_branch_selector_mode_other_key_ignored() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(RebaseBranchSelectorMode.into());
    handle_rebase_branch_selector_mode(&mut app, KeyCode::Tab).unwrap();
    assert_eq!(app.mode, RebaseBranchSelectorMode.into());
}

// ========== MergeBranchSelector mode tests ==========

#[test]
fn test_handle_merge_branch_selector_mode_esc() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(MergeBranchSelectorMode.into());
    handle_merge_branch_selector_mode(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_merge_branch_selector_mode_filter() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(MergeBranchSelectorMode.into());
    handle_merge_branch_selector_mode(&mut app, KeyCode::Char('f')).unwrap();
    assert_eq!(app.data.review.filter, "f");

    handle_merge_branch_selector_mode(&mut app, KeyCode::Backspace).unwrap();
    assert_eq!(app.data.review.filter, "");
}

#[test]
fn test_handle_merge_branch_selector_mode_navigation_up() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(MergeBranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 1;
    handle_merge_branch_selector_mode(&mut app, KeyCode::Up).unwrap();
    assert_eq!(app.data.review.selected, 0);
}

#[test]
fn test_handle_merge_branch_selector_mode_navigation_down() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(MergeBranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 0;
    handle_merge_branch_selector_mode(&mut app, KeyCode::Down).unwrap();
    assert_eq!(app.data.review.selected, 1);
}

#[test]
fn test_handle_merge_branch_selector_mode_other_key_ignored() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(MergeBranchSelectorMode.into());
    handle_merge_branch_selector_mode(&mut app, KeyCode::Tab).unwrap();
    assert_eq!(app.mode, MergeBranchSelectorMode.into());
}

// ========== SwitchBranchSelector mode tests ==========

#[test]
fn test_handle_switch_branch_selector_mode_esc() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(SwitchBranchSelectorMode.into());
    handle_switch_branch_selector_mode(&mut app, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_handle_switch_branch_selector_mode_filter() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(SwitchBranchSelectorMode.into());
    handle_switch_branch_selector_mode(&mut app, KeyCode::Char('m')).unwrap();
    assert_eq!(app.data.review.filter, "m");

    handle_switch_branch_selector_mode(&mut app, KeyCode::Backspace).unwrap();
    assert_eq!(app.data.review.filter, "");
}

#[test]
fn test_handle_switch_branch_selector_mode_navigation_up() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(SwitchBranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 1;
    handle_switch_branch_selector_mode(&mut app, KeyCode::Up).unwrap();
    assert_eq!(app.data.review.selected, 0);
}

#[test]
fn test_handle_switch_branch_selector_mode_navigation_down() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(SwitchBranchSelectorMode.into());
    app.data.review.branches = test_branches();
    app.data.review.selected = 0;
    handle_switch_branch_selector_mode(&mut app, KeyCode::Down).unwrap();
    assert_eq!(app.data.review.selected, 1);
}

#[test]
fn test_handle_switch_branch_selector_mode_other_key_ignored() {
    let (mut app, _temp) = create_test_app();
    app.apply_mode(SwitchBranchSelectorMode.into());
    handle_switch_branch_selector_mode(&mut app, KeyCode::Tab).unwrap();
    assert_eq!(app.mode, SwitchBranchSelectorMode.into());
}
