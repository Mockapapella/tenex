use super::*;
use crate::agent::WorkspaceKind;
use crate::agent::{Agent, ChildConfig, Storage};
use crate::app::Settings;
use crate::app::WorktreeConflictInfo;
use crate::config::Config;
use crate::state::ConfirmAction;
use crate::state::{
    AppMode, BranchSelectorMode, BroadcastingMode, ChildCountMode, CommandPaletteMode,
    ConfirmPushMode, ConfirmingMode, CreatingMode, DiffFocusedMode, ErrorModalMode, HelpMode,
    KeyboardRemapPromptMode, MergeBranchSelectorMode, ModelSelectorMode, PreviewFocusedMode,
    PromptingMode, RebaseBranchSelectorMode, ReconnectPromptMode, RenameBranchMode,
    ReviewChildCountMode, ReviewInfoMode, ScrollingMode, SettingsMenuMode,
    SwitchBranchSelectorMode, TerminalPromptMode, UpdatePromptMode,
};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tempfile::TempDir;

fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::sink)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::with_default(&dispatch, f)
}

fn create_test_app() -> (App, NamedTempFile) {
    let temp_file = NamedTempFile::new().expect("temp state file should be created");
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

fn add_agent_with_child(app: &mut App) {
    let worktree_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        worktree_path,
    );
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    let root_branch = root.branch.clone();
    let root_worktree = root.worktree_path.clone();
    app.data.storage.add(root);
    let child = Agent::new_child(
        "child".to_string(),
        "claude".to_string(),
        root_branch,
        root_worktree,
        ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 1,
            repo_root: None,
        },
    );
    app.data.storage.add(child);
}

fn reset_to_scrolling(app: &mut App) {
    app.exit_mode();
    app.enter_mode(ScrollingMode.into());
}

fn dispatch_changelog_and_assert_scroll(
    app: &mut App,
    max_scroll: usize,
    code: KeyCode,
    modifiers: KeyModifiers,
    expected_scroll: usize,
) {
    dispatch_changelog_mode(app, None, max_scroll, code, modifiers).unwrap();
    assert_eq!(app.data.ui.changelog_scroll, expected_scroll);
}

fn mode_is_preview_or_diff(mode: &AppMode) -> bool {
    matches!(mode, AppMode::PreviewFocused(_) | AppMode::DiffFocused(_))
}

fn mode_is_confirming(mode: &AppMode) -> bool {
    matches!(mode, AppMode::Confirming(_))
}

fn mode_is_error_modal(mode: &AppMode) -> bool {
    matches!(mode, AppMode::ErrorModal(_))
}

fn mode_is_diff_focused(mode: &AppMode) -> bool {
    matches!(mode, AppMode::DiffFocused(_))
}

fn mode_is_creating(mode: &AppMode) -> bool {
    matches!(mode, AppMode::Creating(_))
}

fn mode_is_command_palette(mode: &AppMode) -> bool {
    matches!(mode, AppMode::CommandPalette(_))
}

fn mode_is_child_count_or_normal(mode: &AppMode) -> bool {
    matches!(mode, AppMode::ChildCount(_) | AppMode::Normal(_))
}

fn mode_is_error_modal_or_switch_branch_selector(mode: &AppMode) -> bool {
    matches!(
        mode,
        AppMode::ErrorModal(_) | AppMode::SwitchBranchSelector(_)
    )
}

fn mode_is_reconnect_prompt(mode: &AppMode) -> bool {
    matches!(mode, AppMode::ReconnectPrompt(_))
}

