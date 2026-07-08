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
        KeyAction::ToggleSynthesisMark => ToggleSynthesisMarkAction.execute(NormalMode, app_data),
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
        KeyAction::ToggleSynthesisMark => {
            ToggleSynthesisMarkAction.execute(ScrollingMode, app_data)
        }
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
mod tests;
