//! Compile-time action types (new architecture).

mod agent;
mod confirm;
mod diff;
mod git;
mod misc;
mod modal;
mod navigation;
mod picker;
mod preview;
mod text_input;

pub use agent::*;
pub use confirm::*;
pub use diff::*;
pub use git::*;
pub use misc::*;
pub(crate) use modal::changelog_max_scroll;
#[cfg(test)]
pub(crate) use modal::help_max_scroll;
pub use modal::{HalfPageDownAction, HalfPageUpAction, PageDownAction, PageUpAction};
pub use navigation::*;
pub use picker::*;
#[cfg(test)]
pub(crate) use preview::keycode_to_input_sequence;
pub use preview::{ForwardKeystrokeAction, UnfocusPreviewAction};
pub use text_input::*;

use crate::app::{App, AppData};
use crate::config::Action as KeyAction;
use crate::state::{
    AppMode, BranchSelectorMode, BroadcastingMode, ChildCountMode, ChildPromptMode,
    CommandPaletteMode, ConfirmAction, ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode,
    CreatingMode, CustomAgentCommandMode, DiffFocusedMode, ErrorModalMode, HelpMode,
    KeyboardRemapPromptMode, MergeBranchSelectorMode, ModelSelectorMode, NormalMode,
    PreviewFocusedMode, PromptingMode, RebaseBranchSelectorMode, ReconnectPromptMode,
    RenameBranchMode, ReviewChildCountMode, ReviewInfoMode, ScrollingMode, SettingsMenuMode,
    SuccessModalMode, SwitchBranchSelectorMode, SynthesisPromptMode, TerminalPromptMode,
    UpdatePromptMode,
};
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use semver::Version;
use tracing::warn;

#[cfg(any(test, coverage))]
thread_local! {
    static TEST_FORCE_INFAILLIBLE_ACTION_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(any(test, coverage))]
#[doc(hidden)]
#[derive(Debug)]
pub struct ForcedInfallibleActionErrorGuard {
    previous: bool,
}

#[cfg(any(test, coverage))]
impl Drop for ForcedInfallibleActionErrorGuard {
    fn drop(&mut self) {
        TEST_FORCE_INFAILLIBLE_ACTION_ERROR.with(|slot| slot.set(self.previous));
    }
}

#[cfg(any(test, coverage))]
#[doc(hidden)]
/// Force otherwise-infallible actions to return an error until the guard is dropped.
#[must_use]
pub fn force_infallible_action_error_for_tests() -> ForcedInfallibleActionErrorGuard {
    let previous = TEST_FORCE_INFAILLIBLE_ACTION_ERROR.with(|slot| slot.replace(true));
    ForcedInfallibleActionErrorGuard { previous }
}

#[cfg(any(test, coverage))]
fn force_infallible_action_error_if_enabled_for_tests() -> Result<()> {
    if TEST_FORCE_INFAILLIBLE_ACTION_ERROR.with(std::cell::Cell::get) {
        anyhow::bail!("forced infallible action error for test");
    }
    Ok(())
}

/// Marker trait: This action is valid in this state.
///
/// Each impl is an explicit entry in the "registry" of valid combinations.
pub trait ValidIn<State>: Sized {
    /// The next state produced after executing this action in `State`.
    type NextState;

    /// Execute this action in `State`, producing the next state.
    ///
    /// # Errors
    ///
    /// Returns an error if executing the action fails.
    fn execute(self, state: State, app_data: &mut AppData) -> Result<Self::NextState>;
}

