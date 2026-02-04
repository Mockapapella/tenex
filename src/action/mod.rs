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
    SuccessModalMode, SynthesisPromptMode, TerminalPromptMode, UpdatePromptMode,
};
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use semver::Version;
use tracing::warn;

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
    let next = match action {
        KeyAction::NewAgent => NewAgentAction.execute(NormalMode, &mut app.data)?,
        KeyAction::NewAgentWithPrompt => {
            NewAgentWithPromptAction.execute(NormalMode, &mut app.data)?
        }
        KeyAction::Help => HelpAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Quit => QuitAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Kill => KillAction.execute(NormalMode, &mut app.data)?,
        KeyAction::SwitchTab => SwitchTabAction.execute(NormalMode, &mut app.data)?,
        KeyAction::NextAgent => NextAgentAction.execute(NormalMode, &mut app.data)?,
        KeyAction::PrevAgent => PrevAgentAction.execute(NormalMode, &mut app.data)?,
        KeyAction::SelectProjectHeader => {
            SelectProjectHeaderAction.execute(NormalMode, &mut app.data)?
        }
        KeyAction::SelectProjectFirstAgent => {
            SelectProjectFirstAgentAction.execute(NormalMode, &mut app.data)?
        }
        KeyAction::ScrollUp => ScrollUpAction.execute(NormalMode, &mut app.data)?,
        KeyAction::ScrollDown => ScrollDownAction.execute(NormalMode, &mut app.data)?,
        KeyAction::ScrollTop => ScrollTopAction.execute(NormalMode, &mut app.data)?,
        KeyAction::ScrollBottom => ScrollBottomAction.execute(NormalMode, &mut app.data)?,
        KeyAction::FocusPreview => FocusPreviewAction.execute(NormalMode, &mut app.data)?,
        KeyAction::SpawnChildren => SpawnChildrenAction.execute(NormalMode, &mut app.data)?,
        KeyAction::PlanSwarm => PlanSwarmAction.execute(NormalMode, &mut app.data)?,
        KeyAction::AddChildren => AddChildrenAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Synthesize => SynthesizeAction.execute(NormalMode, &mut app.data)?,
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Broadcast => BroadcastAction.execute(NormalMode, &mut app.data)?,
        KeyAction::ReviewSwarm => ReviewSwarmAction.execute(NormalMode, &mut app.data)?,
        KeyAction::SpawnTerminal => SpawnTerminalAction.execute(NormalMode, &mut app.data)?,
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction.execute(NormalMode, &mut app.data)?
        }
        KeyAction::Push => PushAction.execute(NormalMode, &mut app.data)?,
        KeyAction::RenameBranch => RenameBranchAction.execute(NormalMode, &mut app.data)?,
        KeyAction::OpenPR => OpenPRAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Rebase => RebaseAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Merge => MergeAction.execute(NormalMode, &mut app.data)?,
        KeyAction::CommandPalette => CommandPaletteAction.execute(NormalMode, &mut app.data)?,
        KeyAction::Cancel => CancelAction.execute(NormalMode, &mut app.data)?,

        // Not valid in Normal mode; treat as no-op.
        KeyAction::Confirm
        | KeyAction::UnfocusPreview
        | KeyAction::DiffCursorUp
        | KeyAction::DiffCursorDown
        | KeyAction::DiffToggleVisual
        | KeyAction::DiffDeleteLine
        | KeyAction::DiffUndo
        | KeyAction::DiffRedo => NormalMode.into(),
    };

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
    let next = match action {
        KeyAction::NewAgent => NewAgentAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::NewAgentWithPrompt => {
            NewAgentWithPromptAction.execute(ScrollingMode, &mut app.data)?
        }
        KeyAction::Help => HelpAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Quit => QuitAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Kill => KillAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::SwitchTab => SwitchTabAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::NextAgent => NextAgentAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::PrevAgent => PrevAgentAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::SelectProjectHeader => {
            SelectProjectHeaderAction.execute(ScrollingMode, &mut app.data)?
        }
        KeyAction::SelectProjectFirstAgent => {
            SelectProjectFirstAgentAction.execute(ScrollingMode, &mut app.data)?
        }
        KeyAction::ScrollUp => ScrollUpAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::ScrollDown => ScrollDownAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::ScrollTop => ScrollTopAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::ScrollBottom => ScrollBottomAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::FocusPreview => FocusPreviewAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::SpawnChildren => SpawnChildrenAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::PlanSwarm => PlanSwarmAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::AddChildren => AddChildrenAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Synthesize => SynthesizeAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Broadcast => BroadcastAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::ReviewSwarm => ReviewSwarmAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::SpawnTerminal => SpawnTerminalAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction.execute(ScrollingMode, &mut app.data)?
        }
        KeyAction::Push => PushAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::RenameBranch => RenameBranchAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::OpenPR => OpenPRAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Rebase => RebaseAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Merge => MergeAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::CommandPalette => CommandPaletteAction.execute(ScrollingMode, &mut app.data)?,
        KeyAction::Cancel => CancelAction.execute(ScrollingMode, &mut app.data)?,

        // Not valid in Scrolling mode; treat as no-op.
        KeyAction::Confirm
        | KeyAction::UnfocusPreview
        | KeyAction::DiffCursorUp
        | KeyAction::DiffCursorDown
        | KeyAction::DiffToggleVisual
        | KeyAction::DiffDeleteLine
        | KeyAction::DiffUndo
        | KeyAction::DiffRedo => ScrollingMode.into(),
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `HelpMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_help_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    let next = {
        let app_data = &mut app.data;

        match (code, modifiers) {
            (KeyCode::Up, _) => ScrollUpAction.execute(HelpMode, app_data)?,
            (KeyCode::Down, _) => ScrollDownAction.execute(HelpMode, app_data)?,
            (KeyCode::PageUp, _) => PageUpAction.execute(HelpMode, app_data)?,
            (KeyCode::PageDown, _) => PageDownAction.execute(HelpMode, app_data)?,
            (KeyCode::Char('u'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                HalfPageUpAction.execute(HelpMode, app_data)?
            }
            (KeyCode::Char('d'), mods) if mods.contains(KeyModifiers::CONTROL) => {
                HalfPageDownAction.execute(HelpMode, app_data)?
            }
            (KeyCode::Char('g') | KeyCode::Home, _) => {
                ScrollTopAction.execute(HelpMode, app_data)?
            }
            (KeyCode::Char('G') | KeyCode::End, _) => {
                ScrollBottomAction.execute(HelpMode, app_data)?
            }
            _ => DismissAction.execute(HelpMode, app_data)?,
        }
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
/// Returns an error if the dispatched action fails.
pub fn dispatch_error_modal_mode(app: &mut App, message: String) -> Result<()> {
    let next = DismissAction.execute(ErrorModalMode { message }, &mut app.data)?;
    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `SuccessModalMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
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
    // Tab switching is bound to Normal mode only.
    if matches!(code, KeyCode::Tab | KeyCode::BackTab) {
        return Ok(());
    }

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

    if let Some(action) = crate::config::get_action(code, modifiers) {
        let next = match action {
            KeyAction::DiffToggleVisual => {
                DiffToggleVisualAction.execute(DiffFocusedMode, &mut app.data)?
            }
            KeyAction::DiffDeleteLine => {
                DiffDeleteLineAction.execute(DiffFocusedMode, &mut app.data)?
            }
            KeyAction::DiffUndo => DiffUndoAction.execute(DiffFocusedMode, &mut app.data)?,
            KeyAction::DiffRedo => DiffRedoAction.execute(DiffFocusedMode, &mut app.data)?,
            KeyAction::ToggleCollapse => {
                ToggleCollapseAction.execute(DiffFocusedMode, &mut app.data)?
            }
            KeyAction::ScrollUp => ScrollUpAction.execute(DiffFocusedMode, &mut app.data)?,
            KeyAction::ScrollDown => ScrollDownAction.execute(DiffFocusedMode, &mut app.data)?,
            KeyAction::ScrollTop => ScrollTopAction.execute(DiffFocusedMode, &mut app.data)?,
            KeyAction::ScrollBottom => {
                ScrollBottomAction.execute(DiffFocusedMode, &mut app.data)?
            }
            KeyAction::SwitchTab => SwitchTabAction.execute(DiffFocusedMode, &mut app.data)?,
            other => {
                // For everything else, fall back to normal-mode dispatch (which exits diff focus).
                return dispatch_normal_mode(app, other);
            }
        };

        app.apply_mode(next);
    }

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
            KeyCode::Enter => SelectAction.execute(ChildCountMode, app_data)?,
            KeyCode::Esc => CancelAction.execute(ChildCountMode, app_data)?,
            KeyCode::Up => IncrementAction.execute(ChildCountMode, app_data)?,
            KeyCode::Down => DecrementAction.execute(ChildCountMode, app_data)?,
            _ => ChildCountMode.into(),
        }
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
            KeyCode::Enter => SelectAction.execute(ReviewChildCountMode, app_data)?,
            KeyCode::Esc => CancelAction.execute(ReviewChildCountMode, app_data)?,
            KeyCode::Up => IncrementAction.execute(ReviewChildCountMode, app_data)?,
            KeyCode::Down => DecrementAction.execute(ReviewChildCountMode, app_data)?,
            _ => ReviewChildCountMode.into(),
        }
    };

    app.apply_mode(next);
    Ok(())
}

