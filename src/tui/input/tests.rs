use super::*;
use crate::agent::Storage;
use crate::app::{ConfirmAction, Settings, TextInputKind};
use crate::config::Config;
use crate::update::UpdateInfo;
use ratatui::crossterm::event::KeyCode;
use semver::Version;
use tempfile::NamedTempFile;

fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
    let temp_file = NamedTempFile::new()?;
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    Ok((
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    ))
}

// ========== Mode routing integration tests ==========

#[test]
fn test_handle_key_event_help_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_error_modal_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Error("test error".to_string()));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Enter,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_success_modal_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Success("success!".to_string()));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char(' '),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_confirm_push_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Push));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('n'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_confirm_push_for_pr_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Confirm(ConfirmKind::PushForPR));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_rename_branch_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::RenameBranch));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('a'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(
        app.mode,
        Mode::Overlay(OverlayMode::TextInput(TextInputKind::RenameBranch))
    );
    assert_eq!(app.input.buffer, "a");
    Ok(())
}

#[test]
fn test_handle_key_event_keyboard_remap_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Confirm(ConfirmKind::KeyboardRemap));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('y'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert!(app.settings.merge_key_remapped);
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_update_prompt_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };
    app.mode = Mode::Overlay(OverlayMode::Confirm(ConfirmKind::UpdatePrompt(info)));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('n'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_update_requested_mode_ignores_input() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };
    app.mode = Mode::UpdateRequested(info);
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    // Should remain in UpdateRequested mode - input is ignored
    assert!(matches!(app.mode, Mode::UpdateRequested(_)));
    Ok(())
}

#[test]
fn test_handle_key_event_confirming_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
        ConfirmAction::Quit,
    )));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('n'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_creating_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('t'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(
        app.mode,
        Mode::Overlay(OverlayMode::TextInput(TextInputKind::Creating))
    );
    assert_eq!(app.input.buffer, "t");
    Ok(())
}

#[test]
fn test_handle_key_event_prompting_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Prompting));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_child_count_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::CountPicker(CountPickerKind::ChildCount));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_review_child_count_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::CountPicker(CountPickerKind::ReviewChildCount));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_review_info_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::ReviewInfo);
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    // ReviewInfo mode exits on any key
    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Enter,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    // Should exit to Normal mode
    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_branch_selector_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::ReviewBaseBranch,
    ));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_rebase_branch_selector_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::BranchPicker(
        BranchPickerKind::RebaseTargetBranch,
    ));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_merge_branch_selector_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::BranchPicker(BranchPickerKind::MergeFromBranch));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_broadcasting_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::Broadcasting));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('h'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(
        app.mode,
        Mode::Overlay(OverlayMode::TextInput(TextInputKind::Broadcasting))
    );
    assert_eq!(app.input.buffer, "h");
    Ok(())
}

#[test]
fn test_handle_key_event_reconnect_prompt_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::ReconnectPrompt));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_child_prompt_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::ChildPrompt));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('x'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(
        app.mode,
        Mode::Overlay(OverlayMode::TextInput(TextInputKind::ChildPrompt))
    );
    assert_eq!(app.input.buffer, "x");
    Ok(())
}

#[test]
fn test_handle_key_event_terminal_prompt_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::TextInput(TextInputKind::TerminalPrompt));
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_scrolling_mode() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Scrolling;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Esc,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}

#[test]
fn test_handle_key_event_normal_mode_help() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Normal;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('?'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_scroll_does_not_exit() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 0;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Down,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert_eq!(app.ui.help_scroll, 1);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_scroll_up_from_bottom_is_immediate() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = usize::MAX;

    let max_scroll = help_max_scroll(&app);
    assert_ne!(max_scroll, 0, "help should be scrollable for this test");

    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Up,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert_eq!(app.ui.help_scroll, max_scroll.saturating_sub(1));
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_page_down() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 0;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::PageDown,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert!(app.ui.help_scroll > 0);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_page_up() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 10;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::PageUp,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert_eq!(app.ui.help_scroll, 0);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_ctrl_d() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 0;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert!(app.ui.help_scroll > 0);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_ctrl_u() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 10;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert_eq!(app.ui.help_scroll, 5);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_go_to_top() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 10;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('g'),
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert_eq!(app.ui.help_scroll, 0);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_go_to_bottom() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 0;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Char('G'),
        KeyModifiers::SHIFT,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    let max_scroll = help_max_scroll(&app);
    assert_eq!(app.ui.help_scroll, max_scroll);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_home_key() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 10;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Home,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    assert_eq!(app.ui.help_scroll, 0);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_end_key() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 0;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::End,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Overlay(OverlayMode::Help));
    let max_scroll = help_max_scroll(&app);
    assert_eq!(app.ui.help_scroll, max_scroll);
    Ok(())
}

#[test]
fn test_handle_key_event_help_mode_any_other_key_exits() -> anyhow::Result<()> {
    let (mut app, _temp) = create_test_app()?;
    app.mode = Mode::Overlay(OverlayMode::Help);
    app.ui.help_scroll = 0;
    let action_handler = Actions::new();
    let mut batched_keys = Vec::new();

    handle_key_event(
        &mut app,
        action_handler,
        KeyCode::Enter,
        KeyModifiers::NONE,
        &mut batched_keys,
    )?;

    assert_eq!(app.mode, Mode::Normal);
    Ok(())
}