/// Dispatch a legacy keybinding `Action` while in `NormalMode`, using the new
/// compile-time (State, Action) registry.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_normal_mode(app: &mut App, action: KeyAction) -> Result<()> {
    let app_data = &mut app.data;
    let next = match action {
        KeyAction::NewAgent => NewAgentAction.execute(NormalMode, app_data),
        KeyAction::NewAgentWithPrompt => NewAgentWithPromptAction.execute(NormalMode, app_data),
        KeyAction::Help => HelpAction.execute(NormalMode, app_data),
        KeyAction::Quit => QuitAction.execute(NormalMode, app_data),
        KeyAction::Kill => KillAction.execute(NormalMode, app_data),
        KeyAction::SwitchTab => SwitchTabAction.execute(NormalMode, app_data),
        KeyAction::NextAgent => NextAgentAction.execute(NormalMode, app_data),
        KeyAction::PrevAgent => PrevAgentAction.execute(NormalMode, app_data),
        KeyAction::SelectProjectHeader => SelectProjectHeaderAction.execute(NormalMode, app_data),
        KeyAction::SelectProjectFirstAgent => {
            SelectProjectFirstAgentAction.execute(NormalMode, app_data)
        }
        KeyAction::ScrollUp => ScrollUpAction.execute(NormalMode, app_data),
        KeyAction::ScrollDown => ScrollDownAction.execute(NormalMode, app_data),
        KeyAction::ScrollTop => ScrollTopAction.execute(NormalMode, app_data),
        KeyAction::ScrollBottom => ScrollBottomAction.execute(NormalMode, app_data),
        KeyAction::FocusPreview => FocusPreviewAction.execute(NormalMode, app_data),
        KeyAction::SpawnChildren => SpawnChildrenAction.execute(NormalMode, app_data),
        KeyAction::PlanSwarm => PlanSwarmAction.execute(NormalMode, app_data),
        KeyAction::AddChildren => AddChildrenAction.execute(NormalMode, app_data),
        KeyAction::Synthesize => SynthesizeAction.execute(NormalMode, app_data),
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(NormalMode, app_data),
        KeyAction::Broadcast => BroadcastAction.execute(NormalMode, app_data),
        KeyAction::ReviewSwarm => ReviewSwarmAction.execute(NormalMode, app_data),
        KeyAction::SpawnTerminal => SpawnTerminalAction.execute(NormalMode, app_data),
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction.execute(NormalMode, app_data)
        }
        KeyAction::Push => PushAction.execute(NormalMode, app_data),
        KeyAction::RenameBranch => RenameBranchAction.execute(NormalMode, app_data),
        KeyAction::OpenPR => OpenPRAction.execute(NormalMode, app_data),
        KeyAction::Rebase => RebaseAction.execute(NormalMode, app_data),
        KeyAction::Merge => MergeAction.execute(NormalMode, app_data),
        KeyAction::SwitchBranch => SwitchBranchAction.execute(NormalMode, app_data),
        KeyAction::CommandPalette => CommandPaletteAction.execute(NormalMode, app_data),
        KeyAction::Cancel => CancelAction.execute(NormalMode, app_data),

        // Not valid in Normal mode; treat as no-op.
        KeyAction::Confirm
        | KeyAction::UnfocusPreview
        | KeyAction::DiffCursorUp
        | KeyAction::DiffCursorDown
        | KeyAction::DiffToggleVisual
        | KeyAction::DiffDeleteLine
        | KeyAction::DiffUndo
        | KeyAction::DiffRedo => Ok(NormalMode.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a legacy keybinding `Action` while in `ScrollingMode`, using the new
/// compile-time (State, Action) registry.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_scrolling_mode(app: &mut App, action: KeyAction) -> Result<()> {
    let app_data = &mut app.data;
    let next = match action {
        KeyAction::NewAgent => NewAgentAction.execute(ScrollingMode, app_data),
        KeyAction::NewAgentWithPrompt => NewAgentWithPromptAction.execute(ScrollingMode, app_data),
        KeyAction::Help => HelpAction.execute(ScrollingMode, app_data),
        KeyAction::Quit => QuitAction.execute(ScrollingMode, app_data),
        KeyAction::Kill => KillAction.execute(ScrollingMode, app_data),
        KeyAction::SwitchTab => SwitchTabAction.execute(ScrollingMode, app_data),
        KeyAction::NextAgent => NextAgentAction.execute(ScrollingMode, app_data),
        KeyAction::PrevAgent => PrevAgentAction.execute(ScrollingMode, app_data),
        KeyAction::SelectProjectHeader => {
            SelectProjectHeaderAction.execute(ScrollingMode, app_data)
        }
        KeyAction::SelectProjectFirstAgent => {
            SelectProjectFirstAgentAction.execute(ScrollingMode, app_data)
        }
        KeyAction::ScrollUp => ScrollUpAction.execute(ScrollingMode, app_data),
        KeyAction::ScrollDown => ScrollDownAction.execute(ScrollingMode, app_data),
        KeyAction::ScrollTop => ScrollTopAction.execute(ScrollingMode, app_data),
        KeyAction::ScrollBottom => ScrollBottomAction.execute(ScrollingMode, app_data),
        KeyAction::FocusPreview => FocusPreviewAction.execute(ScrollingMode, app_data),
        KeyAction::SpawnChildren => SpawnChildrenAction.execute(ScrollingMode, app_data),
        KeyAction::PlanSwarm => PlanSwarmAction.execute(ScrollingMode, app_data),
        KeyAction::AddChildren => AddChildrenAction.execute(ScrollingMode, app_data),
        KeyAction::Synthesize => SynthesizeAction.execute(ScrollingMode, app_data),
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(ScrollingMode, app_data),
        KeyAction::Broadcast => BroadcastAction.execute(ScrollingMode, app_data),
        KeyAction::ReviewSwarm => ReviewSwarmAction.execute(ScrollingMode, app_data),
        KeyAction::SpawnTerminal => SpawnTerminalAction.execute(ScrollingMode, app_data),
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction.execute(ScrollingMode, app_data)
        }
        KeyAction::Push => PushAction.execute(ScrollingMode, app_data),
        KeyAction::RenameBranch => RenameBranchAction.execute(ScrollingMode, app_data),
        KeyAction::OpenPR => OpenPRAction.execute(ScrollingMode, app_data),
        KeyAction::Rebase => RebaseAction.execute(ScrollingMode, app_data),
        KeyAction::Merge => MergeAction.execute(ScrollingMode, app_data),
        KeyAction::SwitchBranch => SwitchBranchAction.execute(ScrollingMode, app_data),
        KeyAction::CommandPalette => CommandPaletteAction.execute(ScrollingMode, app_data),
        KeyAction::Cancel => CancelAction.execute(ScrollingMode, app_data),

        // Not valid in Scrolling mode; treat as no-op.
        KeyAction::Confirm
        | KeyAction::UnfocusPreview
        | KeyAction::DiffCursorUp
        | KeyAction::DiffCursorDown
        | KeyAction::DiffToggleVisual
        | KeyAction::DiffDeleteLine
        | KeyAction::DiffUndo
        | KeyAction::DiffRedo => Ok(ScrollingMode.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `HelpMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if executing the dispatched action fails.
pub fn dispatch_help_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    let next = {
        let app_data = &mut app.data;

        match (code, modifiers) {
            (KeyCode::Up, _) => ScrollUpAction.execute(HelpMode, app_data),
            (KeyCode::Down, _) => ScrollDownAction.execute(HelpMode, app_data),
            (KeyCode::PageUp, _) => PageUpAction.execute(HelpMode, app_data),
            (KeyCode::PageDown, _) => PageDownAction.execute(HelpMode, app_data),
            (KeyCode::Char('u'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                HalfPageUpAction.execute(HelpMode, app_data)
            }
            (KeyCode::Char('d'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                HalfPageDownAction.execute(HelpMode, app_data)
            }
            (KeyCode::Char('g') | KeyCode::Home, _) => ScrollTopAction.execute(HelpMode, app_data),
            (KeyCode::Char('G') | KeyCode::End, _) => {
                ScrollBottomAction.execute(HelpMode, app_data)
            }
            _ => DismissAction.execute(HelpMode, app_data),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ChangelogMode`.
///
/// # Errors
///
/// Returns an error if persisting settings fails.
pub fn dispatch_changelog_mode(
    app: &mut App,
    mark_seen_version: Option<Version>,
    max_scroll: usize,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    match (code, modifiers) {
        (KeyCode::Up, _) => {
            app.data.ui.changelog_scroll = app
                .data
                .ui
                .changelog_scroll
                .min(max_scroll)
                .saturating_sub(1);
        }
        (KeyCode::Down, _) => {
            app.data.ui.changelog_scroll = app
                .data
                .ui
                .changelog_scroll
                .min(max_scroll)
                .saturating_add(1)
                .min(max_scroll);
        }
        (KeyCode::PageUp, _) => {
            app.data.ui.changelog_scroll = app
                .data
                .ui
                .changelog_scroll
                .min(max_scroll)
                .saturating_sub(10);
        }
        (KeyCode::PageDown, _) => {
            app.data.ui.changelog_scroll = app
                .data
                .ui
                .changelog_scroll
                .min(max_scroll)
                .saturating_add(10)
                .min(max_scroll);
        }
        (KeyCode::Char('u'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            app.data.ui.changelog_scroll = app
                .data
                .ui
                .changelog_scroll
                .min(max_scroll)
                .saturating_sub(5);
        }
        (KeyCode::Char('d'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            app.data.ui.changelog_scroll = app
                .data
                .ui
                .changelog_scroll
                .min(max_scroll)
                .saturating_add(5)
                .min(max_scroll);
        }
        (KeyCode::Char('g') | KeyCode::Home, _) => {
            app.data.ui.changelog_scroll = 0;
        }
        (KeyCode::Char('G') | KeyCode::End, _) => {
            app.data.ui.changelog_scroll = max_scroll;
        }
        (KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'Q'), _) => {
            if let Some(version) = mark_seen_version
                && let Err(e) = app.data.settings.set_last_seen_version(&version)
            {
                warn!("Failed to save last_seen_version setting: {}", e);
            }
            app.apply_mode(AppMode::normal());
        }
        _ => {}
    }
    Ok(())
}

/// Dispatch a raw key event while in `ErrorModalMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if executing the dispatched action fails.
pub fn dispatch_error_modal_mode(app: &mut App, message: String) -> Result<()> {
    let next = DismissAction.execute(ErrorModalMode { message }, &mut app.data)?;
    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `SuccessModalMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if executing the dispatched action fails.
pub fn dispatch_success_modal_mode(app: &mut App, message: String) -> Result<()> {
    let next = DismissAction.execute(SuccessModalMode { message }, &mut app.data)?;
    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `PreviewFocusedMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_preview_focused_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) -> Result<()> {
    // Ctrl+C is a sharp edge: forwarding it to an agent will interrupt/terminate the agent
    // process and can cause the agent pane to disappear. Require confirmation for non-terminal
    // agents while attached.
    if matches!(code, KeyCode::Char('c' | 'C'))
        && modifiers.contains(KeyModifiers::CONTROL)
        && let Some(agent) = app.selected_agent()
        && !agent.is_terminal_agent()
    {
        app.apply_mode(
            ConfirmingMode {
                action: ConfirmAction::InterruptAgent,
            }
            .into(),
        );
        return Ok(());
    }

    // Ctrl+q exits preview focus mode (same key quits app when not focused).
    let next = if code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL) {
        UnfocusPreviewAction.execute(PreviewFocusedMode, &mut app.data)?
    } else {
        ForwardKeystrokeAction {
            code,
            modifiers,
            batched_keys,
        }
        .execute(PreviewFocusedMode, &mut app.data)?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `DiffFocusedMode`.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_diff_focused_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    // Ctrl+q exits diff focus mode (same key quits app when not focused).
    if matches!(code, KeyCode::Char('q' | 'Q')) && modifiers.contains(KeyModifiers::CONTROL) {
        let next = UnfocusDiffAction.execute(DiffFocusedMode, &mut app.data)?;
        app.apply_mode(next);
        return Ok(());
    }

    // Tab switching is bound to Normal mode only.
    if matches!(code, KeyCode::Tab | KeyCode::BackTab) {
        return Ok(());
    }

    // Esc should not exit diff focus mode (keep consistent with preview focus).
    if code == KeyCode::Esc {
        return Ok(());
    }

    // Diff navigation uses ↑/↓.
    if modifiers == KeyModifiers::NONE {
        match code {
            KeyCode::Up => {
                let next = DiffCursorUpAction.execute(DiffFocusedMode, &mut app.data)?;
                app.apply_mode(next);
                return Ok(());
            }
            KeyCode::Down => {
                let next = DiffCursorDownAction.execute(DiffFocusedMode, &mut app.data)?;
                app.apply_mode(next);
                return Ok(());
            }
            _ => {}
        }
    }

    let Some(action) = crate::config::get_action(code, modifiers) else {
        return Ok(());
    };

    let next = match action {
        KeyAction::DiffToggleVisual => {
            DiffToggleVisualAction.execute(DiffFocusedMode, &mut app.data)
        }
        KeyAction::DiffDeleteLine => DiffDeleteLineAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::DiffUndo => DiffUndoAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::DiffRedo => DiffRedoAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::ScrollUp => ScrollUpAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::ScrollDown => ScrollDownAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::ScrollTop => ScrollTopAction.execute(DiffFocusedMode, &mut app.data),
        KeyAction::ScrollBottom => ScrollBottomAction.execute(DiffFocusedMode, &mut app.data),
        other => {
            // For everything else, fall back to normal-mode dispatch (which exits diff focus).
            return dispatch_normal_mode(app, other);
        }
    }?;

    app.apply_mode(next);

    Ok(())
}

/// Dispatch a raw key event while in `CreatingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_creating_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    dispatch_text_input_mode(app, CreatingMode, code, modifiers)
}

/// Dispatch a raw key event while in `PromptingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_prompting_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, PromptingMode, code, modifiers)
}

/// Dispatch a raw key event while in `ChildPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_child_prompt_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, ChildPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `BroadcastingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_broadcasting_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, BroadcastingMode, code, modifiers)
}

