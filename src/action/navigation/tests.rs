use super::*;
use crate::agent::{Agent, Storage};
use crate::app::Settings;
use crate::config::Config;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn create_test_data() -> (AppData, NamedTempFile) {
    let temp_file = NamedTempFile::new().unwrap();
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        AppData::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

fn add_two_agents(data: &mut AppData) {
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp"),
    ));
    data.storage.add(Agent::new(
        "second".to_string(),
        "claude".to_string(),
        "tenex/second".to_string(),
        PathBuf::from("/tmp"),
    ));
}

#[test]
fn test_switch_tab_action_toggles_tabs() {
    let (mut data, _temp) = create_test_data();

    data.active_tab = Tab::Preview;
    assert_eq!(
        SwitchTabAction.execute(NormalMode, &mut data).unwrap(),
        AppMode::normal()
    );
    assert_eq!(data.active_tab, Tab::Diff);

    assert_eq!(
        SwitchTabAction.execute(ScrollingMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.active_tab, Tab::Commits);

    assert_eq!(
        SwitchTabAction.execute(ScrollingMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.active_tab, Tab::Preview);

    data.active_tab = Tab::Diff;
    assert_eq!(
        SwitchTabAction.execute(DiffFocusedMode, &mut data).unwrap(),
        AppMode::normal()
    );
    assert_eq!(data.active_tab, Tab::Commits);
}

#[test]
fn test_next_prev_agent_actions_update_selection() {
    let (mut data, _temp) = create_test_data();

    assert_eq!(data.selected, 1);
    assert_eq!(
        NextAgentAction.execute(NormalMode, &mut data).unwrap(),
        AppMode::normal()
    );
    assert_eq!(data.selected, 1);

    add_two_agents(&mut data);
    assert_eq!(data.selected, 1);
    assert_eq!(
        NextAgentAction.execute(NormalMode, &mut data).unwrap(),
        AppMode::normal()
    );
    assert_eq!(data.selected, 2);
    assert_eq!(
        PrevAgentAction.execute(ScrollingMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.selected, 1);
}

#[test]
fn test_project_header_selection_actions() {
    let (mut data, _temp) = create_test_data();
    add_two_agents(&mut data);
    assert_eq!(data.selected, 1);

    assert_eq!(
        SelectProjectHeaderAction
            .execute(ScrollingMode, &mut data)
            .unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.selected, 0);

    assert_eq!(
        SelectProjectFirstAgentAction
            .execute(NormalMode, &mut data)
            .unwrap(),
        AppMode::normal()
    );
    assert_eq!(data.selected, 1);

    assert_eq!(
        SelectProjectHeaderAction
            .execute(NormalMode, &mut data)
            .unwrap(),
        AppMode::normal()
    );
    assert_eq!(data.selected, 0);

    assert_eq!(
        SelectProjectFirstAgentAction
            .execute(ScrollingMode, &mut data)
            .unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.selected, 1);
}

#[test]
fn test_scroll_actions_respect_active_tab() {
    let (mut data, _temp) = create_test_data();

    data.active_tab = Tab::Preview;
    data.ui.set_preview_content("line1\nline2\nline3\n");
    data.ui.preview_dimensions = Some((80, 1));
    data.ui.preview_scroll = usize::MAX;
    data.ui.preview_follow = true;
    assert_eq!(
        ScrollUpAction.execute(NormalMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert!(!data.ui.preview_follow);

    data.active_tab = Tab::Diff;
    data.ui.set_diff_content("a\nb\nc\nd\ne\n");
    assert_eq!(
        ScrollDownAction
            .execute(DiffFocusedMode, &mut data)
            .unwrap(),
        DiffFocusedMode.into()
    );
    assert_eq!(
        ScrollUpAction.execute(DiffFocusedMode, &mut data).unwrap(),
        DiffFocusedMode.into()
    );

    assert_eq!(
        ScrollTopAction.execute(ScrollingMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.ui.diff_scroll, 0);

    assert_eq!(
        ScrollTopAction.execute(NormalMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert_eq!(data.ui.diff_scroll, 0);

    assert_eq!(
        ScrollBottomAction
            .execute(DiffFocusedMode, &mut data)
            .unwrap(),
        DiffFocusedMode.into()
    );
    assert!(data.ui.diff_cursor > 0);
}

#[test]
fn test_scroll_up_does_not_pause_preview_when_not_scrollable() {
    // Regression: When the preview buffer has no scrollback (common for full-screen/alt-screen
    // TUIs like Codex early in a session), a scroll-up gesture shouldn't flip follow off.
    // Otherwise Tenex looks "paused" even though there is nothing to scroll.
    let (mut data, _temp) = create_test_data();

    data.active_tab = Tab::Preview;
    data.ui.set_preview_content("line1\nline2\nline3\n");
    data.ui.preview_dimensions = Some((80, 10));
    data.ui.preview_scroll = usize::MAX;
    data.ui.preview_follow = true;

    assert_eq!(
        ScrollUpAction.execute(NormalMode, &mut data).unwrap(),
        ScrollingMode.into()
    );
    assert!(data.ui.preview_follow);
}

#[test]
fn test_focus_preview_action_enters_correct_focus_mode() {
    let (mut data, _temp) = create_test_data();

    data.active_tab = Tab::Preview;
    assert_eq!(
        FocusPreviewAction.execute(NormalMode, &mut data).unwrap(),
        AppMode::normal()
    );

    add_two_agents(&mut data);
    data.active_tab = Tab::Preview;
    assert_eq!(
        FocusPreviewAction.execute(NormalMode, &mut data).unwrap(),
        PreviewFocusedMode.into()
    );

    data.active_tab = Tab::Diff;
    assert_eq!(
        FocusPreviewAction.execute(NormalMode, &mut data).unwrap(),
        DiffFocusedMode.into()
    );
    data.active_tab = Tab::Commits;
    assert_eq!(
        FocusPreviewAction.execute(NormalMode, &mut data).unwrap(),
        AppMode::normal()
    );

    data.active_tab = Tab::Diff;
    assert_eq!(
        FocusPreviewAction
            .execute(ScrollingMode, &mut data)
            .unwrap(),
        DiffFocusedMode.into()
    );
    data.active_tab = Tab::Preview;
    assert_eq!(
        FocusPreviewAction
            .execute(ScrollingMode, &mut data)
            .unwrap(),
        PreviewFocusedMode.into()
    );
    data.active_tab = Tab::Commits;
    assert_eq!(
        FocusPreviewAction
            .execute(ScrollingMode, &mut data)
            .unwrap(),
        ScrollingMode.into()
    );

    let (mut data, _temp) = create_test_data();
    data.active_tab = Tab::Diff;
    assert_eq!(
        FocusPreviewAction
            .execute(ScrollingMode, &mut data)
            .unwrap(),
        ScrollingMode.into()
    );
}
