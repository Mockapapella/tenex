//! Mode-specific key handling
//!
//! This module provides a unified way to handle keyboard input based on the current mode.
//! Each mode has its own handler that implements the `ModeKeyHandler` trait.

mod command;
mod confirm;
mod mouse;
mod normal;
mod picker;
mod text_input;

use crate::app::App;
use crate::state::AppMode;
use anyhow::Result;
use ratatui::{
    crossterm::event::{KeyCode, KeyModifiers, MouseEvent},
    layout::Rect,
};

#[derive(Clone, Copy)]
struct KeyEventHandlers {
    text_input: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
    child_count: fn(&mut App, KeyCode) -> Result<()>,
    review_child_count: fn(&mut App, KeyCode) -> Result<()>,
    review_info: fn(&mut App) -> Result<()>,
    branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    confirm_push: fn(&mut App, KeyCode) -> Result<()>,
    confirm_push_for_pr: fn(&mut App, KeyCode) -> Result<()>,
    rename_branch: fn(&mut App, KeyCode) -> Result<()>,
    confirming: fn(&mut App, crate::state::ConfirmAction, KeyCode) -> Result<()>,
    rebase_branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    merge_branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    switch_branch_selector: fn(&mut App, KeyCode) -> Result<()>,
    keyboard_remap: fn(&mut App, KeyCode) -> Result<()>,
    update_prompt: fn(&mut App, &crate::update::UpdateInfo, KeyCode) -> Result<()>,
    dispatch_changelog:
        fn(&mut App, Option<semver::Version>, usize, KeyCode, KeyModifiers) -> Result<()>,
    dispatch_help: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
    dispatch_error_modal: fn(&mut App, String) -> Result<()>,
    dispatch_success_modal: fn(&mut App, String) -> Result<()>,
    command_palette: fn(&mut App, KeyCode) -> Result<()>,
    model_selector: fn(&mut App, KeyCode) -> Result<()>,
    settings_menu: fn(&mut App, KeyCode) -> Result<()>,
    dispatch_preview_focused: fn(&mut App, KeyCode, KeyModifiers, &mut Vec<String>) -> Result<()>,
    dispatch_diff_focused: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
    normal: fn(&mut App, KeyCode, KeyModifiers) -> Result<()>,
}

const KEY_EVENT_HANDLERS: KeyEventHandlers = KeyEventHandlers {
    text_input: text_input::handle_text_input_mode,
    child_count: picker::handle_child_count_mode,
    review_child_count: picker::handle_review_child_count_mode,
    review_info: picker::handle_review_info_mode,
    branch_selector: picker::handle_branch_selector_mode,
    confirm_push: confirm::handle_confirm_push_mode,
    confirm_push_for_pr: confirm::handle_confirm_push_for_pr_mode,
    rename_branch: confirm::handle_rename_branch_mode,
    confirming: confirm::handle_confirming_mode,
    rebase_branch_selector: picker::handle_rebase_branch_selector_mode,
    merge_branch_selector: picker::handle_merge_branch_selector_mode,
    switch_branch_selector: picker::handle_switch_branch_selector_mode,
    keyboard_remap: confirm::handle_keyboard_remap_mode,
    update_prompt: confirm::handle_update_prompt_mode,
    dispatch_changelog: crate::action::dispatch_changelog_mode,
    dispatch_help: crate::action::dispatch_help_mode,
    dispatch_error_modal: crate::action::dispatch_error_modal_mode,
    dispatch_success_modal: crate::action::dispatch_success_modal_mode,
    command_palette: command::handle_command_palette_mode,
    model_selector: command::handle_model_selector_mode,
    settings_menu: command::handle_settings_menu_mode,
    dispatch_preview_focused: crate::action::dispatch_preview_focused_mode,
    dispatch_diff_focused: crate::action::dispatch_diff_focused_mode,
    normal: normal::handle_normal_mode,
};

/// Handle a key event based on the current mode
///
/// Returns Ok(()) if the key was handled or ignored, or an error if something went wrong.
pub fn handle_key_event(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) -> Result<()> {
    handle_key_event_with_handlers(app, code, modifiers, batched_keys, KEY_EVENT_HANDLERS)
}