#[test]
fn test_mode_predicates_cover_match_arms() {
    assert!(mode_is_preview_or_diff(&AppMode::PreviewFocused(
        PreviewFocusedMode
    )));
    assert!(mode_is_preview_or_diff(&AppMode::DiffFocused(
        DiffFocusedMode
    )));
    assert!(!mode_is_preview_or_diff(&AppMode::normal()));

    assert!(mode_is_confirming(&AppMode::Confirming(ConfirmingMode {
        action: ConfirmAction::Quit,
    })));
    assert!(!mode_is_confirming(&AppMode::normal()));

    assert!(mode_is_error_modal(&AppMode::ErrorModal(ErrorModalMode {
        message: "error".to_string(),
    })));
    assert!(!mode_is_error_modal(&AppMode::normal()));

    assert!(mode_is_diff_focused(&AppMode::DiffFocused(DiffFocusedMode)));
    assert!(!mode_is_diff_focused(&AppMode::normal()));

    assert!(mode_is_creating(&AppMode::Creating(CreatingMode)));
    assert!(!mode_is_creating(&AppMode::normal()));

    assert!(mode_is_command_palette(&AppMode::CommandPalette(
        CommandPaletteMode
    )));
    assert!(!mode_is_command_palette(&AppMode::normal()));

    assert!(mode_is_child_count_or_normal(&AppMode::ChildCount(
        ChildCountMode
    )));
    assert!(mode_is_child_count_or_normal(&AppMode::normal()));
    assert!(!mode_is_child_count_or_normal(&AppMode::Help(HelpMode)));

    assert!(mode_is_error_modal_or_switch_branch_selector(
        &AppMode::ErrorModal(ErrorModalMode {
            message: "error".to_string(),
        })
    ));
    assert!(mode_is_error_modal_or_switch_branch_selector(
        &AppMode::SwitchBranchSelector(SwitchBranchSelectorMode)
    ));
    assert!(!mode_is_error_modal_or_switch_branch_selector(
        &AppMode::normal()
    ));

    assert!(mode_is_reconnect_prompt(&AppMode::ReconnectPrompt(
        ReconnectPromptMode
    )));
    assert!(!mode_is_reconnect_prompt(&AppMode::normal()));
}