/// Dispatch a raw key event while in `ReconnectPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_reconnect_prompt_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, ReconnectPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `TerminalPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_terminal_prompt_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, TerminalPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `CustomAgentCommandMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_custom_agent_command_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, CustomAgentCommandMode, code, modifiers)
}

/// Dispatch a raw key event while in `SynthesisPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_synthesis_prompt_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, SynthesisPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `ChildCountMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_child_count_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Enter => SelectAction.execute(ChildCountMode, app_data),
            KeyCode::Esc => CancelAction.execute(ChildCountMode, app_data),
            KeyCode::Up => IncrementAction.execute(ChildCountMode, app_data),
            KeyCode::Down => DecrementAction.execute(ChildCountMode, app_data),
            _ => Ok(ChildCountMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ReviewChildCountMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_review_child_count_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Enter => SelectAction.execute(ReviewChildCountMode, app_data),
            KeyCode::Esc => CancelAction.execute(ReviewChildCountMode, app_data),
            KeyCode::Up => IncrementAction.execute(ReviewChildCountMode, app_data),
            KeyCode::Down => DecrementAction.execute(ReviewChildCountMode, app_data),
            _ => Ok(ReviewChildCountMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ReviewInfoMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if executing the dispatched action fails.
pub fn dispatch_review_info_mode(app: &mut App) -> Result<()> {
    let next = DismissAction.execute(ReviewInfoMode, &mut app.data)?;
    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `BranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Enter => SelectAction.execute(BranchSelectorMode, app_data),
            KeyCode::Esc => CancelAction.execute(BranchSelectorMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(BranchSelectorMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(BranchSelectorMode, app_data),
            KeyCode::Char(c) => CharInputAction(c).execute(BranchSelectorMode, app_data),
            KeyCode::Backspace => BackspaceAction.execute(BranchSelectorMode, app_data),
            _ => Ok(BranchSelectorMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `RebaseBranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_rebase_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Enter => SelectAction.execute(RebaseBranchSelectorMode, app_data),
            KeyCode::Esc => CancelAction.execute(RebaseBranchSelectorMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(RebaseBranchSelectorMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(RebaseBranchSelectorMode, app_data),
            KeyCode::Char(c) => CharInputAction(c).execute(RebaseBranchSelectorMode, app_data),
            KeyCode::Backspace => BackspaceAction.execute(RebaseBranchSelectorMode, app_data),
            _ => Ok(RebaseBranchSelectorMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `MergeBranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_merge_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Enter => SelectAction.execute(MergeBranchSelectorMode, app_data),
            KeyCode::Esc => CancelAction.execute(MergeBranchSelectorMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(MergeBranchSelectorMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(MergeBranchSelectorMode, app_data),
            KeyCode::Char(c) => CharInputAction(c).execute(MergeBranchSelectorMode, app_data),
            KeyCode::Backspace => BackspaceAction.execute(MergeBranchSelectorMode, app_data),
            _ => Ok(MergeBranchSelectorMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `SwitchBranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_switch_branch_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Enter => SelectAction.execute(SwitchBranchSelectorMode, app_data),
            KeyCode::Esc => CancelAction.execute(SwitchBranchSelectorMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(SwitchBranchSelectorMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(SwitchBranchSelectorMode, app_data),
            KeyCode::Char(c) => CharInputAction(c).execute(SwitchBranchSelectorMode, app_data),
            KeyCode::Backspace => BackspaceAction.execute(SwitchBranchSelectorMode, app_data),
            _ => Ok(SwitchBranchSelectorMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ModelSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_model_selector_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Esc => CancelAction.execute(ModelSelectorMode, app_data),
            KeyCode::Enter => SelectAction.execute(ModelSelectorMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(ModelSelectorMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(ModelSelectorMode, app_data),
            KeyCode::Char(c) => CharInputAction(c).execute(ModelSelectorMode, app_data),
            KeyCode::Backspace => BackspaceAction.execute(ModelSelectorMode, app_data),
            _ => Ok(ModelSelectorMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `SettingsMenuMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_settings_menu_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Esc => CancelAction.execute(SettingsMenuMode, app_data),
            KeyCode::Enter => SelectAction.execute(SettingsMenuMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(SettingsMenuMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(SettingsMenuMode, app_data),
            _ => Ok(SettingsMenuMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `CommandPaletteMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_command_palette_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = {
        let app_data = &mut app.data;
        match code {
            KeyCode::Esc => CancelAction.execute(CommandPaletteMode, app_data),
            KeyCode::Enter => SelectAction.execute(CommandPaletteMode, app_data),
            KeyCode::Up => NavigateUpAction.execute(CommandPaletteMode, app_data),
            KeyCode::Down => NavigateDownAction.execute(CommandPaletteMode, app_data),
            KeyCode::Char(c) => CharInputAction(c).execute(CommandPaletteMode, app_data),
            KeyCode::Backspace => BackspaceAction.execute(CommandPaletteMode, app_data),
            KeyCode::Delete => DeleteAction.execute(CommandPaletteMode, app_data),
            KeyCode::Left => CursorLeftAction.execute(CommandPaletteMode, app_data),
            KeyCode::Right => CursorRightAction.execute(CommandPaletteMode, app_data),
            KeyCode::Home => CursorHomeAction.execute(CommandPaletteMode, app_data),
            KeyCode::End => CursorEndAction.execute(CommandPaletteMode, app_data),
            _ => Ok(CommandPaletteMode.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ConfirmPushMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_confirm_push_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = match code {
        KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(ConfirmPushMode, &mut app.data),
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(ConfirmPushMode, &mut app.data),
        KeyCode::Esc => CancelAction.execute(ConfirmPushMode, &mut app.data),
        _ => Ok(ConfirmPushMode.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ConfirmPushForPRMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_confirm_push_for_pr_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = match code {
        KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(ConfirmPushForPRMode, &mut app.data),
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(ConfirmPushForPRMode, &mut app.data),
        KeyCode::Esc => CancelAction.execute(ConfirmPushForPRMode, &mut app.data),
        _ => Ok(ConfirmPushForPRMode.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `RenameBranchMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_rename_branch_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = match code {
        KeyCode::Enter => SubmitAction.execute(RenameBranchMode, &mut app.data),
        KeyCode::Esc => CancelAction.execute(RenameBranchMode, &mut app.data),
        KeyCode::Char(c) => CharInputAction(c).execute(RenameBranchMode, &mut app.data),
        KeyCode::Backspace => BackspaceAction.execute(RenameBranchMode, &mut app.data),
        _ => Ok(RenameBranchMode.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `KeyboardRemapPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_keyboard_remap_prompt_mode(app: &mut App, code: KeyCode) -> Result<()> {
    let next = match code {
        KeyCode::Char('y' | 'Y') => {
            ConfirmYesAction.execute(KeyboardRemapPromptMode, &mut app.data)
        }
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(KeyboardRemapPromptMode, &mut app.data),
        KeyCode::Esc => CancelAction.execute(KeyboardRemapPromptMode, &mut app.data),
        _ => Ok(KeyboardRemapPromptMode.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `UpdatePromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_update_prompt_mode(app: &mut App, info: &UpdateInfo, code: KeyCode) -> Result<()> {
    let next = match code {
        KeyCode::Char('y' | 'Y') => {
            ConfirmYesAction.execute(UpdatePromptMode { info: info.clone() }, &mut app.data)
        }
        KeyCode::Char('n' | 'N') => {
            ConfirmNoAction.execute(UpdatePromptMode { info: info.clone() }, &mut app.data)
        }
        KeyCode::Esc => {
            CancelAction.execute(UpdatePromptMode { info: info.clone() }, &mut app.data)
        }
        _ => Ok(UpdatePromptMode { info: info.clone() }.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ConfirmingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_confirming_mode(app: &mut App, action: ConfirmAction, code: KeyCode) -> Result<()> {
    let state = ConfirmingMode { action };

    let next = if action == ConfirmAction::WorktreeConflict {
        match code {
            KeyCode::Char('r' | 'R') => WorktreeReconnectAction.execute(state, &mut app.data),
            KeyCode::Char('d' | 'D') => WorktreeRecreateAction.execute(state, &mut app.data),
            KeyCode::Esc => CancelAction.execute(state, &mut app.data),
            _ => Ok(state.into()),
        }?
    } else {
        match code {
            KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(state, &mut app.data),
            KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(state, &mut app.data),
            KeyCode::Esc => CancelAction.execute(state, &mut app.data),
            _ => Ok(state.into()),
        }?
    };

    app.apply_mode(next);
    Ok(())
}

#[cfg(test)]
mod tests {
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
            dispatch_diff_focused_mode(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL)
                .is_err()
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
            with_forced_picker_action_error_for_tests(|| dispatch_review_info_mode(&mut app))
                .is_err()
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
    fn test_dispatch_confirming_mode_propagates_execute_errors_for_kill_yes_when_storage_save_fails()
     {
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

        assert!(
            dispatch_confirming_mode(&mut app, ConfirmAction::Kill, KeyCode::Char('y')).is_err()
        );
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
        let backtab_result = dispatch_preview_focused_mode(
            &mut app,
            KeyCode::BackTab,
            KeyModifiers::NONE,
            &mut keys,
        );

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

        let err = dispatch_preview_focused_mode(
            &mut app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            &mut keys,
        )
        .expect_err("expected forced forward keystroke error");
        assert!(
            err.to_string()
                .contains("forced infallible action error for test")
        );
    }

    #[test]
    fn test_dispatch_confirming_mode_worktree_conflict_r_enters_reconnect_prompt_and_prefills_input()
     {
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
}