/// Dispatch a raw key event while in `ReviewInfoMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
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
            KeyCode::Enter => SelectAction.execute(BranchSelectorMode, app_data)?,
            KeyCode::Esc => CancelAction.execute(BranchSelectorMode, app_data)?,
            KeyCode::Up => NavigateUpAction.execute(BranchSelectorMode, app_data)?,
            KeyCode::Down => NavigateDownAction.execute(BranchSelectorMode, app_data)?,
            KeyCode::Char(c) => CharInputAction(c).execute(BranchSelectorMode, app_data)?,
            KeyCode::Backspace => BackspaceAction.execute(BranchSelectorMode, app_data)?,
            _ => BranchSelectorMode.into(),
        }
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
            KeyCode::Enter => SelectAction.execute(RebaseBranchSelectorMode, app_data)?,
            KeyCode::Esc => CancelAction.execute(RebaseBranchSelectorMode, app_data)?,
            KeyCode::Up => NavigateUpAction.execute(RebaseBranchSelectorMode, app_data)?,
            KeyCode::Down => NavigateDownAction.execute(RebaseBranchSelectorMode, app_data)?,
            KeyCode::Char(c) => CharInputAction(c).execute(RebaseBranchSelectorMode, app_data)?,
            KeyCode::Backspace => BackspaceAction.execute(RebaseBranchSelectorMode, app_data)?,
            _ => RebaseBranchSelectorMode.into(),
        }
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
            KeyCode::Enter => SelectAction.execute(MergeBranchSelectorMode, app_data)?,
            KeyCode::Esc => CancelAction.execute(MergeBranchSelectorMode, app_data)?,
            KeyCode::Up => NavigateUpAction.execute(MergeBranchSelectorMode, app_data)?,
            KeyCode::Down => NavigateDownAction.execute(MergeBranchSelectorMode, app_data)?,
            KeyCode::Char(c) => CharInputAction(c).execute(MergeBranchSelectorMode, app_data)?,
            KeyCode::Backspace => BackspaceAction.execute(MergeBranchSelectorMode, app_data)?,
            _ => MergeBranchSelectorMode.into(),
        }
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
            KeyCode::Esc => CancelAction.execute(ModelSelectorMode, app_data)?,
            KeyCode::Enter => SelectAction.execute(ModelSelectorMode, app_data)?,
            KeyCode::Up => NavigateUpAction.execute(ModelSelectorMode, app_data)?,
            KeyCode::Down => NavigateDownAction.execute(ModelSelectorMode, app_data)?,
            KeyCode::Char(c) => CharInputAction(c).execute(ModelSelectorMode, app_data)?,
            KeyCode::Backspace => BackspaceAction.execute(ModelSelectorMode, app_data)?,
            _ => ModelSelectorMode.into(),
        }
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
            KeyCode::Esc => CancelAction.execute(SettingsMenuMode, app_data)?,
            KeyCode::Enter => SelectAction.execute(SettingsMenuMode, app_data)?,
            KeyCode::Up => NavigateUpAction.execute(SettingsMenuMode, app_data)?,
            KeyCode::Down => NavigateDownAction.execute(SettingsMenuMode, app_data)?,
            _ => SettingsMenuMode.into(),
        }
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
            KeyCode::Esc => CancelAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Enter => SelectAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Up => NavigateUpAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Down => NavigateDownAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Char(c) => CharInputAction(c).execute(CommandPaletteMode, app_data)?,
            KeyCode::Backspace => BackspaceAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Delete => DeleteAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Left => CursorLeftAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Right => CursorRightAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::Home => CursorHomeAction.execute(CommandPaletteMode, app_data)?,
            KeyCode::End => CursorEndAction.execute(CommandPaletteMode, app_data)?,
            _ => CommandPaletteMode.into(),
        }
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
        KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(ConfirmPushMode, &mut app.data)?,
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(ConfirmPushMode, &mut app.data)?,
        KeyCode::Esc => CancelAction.execute(ConfirmPushMode, &mut app.data)?,
        _ => ConfirmPushMode.into(),
    };

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
        KeyCode::Char('y' | 'Y') => {
            ConfirmYesAction.execute(ConfirmPushForPRMode, &mut app.data)?
        }
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(ConfirmPushForPRMode, &mut app.data)?,
        KeyCode::Esc => CancelAction.execute(ConfirmPushForPRMode, &mut app.data)?,
        _ => ConfirmPushForPRMode.into(),
    };

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
        KeyCode::Enter => SubmitAction.execute(RenameBranchMode, &mut app.data)?,
        KeyCode::Esc => CancelAction.execute(RenameBranchMode, &mut app.data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(RenameBranchMode, &mut app.data)?,
        KeyCode::Backspace => BackspaceAction.execute(RenameBranchMode, &mut app.data)?,
        _ => RenameBranchMode.into(),
    };

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
            ConfirmYesAction.execute(KeyboardRemapPromptMode, &mut app.data)?
        }
        KeyCode::Char('n' | 'N') => {
            ConfirmNoAction.execute(KeyboardRemapPromptMode, &mut app.data)?
        }
        KeyCode::Esc => CancelAction.execute(KeyboardRemapPromptMode, &mut app.data)?,
        _ => KeyboardRemapPromptMode.into(),
    };

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
            ConfirmYesAction.execute(UpdatePromptMode { info: info.clone() }, &mut app.data)?
        }
        KeyCode::Char('n' | 'N') => {
            ConfirmNoAction.execute(UpdatePromptMode { info: info.clone() }, &mut app.data)?
        }
        KeyCode::Esc => {
            CancelAction.execute(UpdatePromptMode { info: info.clone() }, &mut app.data)?
        }
        _ => UpdatePromptMode { info: info.clone() }.into(),
    };

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
            KeyCode::Char('r' | 'R') => WorktreeReconnectAction.execute(state, &mut app.data)?,
            KeyCode::Char('d' | 'D') => WorktreeRecreateAction.execute(state, &mut app.data)?,
            KeyCode::Esc => CancelAction.execute(state, &mut app.data)?,
            _ => state.into(),
        }
    } else {
        match code {
            KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(state, &mut app.data)?,
            KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(state, &mut app.data)?,
            KeyCode::Esc => CancelAction.execute(state, &mut app.data)?,
            _ => state.into(),
        }
    };

    app.apply_mode(next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::{
        AppMode, BroadcastingMode, ChildCountMode, CommandPaletteMode, ConfirmPushMode,
        CreatingMode, HelpMode, MergeBranchSelectorMode, PromptingMode, RebaseBranchSelectorMode,
        RenameBranchMode, ReviewChildCountMode, ScrollingMode, TerminalPromptMode,
    };
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
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

    #[test]
    fn test_scrolling_mode_typed_dispatch_covers_actions() -> anyhow::Result<()> {
        let original_dir = std::env::current_dir()?;
        std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"))?;

        let (mut app, _temp) = create_test_app()?;
        add_agent_with_child(&mut app);

        dispatch_normal_mode(&mut app, KeyAction::ScrollUp)?;
        assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

        dispatch_scrolling_mode(&mut app, KeyAction::SwitchTab)?;
        dispatch_scrolling_mode(&mut app, KeyAction::NextAgent)?;
        dispatch_scrolling_mode(&mut app, KeyAction::PrevAgent)?;
        dispatch_scrolling_mode(&mut app, KeyAction::ScrollDown)?;
        dispatch_scrolling_mode(&mut app, KeyAction::ScrollTop)?;
        dispatch_scrolling_mode(&mut app, KeyAction::ScrollBottom)?;

        dispatch_scrolling_mode(&mut app, KeyAction::Kill)?;
        assert!(matches!(&app.mode, AppMode::Confirming(_)));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Synthesize)?;
        assert!(matches!(&app.mode, AppMode::Confirming(_)));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::ToggleCollapse)?;
        assert_eq!(app.mode, AppMode::Scrolling(ScrollingMode));

        dispatch_scrolling_mode(&mut app, KeyAction::SpawnChildren)?;
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::PlanSwarm)?;
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::AddChildren)?;
        assert_eq!(app.mode, AppMode::ChildCount(ChildCountMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::ReviewSwarm)?;
        assert_eq!(app.mode, AppMode::ReviewChildCount(ReviewChildCountMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Broadcast)?;
        assert_eq!(app.mode, AppMode::Broadcasting(BroadcastingMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, AppMode::TerminalPrompt(TerminalPromptMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Push)?;
        assert_eq!(app.mode, AppMode::ConfirmPush(ConfirmPushMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::RenameBranch)?;
        assert_eq!(app.mode, AppMode::RenameBranch(RenameBranchMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Rebase)?;
        assert_eq!(
            app.mode,
            AppMode::RebaseBranchSelector(RebaseBranchSelectorMode)
        );
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Merge)?;
        assert_eq!(
            app.mode,
            AppMode::MergeBranchSelector(MergeBranchSelectorMode)
        );
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Help)?;
        assert_eq!(app.mode, AppMode::Help(HelpMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::CommandPalette)?;
        assert_eq!(app.mode, AppMode::CommandPalette(CommandPaletteMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::NewAgent)?;
        assert_eq!(app.mode, AppMode::Creating(CreatingMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::NewAgentWithPrompt)?;
        assert_eq!(app.mode, AppMode::Prompting(PromptingMode));
        app.exit_mode();
        app.enter_mode(ScrollingMode.into());

        dispatch_scrolling_mode(&mut app, KeyAction::Cancel)?;
        assert_eq!(app.mode, AppMode::normal());

        std::env::set_current_dir(original_dir)?;
        Ok(())
    }

    #[test]
    fn test_diff_focused_mode_raw_dispatch_routes_keys() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        add_agent_with_child(&mut app);

        app.data.active_tab = crate::app::Tab::Diff;
        app.data.ui.set_diff_content("a\nb\nc\n");
        app.enter_mode(DiffFocusedMode.into());

        dispatch_diff_focused_mode(&mut app, KeyCode::Down, KeyModifiers::NONE)?;
        assert_eq!(app.data.ui.diff_cursor, 1);
        assert!(matches!(app.mode, AppMode::DiffFocused(_)));

        dispatch_diff_focused_mode(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL)?;
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Nothing to undo")
        );
        assert!(matches!(app.mode, AppMode::DiffFocused(_)));

        dispatch_diff_focused_mode(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL)?;
        assert_eq!(
            app.data.ui.status_message.as_deref(),
            Some("Nothing to redo")
        );
        assert!(matches!(app.mode, AppMode::DiffFocused(_)));

        dispatch_diff_focused_mode(&mut app, KeyCode::Char(' '), KeyModifiers::NONE)?;
        assert!(matches!(app.mode, AppMode::DiffFocused(_)));

        // Unhandled actions should fall back to normal-mode dispatch.
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE)?;
        assert!(matches!(app.mode, AppMode::Creating(_)));

        app.enter_mode(DiffFocusedMode.into());
        dispatch_diff_focused_mode(&mut app, KeyCode::Esc, KeyModifiers::NONE)?;
        assert!(matches!(app.mode, AppMode::DiffFocused(_)));

        app.enter_mode(DiffFocusedMode.into());
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL)?;
        assert_eq!(app.mode, AppMode::normal());

        Ok(())
    }

    #[test]
    fn test_dispatch_changelog_mode_scroll_and_dismiss() -> anyhow::Result<()> {
        let (mut app, _temp) = create_test_app()?;
        app.mode = AppMode::Changelog(crate::state::ChangelogMode {
            title: "Changelog".to_string(),
            lines: vec!["Line".to_string()],
            mark_seen_version: None,
        });

        let max_scroll = 5usize;
        app.data.ui.changelog_scroll = 100;

        dispatch_changelog_mode(&mut app, None, max_scroll, KeyCode::Up, KeyModifiers::NONE)?;
        assert_eq!(app.data.ui.changelog_scroll, 4);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Down,
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 5);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Down,
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 5);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::PageUp,
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 0);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::PageDown,
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 5);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 0);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 5);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Char('g'),
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 0);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Char('G'),
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 5);

        dispatch_changelog_mode(
            &mut app,
            None,
            max_scroll,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )?;
        assert_eq!(app.data.ui.changelog_scroll, 5);

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
        )?;
        assert_eq!(app.mode, AppMode::normal());

        Ok(())
    }
}