#[test]
fn test_scrolling_mode_typed_dispatch_covers_actions() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);

    dispatch_normal_mode(&mut app, KeyAction::ScrollUp).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

    dispatch_scrolling_mode(&mut app, KeyAction::Quit).unwrap();
    assert!(app.data.should_quit);
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));
    app.data.should_quit = false;

    dispatch_scrolling_mode(&mut app, KeyAction::SelectProjectHeader).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));
    dispatch_scrolling_mode(&mut app, KeyAction::SelectProjectFirstAgent).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

    dispatch_scrolling_mode(&mut app, KeyAction::SwitchTab).unwrap();
    dispatch_scrolling_mode(&mut app, KeyAction::NextAgent).unwrap();
    dispatch_scrolling_mode(&mut app, KeyAction::PrevAgent).unwrap();
    dispatch_scrolling_mode(&mut app, KeyAction::ScrollDown).unwrap();
    dispatch_scrolling_mode(&mut app, KeyAction::ScrollTop).unwrap();
    dispatch_scrolling_mode(&mut app, KeyAction::ScrollBottom).unwrap();

    dispatch_scrolling_mode(&mut app, KeyAction::DiffRedo).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

    dispatch_scrolling_mode(&mut app, KeyAction::FocusPreview).unwrap();
    assert!(mode_is_preview_or_diff(&app.mode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Kill).unwrap();
    assert!(mode_is_confirming(&app.mode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Synthesize).unwrap();
    assert!(mode_is_confirming(&app.mode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::ToggleSynthesisMark).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

    dispatch_scrolling_mode(&mut app, KeyAction::ToggleCollapse).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

    dispatch_scrolling_mode(&mut app, KeyAction::SpawnChildren).unwrap();
    assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::PlanSwarm).unwrap();
    assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::AddChildren).unwrap();
    assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::ReviewSwarm).unwrap();
    assert_eq!(app.mode, AppMode::ReviewChildCount(ReviewChildCountMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Broadcast).unwrap();
    assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::SpawnTerminalPrompted).unwrap();
    assert_eq!(app.mode, AppMode::TerminalPrompt(TerminalPromptMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Push).unwrap();
    assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::RenameBranch).unwrap();
    assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Rebase).unwrap();
    assert_eq!(
        app.mode,
        AppMode::RebaseBranchSelector(RebaseBranchSelectorMode)
    );
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Merge).unwrap();
    assert_eq!(
        app.mode,
        AppMode::MergeBranchSelector(MergeBranchSelectorMode)
    );
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::SwitchBranch).unwrap();
    assert_eq!(
        app.mode,
        AppMode::SwitchBranchSelector(SwitchBranchSelectorMode)
    );
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Help).unwrap();
    assert_eq!(app.mode, AppMode::Help(HelpMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::CommandPalette).unwrap();
    assert_eq!(app.mode, AppMode::CommandPalette(CommandPaletteMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::NewAgent).unwrap();
    assert_eq!(app.mode, AppMode::Creating(CreatingMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::NewAgentWithPrompt).unwrap();
    assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
    reset_to_scrolling(&mut app);

    dispatch_scrolling_mode(&mut app, KeyAction::Cancel).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_scrolling_mode_open_pr_errors_when_not_git_workspace() {
    let (mut app, _temp) = create_test_app();
    let temp_dir = TempDir::new().unwrap();
    let mut agent = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "root".to_string(),
        temp_dir.path().to_path_buf(),
    );
    agent.workspace_kind = WorkspaceKind::PlainDir;
    app.data.storage.add(agent);
    assert!(app.data.selected_agent().is_some());
    app.enter_mode(ScrollingMode.into());

    dispatch_scrolling_mode(&mut app, KeyAction::OpenPR).unwrap();
    assert!(mode_is_error_modal(&app.mode));
}

#[test]
fn test_diff_focused_mode_raw_dispatch_routes_keys() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);

    app.data.active_tab = crate::app::Tab::Diff;
    app.data.ui.set_preview_dimensions(80, 1);
    let diff_content = (0..64)
        .map(|idx| format!("line-{idx}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    app.data.ui.set_diff_content(diff_content);
    app.enter_mode(DiffFocusedMode.into());

    dispatch_diff_focused_mode(&mut app, KeyCode::Down, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.ui.diff_cursor, 1);
    assert!(mode_is_diff_focused(&app.mode));

    dispatch_diff_focused_mode(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL).unwrap();
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Nothing to undo")
    );
    assert!(mode_is_diff_focused(&app.mode));

    dispatch_diff_focused_mode(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL).unwrap();
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Nothing to redo")
    );
    assert!(mode_is_diff_focused(&app.mode));

    app.data.ui.diff_scroll = 10;
    app.data.ui.diff_cursor = 10;
    dispatch_diff_focused_mode(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL).unwrap();
    assert!(mode_is_diff_focused(&app.mode));
    assert_eq!(app.data.ui.diff_scroll, 5);

    dispatch_diff_focused_mode(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL).unwrap();
    assert!(mode_is_diff_focused(&app.mode));
    assert_eq!(app.data.ui.diff_scroll, 10);

    dispatch_diff_focused_mode(&mut app, KeyCode::Char('G'), KeyModifiers::NONE).unwrap();
    assert!(mode_is_diff_focused(&app.mode));
    assert!(app.data.ui.diff_scroll > 0);

    dispatch_diff_focused_mode(&mut app, KeyCode::Char(' '), KeyModifiers::NONE).unwrap();
    assert!(mode_is_diff_focused(&app.mode));

    // Unhandled actions should fall back to normal-mode dispatch.
    dispatch_diff_focused_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE).unwrap();
    assert!(mode_is_creating(&app.mode));

    app.enter_mode(DiffFocusedMode.into());
    dispatch_diff_focused_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE).unwrap();
    assert!(mode_is_diff_focused(&app.mode));

    app.data.active_tab = crate::app::Tab::Diff;
    app.enter_mode(DiffFocusedMode.into());
    dispatch_diff_focused_mode(&mut app, KeyCode::Tab, KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.active_tab, crate::app::Tab::Diff);
    assert!(mode_is_diff_focused(&app.mode));

    app.enter_mode(DiffFocusedMode.into());
    dispatch_diff_focused_mode(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_diff_focused_mode_raw_dispatch_plain_q_falls_back_to_normal_dispatch() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);

    app.data.active_tab = crate::app::Tab::Diff;
    app.data.ui.set_preview_dimensions(80, 1);
    app.data.ui.set_diff_content("line\n");
    app.enter_mode(DiffFocusedMode.into());

    dispatch_diff_focused_mode(&mut app, KeyCode::Char('q'), KeyModifiers::NONE).unwrap();

    assert!(!app.data.should_quit);
    assert!(mode_is_diff_focused(&app.mode));
}