fn handle_key_event_with_handlers(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
    handlers: KeyEventHandlers,
) -> Result<()> {
    match &app.mode {
        // Text input modes
        AppMode::Creating(_)
        | AppMode::Prompting(_)
        | AppMode::ChildPrompt(_)
        | AppMode::Broadcasting(_)
        | AppMode::ReconnectPrompt(_)
        | AppMode::TerminalPrompt(_)
        | AppMode::CustomAgentCommand(_)
        | AppMode::SynthesisPrompt(_) => {
            (handlers.text_input)(app, code, modifiers)?;
        }

        // Count picker modes
        AppMode::ChildCount(_) => {
            (handlers.child_count)(app, code)?;
        }
        AppMode::ReviewChildCount(_) => {
            (handlers.review_child_count)(app, code)?;
        }
        AppMode::ReviewInfo(_) => {
            (handlers.review_info)(app)?;
        }

        // Branch selector mode
        AppMode::BranchSelector(_) => {
            (handlers.branch_selector)(app, code)?;
        }

        // Git operation confirmation modes
        AppMode::ConfirmPush(_) => {
            (handlers.confirm_push)(app, code)?;
        }
        AppMode::ConfirmPushForPR(_) => {
            (handlers.confirm_push_for_pr)(app, code)?;
        }
        AppMode::RenameBranch(_) => {
            (handlers.rename_branch)(app, code)?;
        }

        // General confirmation mode
        AppMode::Confirming(state) => {
            (handlers.confirming)(app, state.action, code)?;
        }

        // Rebase/Merge branch selector modes
        AppMode::RebaseBranchSelector(_) => {
            (handlers.rebase_branch_selector)(app, code)?;
        }
        AppMode::MergeBranchSelector(_) => {
            (handlers.merge_branch_selector)(app, code)?;
        }
        AppMode::SwitchBranchSelector(_) => {
            (handlers.switch_branch_selector)(app, code)?;
        }

        // Keyboard remap prompt
        AppMode::KeyboardRemapPrompt(_) => {
            (handlers.keyboard_remap)(app, code)?;
        }
        // Self-update prompt on startup
        AppMode::UpdatePrompt(state) => {
            let info = state.info.clone();
            (handlers.update_prompt)(app, &info, code)?;
        }
        // Ignore input while the app is busy with a blocking background step.
        AppMode::UpdateRequested(_) | AppMode::PreparingDocker(_) => {}

        // Help, error, and success modes
        AppMode::Changelog(state) => {
            let mark_seen_version = state.mark_seen_version.clone();
            let max_scroll = crate::action::changelog_max_scroll(&app.data, state);
            (handlers.dispatch_changelog)(app, mark_seen_version, max_scroll, code, modifiers)?;
        }
        AppMode::Help(_) => {
            (handlers.dispatch_help)(app, code, modifiers)?;
        }
        AppMode::ErrorModal(state) => {
            (handlers.dispatch_error_modal)(app, state.message.clone())?;
        }
        AppMode::SuccessModal(state) => {
            (handlers.dispatch_success_modal)(app, state.message.clone())?;
        }

        // Slash commands
        AppMode::CommandPalette(_) => {
            (handlers.command_palette)(app, code)?;
        }

        // Slash command modal/pickers
        AppMode::ModelSelector(_) => {
            (handlers.model_selector)(app, code)?;
        }
        AppMode::SettingsMenu(_) => {
            (handlers.settings_menu)(app, code)?;
        }

        // Preview focused mode (forwards keys to the mux backend)
        AppMode::PreviewFocused(_) => {
            (handlers.dispatch_preview_focused)(app, code, modifiers, batched_keys)?;
        }
        // Diff focused mode (interactive diff view)
        AppMode::DiffFocused(_) => {
            (handlers.dispatch_diff_focused)(app, code, modifiers)?;
        }

        // Normal and scrolling modes
        AppMode::Normal(_) | AppMode::Scrolling(_) => {
            (handlers.normal)(app, code, modifiers)?;
        }
    }
    Ok(())
}

