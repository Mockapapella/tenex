use crate::action::ValidIn;
use crate::app::{AppData, Tab};
use crate::state::{AppMode, DiffFocusedMode, NormalMode, PreviewFocusedMode, ScrollingMode};
use anyhow::Result;

/// Normal-mode action: switch the detail pane tab (Preview/Diff).
#[derive(Debug, Clone, Copy, Default)]
pub struct SwitchTabAction;

impl ValidIn<NormalMode> for SwitchTabAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SwitchTabAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for SwitchTabAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.switch_tab();
        Ok(AppMode::normal())
    }
}

/// Normal-mode action: select the next agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct NextAgentAction;

impl ValidIn<NormalMode> for NextAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_next();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for NextAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_next();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: select the previous agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrevAgentAction;

impl ValidIn<NormalMode> for PrevAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_prev();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for PrevAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_prev();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: highlight the selected agent's project header.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectProjectHeaderAction;

impl ValidIn<NormalMode> for SelectProjectHeaderAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_project_header();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SelectProjectHeaderAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_project_header();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: highlight the first agent under the selected project.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectProjectFirstAgentAction;

impl ValidIn<NormalMode> for SelectProjectFirstAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_first_agent_in_selected_project();
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SelectProjectFirstAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.select_first_agent_in_selected_project();
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: scroll up in the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollUpAction;

impl ValidIn<NormalMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollUpAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_up(5);
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: scroll down in the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollDownAction;

impl ValidIn<NormalMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollDownAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_down(5);
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: scroll to the top of the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollTopAction;

impl ValidIn<NormalMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollTopAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_top();
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: scroll to the bottom of the active view.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollBottomAction;

impl ValidIn<NormalMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<ScrollingMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ScrollBottomAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.scroll_to_bottom(10000, 0);
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: focus the preview pane (forward keys to mux).
#[derive(Debug, Clone, Copy, Default)]
pub struct FocusPreviewAction;

impl ValidIn<NormalMode> for FocusPreviewAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            match app_data.active_tab {
                Tab::Preview => Ok(PreviewFocusedMode.into()),
                Tab::Diff => Ok(DiffFocusedMode.into()),
                Tab::Commits => Ok(AppMode::normal()),
            }
        } else {
            Ok(AppMode::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for FocusPreviewAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            match app_data.active_tab {
                Tab::Preview => Ok(PreviewFocusedMode.into()),
                Tab::Diff => Ok(DiffFocusedMode.into()),
                Tab::Commits => Ok(ScrollingMode.into()),
            }
        } else {
            Ok(ScrollingMode.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_data() -> Result<(AppData, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            AppData::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
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
    fn test_switch_tab_action_toggles_tabs() -> Result<(), Box<dyn std::error::Error>> {
        let (mut data, _temp) = create_test_data()?;

        data.active_tab = Tab::Preview;
        assert_eq!(
            SwitchTabAction.execute(NormalMode, &mut data)?,
            AppMode::normal()
        );
        assert_eq!(data.active_tab, Tab::Diff);

        assert_eq!(
            SwitchTabAction.execute(ScrollingMode, &mut data)?,
            ScrollingMode.into()
        );
        assert_eq!(data.active_tab, Tab::Commits);

        assert_eq!(
            SwitchTabAction.execute(ScrollingMode, &mut data)?,
            ScrollingMode.into()
        );
        assert_eq!(data.active_tab, Tab::Preview);

        data.active_tab = Tab::Diff;
        assert_eq!(
            SwitchTabAction.execute(DiffFocusedMode, &mut data)?,
            AppMode::normal()
        );
        assert_eq!(data.active_tab, Tab::Commits);

        Ok(())
    }

    #[test]
    fn test_next_prev_agent_actions_update_selection() -> Result<(), Box<dyn std::error::Error>> {
        let (mut data, _temp) = create_test_data()?;

        assert_eq!(data.selected, 1);
        assert_eq!(
            NextAgentAction.execute(NormalMode, &mut data)?,
            AppMode::normal()
        );
        assert_eq!(data.selected, 1);

        add_two_agents(&mut data);
        assert_eq!(data.selected, 1);
        assert_eq!(
            NextAgentAction.execute(NormalMode, &mut data)?,
            AppMode::normal()
        );
        assert_eq!(data.selected, 2);
        assert_eq!(
            PrevAgentAction.execute(ScrollingMode, &mut data)?,
            ScrollingMode.into()
        );
        assert_eq!(data.selected, 1);
        Ok(())
    }

    #[test]
    fn test_scroll_actions_respect_active_tab() -> Result<(), Box<dyn std::error::Error>> {
        let (mut data, _temp) = create_test_data()?;

        data.active_tab = Tab::Preview;
        data.ui.set_preview_content("line1\nline2\nline3\n");
        data.ui.preview_dimensions = Some((80, 1));
        data.ui.preview_scroll = usize::MAX;
        data.ui.preview_follow = true;
        assert_eq!(
            ScrollUpAction.execute(NormalMode, &mut data)?,
            ScrollingMode.into()
        );
        assert!(!data.ui.preview_follow);

        data.active_tab = Tab::Diff;
        data.ui.set_diff_content("a\nb\nc\nd\ne\n");
        assert_eq!(
            ScrollDownAction.execute(DiffFocusedMode, &mut data)?,
            DiffFocusedMode.into()
        );

        assert_eq!(
            ScrollTopAction.execute(ScrollingMode, &mut data)?,
            ScrollingMode.into()
        );
        assert_eq!(data.ui.diff_scroll, 0);

        assert_eq!(
            ScrollBottomAction.execute(DiffFocusedMode, &mut data)?,
            DiffFocusedMode.into()
        );
        assert!(data.ui.diff_cursor > 0);
        Ok(())
    }

    #[test]
    fn test_scroll_up_does_not_pause_preview_when_not_scrollable()
    -> Result<(), Box<dyn std::error::Error>> {
        // Regression: When the preview buffer has no scrollback (common for full-screen/alt-screen
        // TUIs like Codex early in a session), a scroll-up gesture shouldn't flip follow off.
        // Otherwise Tenex looks "paused" even though there is nothing to scroll.
        let (mut data, _temp) = create_test_data()?;

        data.active_tab = Tab::Preview;
        data.ui.set_preview_content("line1\nline2\nline3\n");
        data.ui.preview_dimensions = Some((80, 10));
        data.ui.preview_scroll = usize::MAX;
        data.ui.preview_follow = true;

        assert_eq!(
            ScrollUpAction.execute(NormalMode, &mut data)?,
            ScrollingMode.into()
        );
        assert!(data.ui.preview_follow);

        Ok(())
    }

    #[test]
    fn test_focus_preview_action_enters_correct_focus_mode()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut data, _temp) = create_test_data()?;

        data.active_tab = Tab::Preview;
        assert_eq!(
            FocusPreviewAction.execute(NormalMode, &mut data)?,
            AppMode::normal()
        );

        add_two_agents(&mut data);
        data.active_tab = Tab::Preview;
        assert_eq!(
            FocusPreviewAction.execute(NormalMode, &mut data)?,
            PreviewFocusedMode.into()
        );

        data.active_tab = Tab::Diff;
        assert_eq!(
            FocusPreviewAction.execute(ScrollingMode, &mut data)?,
            DiffFocusedMode.into()
        );

        let (mut data, _temp) = create_test_data()?;
        data.active_tab = Tab::Diff;
        assert_eq!(
            FocusPreviewAction.execute(ScrollingMode, &mut data)?,
            ScrollingMode.into()
        );
        Ok(())
    }
}