#[test]
fn test_diff_focused_mode_raw_dispatch_ignores_unbound_keys() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);

    app.data.active_tab = crate::app::Tab::Diff;
    app.data.ui.set_preview_dimensions(80, 1);
    app.data.ui.set_diff_content("line\n");
    app.enter_mode(DiffFocusedMode.into());

    let cursor = app.data.ui.diff_cursor;
    let anchor = app.data.ui.diff_visual_anchor;
    dispatch_diff_focused_mode(&mut app, KeyCode::Char('~'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.data.ui.diff_cursor, cursor);
    assert_eq!(app.data.ui.diff_visual_anchor, anchor);
    assert!(mode_is_diff_focused(&app.mode));
}

#[test]
fn test_diff_focused_mode_raw_dispatch_propagates_action_errors() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);

    app.data.active_tab = crate::app::Tab::Diff;
    app.data.ui.diff_undo.push(crate::app::DiffEdit {
        patch: "not a patch".to_string(),
        applied_reverse: false,
    });
    app.enter_mode(DiffFocusedMode.into());

    assert!(
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL).is_err()
    );
    assert_eq!(app.data.ui.diff_undo.len(), 1);
    assert!(mode_is_diff_focused(&app.mode));
}

#[test]
fn test_diff_focused_mode_raw_dispatch_propagates_infallible_action_errors() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);
    app.data.active_tab = crate::app::Tab::Diff;
    app.enter_mode(DiffFocusedMode.into());

    let _guard = force_infallible_action_error_for_tests();
    for (code, mods) in [
        (KeyCode::Char('q'), KeyModifiers::CONTROL),
        (KeyCode::Up, KeyModifiers::NONE),
        (KeyCode::Down, KeyModifiers::NONE),
    ] {
        let err = dispatch_diff_focused_mode(&mut app, code, mods)
            .expect_err("expected forced diff focused dispatch error");
        assert!(
            err.to_string()
                .contains("forced infallible action error for test")
        );
    }
}

#[test]
fn test_dispatch_confirming_mode_worktree_conflict_d_routes_to_recreate_action() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);
    let result = dispatch_confirming_mode(
        &mut app,
        ConfirmAction::WorktreeConflict,
        KeyCode::Char('d'),
    );
    assert!(result.is_err());
}