/// Handle a mouse event based on the current mode and layout.
///
/// `frame_area` should be the terminal viewport (`Rect::new(0, 0, width, height)`).
pub fn handle_mouse_event(
    app: &mut App,
    mouse: MouseEvent,
    frame_area: Rect,
    batched_keys: &mut Vec<String>,
) {
    mouse::handle_mouse_event(app, mouse, frame_area, batched_keys);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Status, Storage};
    use crate::app::{Settings, Tab};
    use crate::config::Config;
    use crate::state::*;
    use crate::update::UpdateInfo;
    use ratatui::crossterm::event::KeyCode;
    use semver::Version;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> (App, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    fn err_app_keycode_modifiers(_: &mut App, _: KeyCode, _: KeyModifiers) -> Result<()> {
        Err(anyhow::anyhow!("handler error with modifiers"))
    }

    fn err_app_keycode(_: &mut App, _: KeyCode) -> Result<()> {
        Err(anyhow::anyhow!("handler error without modifiers"))
    }

    fn err_app_only(_: &mut App) -> Result<()> {
        Err(anyhow::anyhow!("handler error app only"))
    }

    fn err_confirming(_: &mut App, _: ConfirmAction, _: KeyCode) -> Result<()> {
        Err(anyhow::anyhow!("handler error confirming"))
    }

    fn err_update_prompt(_: &mut App, _: &UpdateInfo, _: KeyCode) -> Result<()> {
        Err(anyhow::anyhow!("handler error update prompt"))
    }

    fn err_changelog(
        _: &mut App,
        _: Option<Version>,
        _: usize,
        _: KeyCode,
        _: KeyModifiers,
    ) -> Result<()> {
        Err(anyhow::anyhow!("handler error changelog"))
    }

    fn err_app_string(_: &mut App, _: String) -> Result<()> {
        Err(anyhow::anyhow!("handler error app string"))
    }

    fn err_preview_focused(
        _: &mut App,
        _: KeyCode,
        _: KeyModifiers,
        _: &mut Vec<String>,
    ) -> Result<()> {
        Err(anyhow::anyhow!("handler error preview focused"))
    }

    fn base_handlers() -> KeyEventHandlers {
        let ok_app_keycode_modifiers: fn(&mut App, KeyCode, KeyModifiers) -> Result<()> =
            |_, _, _| Ok(());
        let ok_app_keycode: fn(&mut App, KeyCode) -> Result<()> = |_, _| Ok(());
        let ok_app_only: fn(&mut App) -> Result<()> = |_| Ok(());
        let ok_confirming: fn(&mut App, ConfirmAction, KeyCode) -> Result<()> = |_, _, _| Ok(());
        let ok_update_prompt: fn(&mut App, &UpdateInfo, KeyCode) -> Result<()> = |_, _, _| Ok(());
        let ok_changelog: fn(
            &mut App,
            Option<Version>,
            usize,
            KeyCode,
            KeyModifiers,
        ) -> Result<()> = |_, _, _, _, _| Ok(());
        let ok_app_string: fn(&mut App, String) -> Result<()> = |_, _| Ok(());
        let ok_preview_focused: fn(
            &mut App,
            KeyCode,
            KeyModifiers,
            &mut Vec<String>,
        ) -> Result<()> = |_, _, _, _| Ok(());

        KeyEventHandlers {
            text_input: ok_app_keycode_modifiers,
            child_count: ok_app_keycode,
            review_child_count: ok_app_keycode,
            review_info: ok_app_only,
            branch_selector: ok_app_keycode,
            confirm_push: ok_app_keycode,
            confirm_push_for_pr: ok_app_keycode,
            rename_branch: ok_app_keycode,
            confirming: ok_confirming,
            rebase_branch_selector: ok_app_keycode,
            merge_branch_selector: ok_app_keycode,
            switch_branch_selector: ok_app_keycode,
            keyboard_remap: ok_app_keycode,
            update_prompt: ok_update_prompt,
            dispatch_changelog: ok_changelog,
            dispatch_help: ok_app_keycode_modifiers,
            dispatch_error_modal: ok_app_string,
            dispatch_success_modal: ok_app_string,
            command_palette: ok_app_keycode,
            model_selector: ok_app_keycode,
            settings_menu: ok_app_keycode,
            dispatch_preview_focused: ok_preview_focused,
            dispatch_diff_focused: ok_app_keycode_modifiers,
            normal: ok_app_keycode_modifiers,
        }
    }

    fn assert_error_propagates(
        configure_mode: impl FnOnce(&mut App),
        configure_handlers: impl FnOnce(&mut KeyEventHandlers),
        expected_error: &str,
    ) {
        let (mut app, _temp) = create_test_app();
        configure_mode(&mut app);

        let mut handlers = base_handlers();
        configure_handlers(&mut handlers);

        let mut batched_keys = Vec::new();
        let err = handle_key_event_with_handlers(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
            handlers,
        )
        .unwrap_err();
        assert!(err.to_string().contains(expected_error));
    }

    // ========== Mode routing integration tests ==========

    #[test]
    fn test_handle_key_event_propagates_errors_from_text_and_picker_handlers() {
        assert_error_propagates(
            |app| app.mode = AppMode::Creating(CreatingMode),
            |handlers| handlers.text_input = err_app_keycode_modifiers,
            "handler error with modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::ChildCount(ChildCountMode),
            |handlers| handlers.child_count = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::ReviewChildCount(ReviewChildCountMode),
            |handlers| handlers.review_child_count = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::ReviewInfo(ReviewInfoMode),
            |handlers| handlers.review_info = err_app_only,
            "handler error app only",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::BranchSelector(BranchSelectorMode),
            |handlers| handlers.branch_selector = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::ConfirmPush(ConfirmPushMode),
            |handlers| handlers.confirm_push = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::ConfirmPushForPR(ConfirmPushForPRMode),
            |handlers| handlers.confirm_push_for_pr = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::RenameBranch(RenameBranchMode),
            |handlers| handlers.rename_branch = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| {
                app.mode = AppMode::Confirming(ConfirmingMode {
                    action: ConfirmAction::Quit,
                });
            },
            |handlers| handlers.confirming = err_confirming,
            "handler error confirming",
        );
    }

    #[test]
    fn test_handle_key_event_propagates_errors_from_prompt_and_modal_handlers() {
        assert_error_propagates(
            |app| app.mode = AppMode::RebaseBranchSelector(RebaseBranchSelectorMode),
            |handlers| handlers.rebase_branch_selector = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::MergeBranchSelector(MergeBranchSelectorMode),
            |handlers| handlers.merge_branch_selector = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::SwitchBranchSelector(SwitchBranchSelectorMode),
            |handlers| handlers.switch_branch_selector = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::KeyboardRemapPrompt(KeyboardRemapPromptMode),
            |handlers| handlers.keyboard_remap = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| {
                app.mode = AppMode::UpdatePrompt(UpdatePromptMode {
                    info: UpdateInfo {
                        current_version: Version::new(1, 0, 0),
                        latest_version: Version::new(2, 0, 0),
                    },
                });
            },
            |handlers| handlers.update_prompt = err_update_prompt,
            "handler error update prompt",
        );
        assert_error_propagates(
            |app| {
                app.mode = AppMode::Changelog(ChangelogMode {
                    title: "What's new".to_string(),
                    lines: vec!["line".to_string()],
                    mark_seen_version: None,
                });
            },
            |handlers| handlers.dispatch_changelog = err_changelog,
            "handler error changelog",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::Help(HelpMode),
            |handlers| handlers.dispatch_help = err_app_keycode_modifiers,
            "handler error with modifiers",
        );
        assert_error_propagates(
            |app| app.set_error("test error"),
            |handlers| handlers.dispatch_error_modal = err_app_string,
            "handler error app string",
        );
        assert_error_propagates(
            |app| app.show_success("success!"),
            |handlers| handlers.dispatch_success_modal = err_app_string,
            "handler error app string",
        );
    }

    #[test]
    fn test_handle_key_event_propagates_errors_from_focus_and_normal_handlers() {
        assert_error_propagates(
            |app| app.apply_mode(CommandPaletteMode.into()),
            |handlers| handlers.command_palette = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.apply_mode(ModelSelectorMode.into()),
            |handlers| handlers.model_selector = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.apply_mode(SettingsMenuMode.into()),
            |handlers| handlers.settings_menu = err_app_keycode,
            "handler error without modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::PreviewFocused(PreviewFocusedMode),
            |handlers| handlers.dispatch_preview_focused = err_preview_focused,
            "handler error preview focused",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::DiffFocused(DiffFocusedMode),
            |handlers| handlers.dispatch_diff_focused = err_app_keycode_modifiers,
            "handler error with modifiers",
        );
        assert_error_propagates(
            |app| app.mode = AppMode::normal(),
            |handlers| handlers.normal = err_app_keycode_modifiers,
            "handler error with modifiers",
        );
    }

    #[test]
    fn test_handle_key_event_with_handlers_ok_paths_cover_handler_stubs() {
        let mut batched_keys = Vec::new();
        let (mut app, _temp) = create_test_app();
        let handlers = base_handlers();
        let none = KeyModifiers::NONE;

        app.mode = AppMode::Creating(CreatingMode);
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char('x'),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();

        app.mode = AppMode::ChildCount(ChildCountMode);
        handle_key_event_with_handlers(&mut app, KeyCode::Esc, none, &mut batched_keys, handlers)
            .unwrap();

        app.mode = AppMode::ReviewInfo(ReviewInfoMode);
        handle_key_event_with_handlers(&mut app, KeyCode::Enter, none, &mut batched_keys, handlers)
            .unwrap();

        app.mode = AppMode::Confirming(ConfirmingMode {
            action: ConfirmAction::Quit,
        });
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char('n'),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();

        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = AppMode::UpdatePrompt(UpdatePromptMode { info });
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char('n'),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();

        app.mode = AppMode::Changelog(ChangelogMode {
            title: "What's new".to_string(),
            lines: vec!["line".to_string()],
            mark_seen_version: None,
        });
        handle_key_event_with_handlers(&mut app, KeyCode::Esc, none, &mut batched_keys, handlers)
            .unwrap();

        app.mode = AppMode::Help(HelpMode);
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char('q'),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();

        app.set_error("test error");
        handle_key_event_with_handlers(&mut app, KeyCode::Enter, none, &mut batched_keys, handlers)
            .unwrap();

        app.show_success("success!");
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char(' '),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();

        app.mode = AppMode::PreviewFocused(PreviewFocusedMode);
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char('x'),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();

        app.mode = AppMode::normal();
        handle_key_event_with_handlers(
            &mut app,
            KeyCode::Char('?'),
            none,
            &mut batched_keys,
            handlers,
        )
        .unwrap();
    }

    #[test]
    fn test_handle_key_event_help_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_error_modal_mode() {
        let (mut app, _temp) = create_test_app();
        app.set_error("test error");
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_success_modal_mode() {
        let (mut app, _temp) = create_test_app();
        app.show_success("success!");
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char(' '),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .unwrap();

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_diff_focused_ctrl_q_exits() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::DiffFocused(DiffFocusedMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )
        .unwrap();

        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_diff_focused_tab_does_not_switch_tabs() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::DiffFocused(DiffFocusedMode);
        app.data.active_tab = Tab::Diff;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Tab,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected diff focused Tab to succeed");

        assert_eq!(app.data.active_tab, Tab::Diff);
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::DiffFocused(DiffFocusedMode))
        );
    }

    #[test]
    fn test_handle_key_event_preview_focused_tab_is_ignored() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::PreviewFocused(PreviewFocusedMode);
        app.data.active_tab = Tab::Preview;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Tab,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected preview focused Tab to succeed");

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::PreviewFocused(PreviewFocusedMode))
        );
        assert!(batched_keys.is_empty());
    }

    #[test]
    fn test_handle_key_event_confirm_push_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ConfirmPush(ConfirmPushMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected confirm push input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_confirm_push_for_pr_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ConfirmPushForPR(ConfirmPushForPRMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected confirm push for PR input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_rename_branch_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::RenameBranch(RenameBranchMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected rename branch input to succeed");

        assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
        assert_eq!(app.data.input.buffer, "a");
    }

    #[test]
    fn test_handle_key_event_keyboard_remap_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::KeyboardRemapPrompt(KeyboardRemapPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected keyboard remap input to succeed");

        assert!(app.data.settings.merge_key_remapped);
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_update_prompt_mode() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = AppMode::UpdatePrompt(UpdatePromptMode { info });
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected update prompt input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_update_requested_mode_ignores_input() {
        let (mut app, _temp) = create_test_app();
        let info = UpdateInfo {
            current_version: Version::new(1, 0, 0),
            latest_version: Version::new(2, 0, 0),
        };
        app.mode = AppMode::UpdateRequested(UpdateRequestedMode { info });
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected update requested input to succeed");

        // Should remain in UpdateRequested mode - input is ignored
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::UpdateRequested(UpdateRequestedMode {
                info: UpdateInfo {
                    current_version: Version::new(0, 0, 0),
                    latest_version: Version::new(0, 0, 0),
                },
            }))
        );
    }

    #[test]
    fn test_handle_key_event_changelog_mode_exits() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Changelog(ChangelogMode {
            title: "What's new".to_string(),
            lines: vec!["line".to_string()],
            mark_seen_version: None,
        });

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected changelog input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_command_palette_mode_routes_to_handler() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(CommandPaletteMode.into());

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected command palette input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_model_selector_mode_routes_to_handler() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(ModelSelectorMode.into());

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected model selector input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_settings_menu_mode_routes_to_handler() {
        let (mut app, _temp) = create_test_app();
        app.apply_mode(SettingsMenuMode.into());

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected settings menu input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_confirming_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Confirming(ConfirmingMode {
            action: ConfirmAction::Quit,
        });
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected confirming mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_creating_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Creating(CreatingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('t'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected creating mode input to succeed");

        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        assert_eq!(app.data.input.buffer, "t");
    }

    #[test]
    fn test_handle_key_event_prompting_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Prompting(PromptingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected prompting mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_child_count_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ChildCount(ChildCountMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected child count mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_review_child_count_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ReviewChildCount(ReviewChildCountMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected review child count mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_review_info_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ReviewInfo(ReviewInfoMode);
        let mut batched_keys = Vec::new();

        // ReviewInfo mode exits on any key
        handle_key_event(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected review info mode input to succeed");

        // Should exit to Normal mode
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_branch_selector_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::BranchSelector(BranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected branch selector mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_rebase_branch_selector_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::RebaseBranchSelector(RebaseBranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected rebase branch selector mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_merge_branch_selector_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::MergeBranchSelector(MergeBranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected merge branch selector mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_switch_branch_selector_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::SwitchBranchSelector(SwitchBranchSelectorMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected switch branch selector mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_broadcasting_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Broadcasting(BroadcastingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('h'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected broadcasting mode input to succeed");

        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
        assert_eq!(app.data.input.buffer, "h");
    }

    #[test]
    fn test_handle_key_event_reconnect_prompt_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ReconnectPrompt(ReconnectPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected reconnect prompt mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_child_prompt_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::ChildPrompt(ChildPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected child prompt mode input to succeed");

        assert_eq!(app.mode, AppMode::ChildPrompt(ChildPromptMode));
        assert_eq!(app.data.input.buffer, "x");
    }

    #[test]
    fn test_handle_key_event_terminal_prompt_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::TerminalPrompt(TerminalPromptMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected terminal prompt mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_scrolling_mode() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Scrolling(ScrollingMode);
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Esc,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected scrolling mode input to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_handle_key_event_scrolling_mode_up_down_scrolls_content_not_agents() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Scrolling(ScrollingMode);
        app.data.active_tab = Tab::Commits;

        app.data.storage.add(Agent::new(
            "a0".to_string(),
            "echo".to_string(),
            "tenex-test/a0".to_string(),
            PathBuf::from("/tmp"),
        ));
        app.data.storage.add(Agent::new(
            "a1".to_string(),
            "echo".to_string(),
            "tenex-test/a1".to_string(),
            PathBuf::from("/tmp"),
        ));
        app.data.selected = 0;

        app.data.ui.set_commits_content(
            (0..30)
                .map(|i| format!("line{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Down,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected scrolling mode down arrow to succeed");

        assert_eq!(app.data.selected, 0);
        assert_eq!(app.data.ui.commits_scroll, 1);
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::Scrolling(ScrollingMode))
        );
    }

    #[test]
    fn test_handle_key_event_scrolling_mode_tab_does_not_switch_tabs() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Scrolling(ScrollingMode);
        app.data.active_tab = Tab::Preview;

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Tab,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected scrolling mode tab to succeed");

        assert_eq!(app.data.active_tab, Tab::Preview);
        assert_eq!(
            std::mem::discriminant(&app.mode),
            std::mem::discriminant(&AppMode::Scrolling(ScrollingMode))
        );
    }

    #[test]
    fn test_handle_key_event_scrolling_mode_ctrl_q_exits_to_normal_instead_of_quit() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Scrolling(ScrollingMode);
        app.data.active_tab = Tab::Commits;

        let mut agent = Agent::new(
            "a0".to_string(),
            "echo".to_string(),
            "tenex-test/a0".to_string(),
            PathBuf::from("/tmp"),
        );
        agent.set_status(Status::Running);
        app.data.storage.add(agent);
        app.data.selected = 0;

        let mut batched_keys = Vec::new();
        handle_key_event(
            &mut app,
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )
        .expect("Expected scrolling mode Ctrl-Q to succeed");

        assert_eq!(app.mode, AppMode::normal());
        assert!(!app.data.should_quit);
    }

    #[test]
    fn test_handle_key_event_normal_mode_help() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::normal();
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('?'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected normal mode help input to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
    }

    #[test]
    fn test_handle_key_event_help_mode_scroll_does_not_exit() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Down,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode down arrow to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 1);
    }

    #[test]
    fn test_handle_key_event_help_mode_scroll_up_from_bottom_is_immediate() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = usize::MAX;

        let max_scroll = crate::action::help_max_scroll(&app.data);
        assert_ne!(max_scroll, 0);

        let mut batched_keys = Vec::new();

        handle_key_event(&mut app, KeyCode::Up, KeyModifiers::NONE, &mut batched_keys).unwrap();

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, max_scroll.saturating_sub(1));
    }

    #[test]
    fn test_handle_key_event_help_mode_page_down() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::PageDown,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode page down to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert!(app.data.ui.help_scroll > 0);
    }

    #[test]
    fn test_handle_key_event_help_mode_page_up() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::PageUp,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode page up to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 0);
    }

    #[test]
    fn test_handle_key_event_help_mode_ctrl_d() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )
        .expect("Expected help mode Ctrl-D to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert!(app.data.ui.help_scroll > 0);
    }

    #[test]
    fn test_handle_key_event_help_mode_ctrl_u() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
            &mut batched_keys,
        )
        .expect("Expected help mode Ctrl-U to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 5);
    }

    #[test]
    fn test_handle_key_event_help_mode_go_to_top() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('g'),
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode go-to-top to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 0);
    }

    #[test]
    fn test_handle_key_event_help_mode_go_to_bottom() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Char('G'),
            KeyModifiers::SHIFT,
            &mut batched_keys,
        )
        .expect("Expected help mode go-to-bottom to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        let max_scroll = crate::action::help_max_scroll(&app.data);
        assert_eq!(app.data.ui.help_scroll, max_scroll);
    }

    #[test]
    fn test_handle_key_event_help_mode_home_key() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 10;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Home,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode home key to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        assert_eq!(app.data.ui.help_scroll, 0);
    }

    #[test]
    fn test_handle_key_event_help_mode_end_key() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::End,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode end key to succeed");

        assert_eq!(app.mode, AppMode::Help(HelpMode));
        let max_scroll = crate::action::help_max_scroll(&app.data);
        assert_eq!(app.data.ui.help_scroll, max_scroll);
    }

    #[test]
    fn test_handle_key_event_help_mode_any_other_key_exits() {
        let (mut app, _temp) = create_test_app();
        app.mode = AppMode::Help(HelpMode);
        app.data.ui.help_scroll = 0;
        let mut batched_keys = Vec::new();

        handle_key_event(
            &mut app,
            KeyCode::Enter,
            KeyModifiers::NONE,
            &mut batched_keys,
        )
        .expect("Expected help mode exit key to succeed");

        assert_eq!(app.mode, AppMode::normal());
    }
}