#[test]
fn test_dispatch_picker_modes_cover_enter_and_fallback_cases() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);
    let settings_path = NamedTempFile::new().unwrap().into_temp_path();
    Settings::set_test_path_override(settings_path.to_path_buf()).unwrap();

    app.enter_mode(RebaseBranchSelectorMode.into());
    dispatch_rebase_branch_selector_mode(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(
        app.mode,
        AppMode::RebaseBranchSelector(RebaseBranchSelectorMode)
    );

    app.enter_mode(MergeBranchSelectorMode.into());
    dispatch_merge_branch_selector_mode(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(
        app.mode,
        AppMode::MergeBranchSelector(MergeBranchSelectorMode)
    );

    app.enter_mode(SwitchBranchSelectorMode.into());
    dispatch_switch_branch_selector_mode(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(
        app.mode,
        AppMode::SwitchBranchSelector(SwitchBranchSelectorMode)
    );

    app.enter_mode(ModelSelectorMode.into());
    dispatch_model_selector_mode(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(app.mode, AppMode::normal());

    app.enter_mode(SettingsMenuMode.into());
    dispatch_settings_menu_mode(&mut app, KeyCode::Enter).unwrap();
    assert_eq!(app.mode, AppMode::ModelSelector(ModelSelectorMode));

    app.enter_mode(SettingsMenuMode.into());
    dispatch_settings_menu_mode(&mut app, KeyCode::Char('x')).unwrap();
    assert_eq!(app.mode, AppMode::SettingsMenu(SettingsMenuMode));

    app.enter_mode(CommandPaletteMode.into());
    dispatch_command_palette_mode(&mut app, KeyCode::Delete).unwrap();
    assert!(mode_is_command_palette(&app.mode));

    app.enter_mode(CommandPaletteMode.into());
    dispatch_command_palette_mode(&mut app, KeyCode::F(1)).unwrap();
    assert_eq!(app.mode, AppMode::CommandPalette(CommandPaletteMode));
}

#[test]
fn test_normal_mode_typed_dispatch_covers_more_actions() {
    let (mut app, _temp) = create_test_app();
    add_agent_with_child(&mut app);

    dispatch_normal_mode(&mut app, KeyAction::SelectProjectHeader).unwrap();
    assert_eq!(app.mode, AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::SelectProjectFirstAgent).unwrap();
    assert_eq!(app.mode, AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::ScrollTop).unwrap();
    assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));
    app.apply_mode(AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::PlanSwarm).unwrap();
    assert!(mode_is_child_count_or_normal(&app.mode));
    app.apply_mode(AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::SwitchBranch).unwrap();
    assert!(mode_is_error_modal_or_switch_branch_selector(&app.mode));
    app.apply_mode(AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::Rebase).unwrap();
    assert_eq!(
        app.mode,
        AppMode::RebaseBranchSelector(RebaseBranchSelectorMode)
    );
    app.apply_mode(AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::Merge).unwrap();
    assert_eq!(
        app.mode,
        AppMode::MergeBranchSelector(MergeBranchSelectorMode)
    );
    app.apply_mode(AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::ToggleSynthesisMark).unwrap();
    assert_eq!(app.mode, AppMode::normal());

    dispatch_normal_mode(&mut app, KeyAction::CommandPalette).unwrap();
    assert_eq!(app.mode, AppMode::CommandPalette(CommandPaletteMode));
}

#[test]
fn test_dispatch_normal_mode_propagates_execute_errors() {
    let (mut app, _temp) = create_test_app();
    assert!(dispatch_normal_mode(&mut app, KeyAction::Push).is_err());
}

#[test]
fn test_dispatch_scrolling_mode_propagates_execute_errors() {
    let (mut app, _temp) = create_test_app();
    app.enter_mode(ScrollingMode.into());
    assert!(dispatch_scrolling_mode(&mut app, KeyAction::Push).is_err());
}

#[test]
fn test_dispatch_picker_and_help_modes_propagate_execute_errors() {
    let (mut app, _temp) = create_test_app();

    app.enter_mode(ChildCountMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| dispatch_child_count_mode(
            &mut app,
            KeyCode::Esc
        ))
        .is_err()
    );

    app.enter_mode(ReviewChildCountMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_review_child_count_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(ModelSelectorMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_model_selector_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(SettingsMenuMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_settings_menu_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(CommandPaletteMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_command_palette_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(BranchSelectorMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_branch_selector_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(RebaseBranchSelectorMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_rebase_branch_selector_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(MergeBranchSelectorMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_merge_branch_selector_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(SwitchBranchSelectorMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_switch_branch_selector_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(ReviewInfoMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| dispatch_review_info_mode(&mut app)).is_err()
    );

    app.enter_mode(HelpMode.into());
    assert!(
        with_forced_picker_action_error_for_tests(|| {
            dispatch_help_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)
        })
        .is_err()
    );
}

#[test]
fn test_dispatch_confirm_related_modes_propagate_execute_errors() {
    let (mut app, _temp) = create_test_app();

    app.enter_mode(RenameBranchMode.into());
    assert!(
        with_forced_confirm_action_error_for_tests(|| {
            dispatch_rename_branch_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    app.enter_mode(KeyboardRemapPromptMode.into());
    assert!(
        with_forced_confirm_action_error_for_tests(|| {
            dispatch_keyboard_remap_prompt_mode(&mut app, KeyCode::Esc)
        })
        .is_err()
    );

    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(1, 0, 1),
    };
    app.enter_mode(UpdatePromptMode { info: info.clone() }.into());
    assert!(
        with_forced_confirm_action_error_for_tests(|| {
            dispatch_update_prompt_mode(&mut app, &info, KeyCode::Esc)
        })
        .is_err()
    );
}

#[test]
fn test_dispatch_confirming_mode_propagates_execute_errors_for_kill_yes_when_storage_save_fails() {
    let temp_dir = TempDir::new().unwrap();
    let storage = Storage::with_path(temp_dir.path().to_path_buf());
    let mut app = App::new(Config::default(), storage, Settings::default(), false);

    let repo_root = TempDir::new().unwrap();
    let mut root = Agent::new(
        "root".to_string(),
        "bash".to_string(),
        "feature".to_string(),
        PathBuf::from("/tmp"),
    );
    root.repo_root = Some(repo_root.path().to_path_buf());
    app.data.storage.add(root);
    app.data.selected = 1;

    assert!(dispatch_confirming_mode(&mut app, ConfirmAction::Kill, KeyCode::Char('y')).is_err());
}

#[test]
fn test_dispatch_help_mode_ctrl_shortcuts_require_control_modifier() {
    let (mut app, _temp) = create_test_app();
    app.enter_mode(HelpMode.into());

    dispatch_help_mode(&mut app, KeyCode::Char('u'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.mode, AppMode::normal());

    app.enter_mode(HelpMode.into());
    dispatch_help_mode(&mut app, KeyCode::Char('d'), KeyModifiers::NONE).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_dispatch_changelog_mode_scroll_and_dismiss() {
    let (mut app, _temp) = create_test_app();
    app.mode = AppMode::Changelog(crate::state::ChangelogMode {
        title: "Changelog".to_string(),
        lines: vec!["Line".to_string()],
        mark_seen_version: None,
    });

    let max_scroll = 5usize;
    app.data.ui.changelog_scroll = 100;

    let cases = [
        (KeyCode::Up, KeyModifiers::NONE, 4),
        (KeyCode::Down, KeyModifiers::NONE, 5),
        (KeyCode::Down, KeyModifiers::NONE, 5),
        (KeyCode::PageUp, KeyModifiers::NONE, 0),
        (KeyCode::PageDown, KeyModifiers::NONE, 5),
        (KeyCode::Char('u'), KeyModifiers::CONTROL, 0),
        (KeyCode::Char('d'), KeyModifiers::CONTROL, 5),
        (KeyCode::Char('g'), KeyModifiers::NONE, 0),
        (KeyCode::Char('G'), KeyModifiers::NONE, 5),
        (KeyCode::Char('x'), KeyModifiers::NONE, 5),
    ];
    for (code, mods, expected) in cases {
        dispatch_changelog_and_assert_scroll(&mut app, max_scroll, code, mods, expected);
    }

    for code in [KeyCode::Char('u'), KeyCode::Char('d')] {
        dispatch_changelog_mode(&mut app, None, max_scroll, code, KeyModifiers::NONE).unwrap();
    }

    app.enter_mode(AppMode::Changelog(crate::state::ChangelogMode {
        title: "Changelog".to_string(),
        lines: vec!["Line".to_string()],
        mark_seen_version: None,
    }));

    dispatch_changelog_mode(
        &mut app,
        Some(Version::new(1, 2, 3)),
        max_scroll,
        KeyCode::Esc,
        KeyModifiers::NONE,
    )
    .unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_dispatch_changelog_mode_warns_when_seen_version_save_fails_with_tracing_enabled() {
    let (mut app, _temp) = create_test_app();

    app.enter_mode(AppMode::Changelog(crate::state::ChangelogMode {
        title: "Changelog".to_string(),
        lines: vec!["Line".to_string()],
        mark_seen_version: None,
    }));

    with_tracing_dispatch(|| {
        dispatch_changelog_mode(
            &mut app,
            Some(Version::new(1, 0, 0)),
            0,
            KeyCode::Esc,
            KeyModifiers::NONE,
        )
    })
    .unwrap();

    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_dispatch_changelog_mode_dismiss_saves_seen_version_when_settings_writable() {
    let (mut app, _temp) = create_test_app();

    let settings_path = NamedTempFile::new().unwrap().into_temp_path();
    Settings::set_test_path_override(settings_path.to_path_buf()).unwrap();

    app.mode = AppMode::Changelog(crate::state::ChangelogMode {
        title: "Changelog".to_string(),
        lines: vec!["Line".to_string()],
        mark_seen_version: None,
    });

    dispatch_changelog_mode(
        &mut app,
        Some(Version::new(9, 9, 9)),
        0,
        KeyCode::Esc,
        KeyModifiers::NONE,
    )
    .unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_dispatch_error_modal_mode_propagates_dismiss_errors() {
    let (mut app, _temp) = create_test_app();

    let _guard = force_infallible_action_error_for_tests();
    let err = dispatch_error_modal_mode(&mut app, "boom".to_string())
        .expect_err("expected forced dismiss error");
    assert!(
        err.to_string()
            .contains("forced infallible action error for test")
    );
}

#[test]
fn test_dispatch_success_modal_mode_propagates_dismiss_errors() {
    let (mut app, _temp) = create_test_app();

    let _guard = force_infallible_action_error_for_tests();
    let err = dispatch_success_modal_mode(&mut app, "ok".to_string())
        .expect_err("expected forced dismiss error");
    assert!(
        err.to_string()
            .contains("forced infallible action error for test")
    );
}

#[test]
fn test_dispatch_preview_focused_mode_forwards_tab_keys() {
    let (mut app, _temp) = create_test_app();
    app.data.active_tab = crate::app::Tab::Preview;
    app.enter_mode(PreviewFocusedMode.into());
    let mut keys = Vec::new();

    let tab_result =
        dispatch_preview_focused_mode(&mut app, KeyCode::Tab, KeyModifiers::NONE, &mut keys);
    let backtab_result =
        dispatch_preview_focused_mode(&mut app, KeyCode::BackTab, KeyModifiers::NONE, &mut keys);

    assert!(tab_result.is_ok());
    assert!(backtab_result.is_ok());
    assert_eq!(keys, vec!["\t".to_string(), "\u{1b}[Z".to_string()]);
    assert_eq!(app.data.active_tab, crate::app::Tab::Preview);
    assert_eq!(app.mode, AppMode::PreviewFocused(PreviewFocusedMode));
}

#[test]
fn test_dispatch_preview_focused_mode_ctrl_c_variants_cover_guard_false_branches() {
    let (mut app, _temp) = create_test_app();

    let mut keys = Vec::new();
    app.enter_mode(PreviewFocusedMode.into());
    dispatch_preview_focused_mode(&mut app, KeyCode::Char('c'), KeyModifiers::NONE, &mut keys)
        .unwrap();

    app.enter_mode(PreviewFocusedMode.into());
    dispatch_preview_focused_mode(
        &mut app,
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        &mut keys,
    )
    .unwrap();

    let temp_dir = TempDir::new().unwrap();
    let terminal = Agent::new(
        "term".to_string(),
        "terminal".to_string(),
        "root".to_string(),
        temp_dir.path().to_path_buf(),
    );
    app.data.storage.add(terminal);
    app.data.selected = 1;
    app.enter_mode(PreviewFocusedMode.into());
    dispatch_preview_focused_mode(
        &mut app,
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        &mut keys,
    )
    .unwrap();

    app.enter_mode(PreviewFocusedMode.into());
    dispatch_preview_focused_mode(&mut app, KeyCode::Char('q'), KeyModifiers::NONE, &mut keys)
        .unwrap();
}

#[test]
fn test_dispatch_preview_focused_mode_propagates_action_errors() {
    let (mut app, _temp) = create_test_app();
    let mut keys = Vec::new();
    app.enter_mode(PreviewFocusedMode.into());

    let _guard = force_infallible_action_error_for_tests();
    let err = dispatch_preview_focused_mode(
        &mut app,
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
        &mut keys,
    )
    .expect_err("expected forced unfocus error");
    assert!(
        err.to_string()
            .contains("forced infallible action error for test")
    );

    let err =
        dispatch_preview_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE, &mut keys)
            .expect_err("expected forced forward keystroke error");
    assert!(
        err.to_string()
            .contains("forced infallible action error for test")
    );
}

#[test]
fn test_dispatch_confirming_mode_worktree_conflict_r_enters_reconnect_prompt_and_prefills_input() {
    let (mut app, _temp) = create_test_app();
    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "conflict".to_string(),
        prompt: Some("hello".to_string()),
        branch: "tenex/conflict".to_string(),
        worktree_path: TempDir::new().unwrap().path().to_path_buf(),
        repo_root: TempDir::new().unwrap().path().to_path_buf(),
        existing_branch: Some("tenex/conflict".to_string()),
        existing_commit: Some("abc1234".to_string()),
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: None,
    });

    dispatch_confirming_mode(
        &mut app,
        ConfirmAction::WorktreeConflict,
        KeyCode::Char('r'),
    )
    .unwrap();
    assert!(mode_is_reconnect_prompt(&app.mode));
    assert_eq!(app.data.input.buffer, "hello");
}

#[test]
fn test_dispatch_confirming_mode_worktree_conflict_esc_cancels_and_clears_conflict() {
    let (mut app, _temp) = create_test_app();
    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "conflict".to_string(),
        prompt: None,
        branch: "tenex/conflict".to_string(),
        worktree_path: TempDir::new().unwrap().path().to_path_buf(),
        repo_root: TempDir::new().unwrap().path().to_path_buf(),
        existing_branch: None,
        existing_commit: None,
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: None,
    });

    dispatch_confirming_mode(&mut app, ConfirmAction::WorktreeConflict, KeyCode::Esc).unwrap();
    assert_eq!(app.mode, AppMode::normal());
    assert!(app.data.spawn.worktree_conflict.is_none());
}

#[test]
fn test_dispatch_confirming_mode_kill_yes_noops_when_no_agent_selected() {
    let (mut app, _temp) = create_test_app();
    dispatch_confirming_mode(&mut app, ConfirmAction::Kill, KeyCode::Char('y')).unwrap();
    assert_eq!(app.mode, AppMode::normal());
}

#[test]
fn test_dispatch_confirm_push_mode_yes_propagates_spawn_errors() {
    let (mut app, _temp) = create_test_app();
    let temp = NamedTempFile::new().unwrap();
    let agent = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature".to_string(),
        temp.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data.git_op.start_push(agent_id, "feature".to_string());

    assert!(dispatch_confirm_push_mode(&mut app, KeyCode::Char('y')).is_err());
}

#[test]
fn test_dispatch_confirm_push_for_pr_mode_yes_propagates_spawn_errors() {
    let (mut app, _temp) = create_test_app();
    let temp = NamedTempFile::new().unwrap();
    let agent = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature".to_string(),
        temp.path().to_path_buf(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);
    app.data
        .git_op
        .start_open_pr(agent_id, "feature".to_string(), "main".to_string(), true);

    assert!(dispatch_confirm_push_for_pr_mode(&mut app, KeyCode::Char('y')).is_err());
}

#[test]
fn test_dispatch_rename_branch_mode_enter_returns_error_modal_when_rename_state_missing() {
    let (mut app, _temp) = create_test_app();
    app.enter_mode(RenameBranchMode.into());
    app.data.input.buffer = "new-name".to_string();
    app.data.input.cursor = app.data.input.buffer.len();

    dispatch_rename_branch_mode(&mut app, KeyCode::Enter).unwrap();
    assert!(mode_is_error_modal(&app.mode));
}
