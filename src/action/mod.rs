//! Compile-time action types (new architecture).

mod agent;
mod confirm;
mod git;
mod misc;
mod modal;
mod navigation;
mod picker;
mod preview;
mod text_input;

pub use agent::*;
pub use confirm::*;
pub use git::*;
pub use misc::*;
#[cfg(test)]
pub(crate) use modal::help_max_scroll;
pub use modal::{HalfPageDownAction, HalfPageUpAction, PageDownAction, PageUpAction};
pub use navigation::*;
pub use picker::*;
#[cfg(test)]
pub(crate) use preview::keycode_to_input_sequence;
pub use preview::{ForwardKeystrokeAction, UnfocusPreviewAction};
pub use text_input::*;

use crate::app::{Actions, App, AppData, ConfirmAction};
use crate::config::Action as KeyAction;
use crate::state::{
    BranchSelectorMode, BroadcastingMode, ChildCountMode, ChildPromptMode, CommandPaletteMode,
    ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode, CreatingMode, CustomAgentCommandMode,
    ErrorModalMode, HelpMode, KeyboardRemapPromptMode, MergeBranchSelectorMode, ModeUnion,
    ModelSelectorMode, NormalMode, PreviewFocusedMode, PromptingMode, RebaseBranchSelectorMode,
    ReconnectPromptMode, RenameBranchMode, ReviewChildCountMode, ReviewInfoMode, ScrollingMode,
    SuccessModalMode, TerminalPromptMode, UpdatePromptMode,
};
use crate::update::UpdateInfo;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

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
    fn execute(self, state: State, app_data: &mut AppData<'_>) -> Result<Self::NextState>;
}

/// Dispatch a legacy keybinding `Action` while in `NormalMode`, using the new
/// compile-time (State, Action) registry.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_normal_mode(app: &mut App, actions: Actions, action: KeyAction) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match action {
        KeyAction::NewAgent => NewAgentAction.execute(NormalMode, &mut app_data)?,
        KeyAction::NewAgentWithPrompt => {
            NewAgentWithPromptAction.execute(NormalMode, &mut app_data)?
        }
        KeyAction::Help => HelpAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Quit => QuitAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Kill => KillAction.execute(NormalMode, &mut app_data)?,
        KeyAction::SwitchTab => SwitchTabAction.execute(NormalMode, &mut app_data)?,
        KeyAction::NextAgent => NextAgentAction.execute(NormalMode, &mut app_data)?,
        KeyAction::PrevAgent => PrevAgentAction.execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollUp => ScrollUpAction.execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollDown => ScrollDownAction.execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollTop => ScrollTopAction.execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollBottom => ScrollBottomAction.execute(NormalMode, &mut app_data)?,
        KeyAction::FocusPreview => FocusPreviewAction.execute(NormalMode, &mut app_data)?,
        KeyAction::SpawnChildren => SpawnChildrenAction.execute(NormalMode, &mut app_data)?,
        KeyAction::PlanSwarm => PlanSwarmAction.execute(NormalMode, &mut app_data)?,
        KeyAction::AddChildren => AddChildrenAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Synthesize => SynthesizeAction.execute(NormalMode, &mut app_data)?,
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Broadcast => BroadcastAction.execute(NormalMode, &mut app_data)?,
        KeyAction::ReviewSwarm => ReviewSwarmAction.execute(NormalMode, &mut app_data)?,
        KeyAction::SpawnTerminal => SpawnTerminalAction.execute(NormalMode, &mut app_data)?,
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction.execute(NormalMode, &mut app_data)?
        }
        KeyAction::Push => PushAction.execute(NormalMode, &mut app_data)?,
        KeyAction::RenameBranch => RenameBranchAction.execute(NormalMode, &mut app_data)?,
        KeyAction::OpenPR => OpenPRAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Rebase => RebaseAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Merge => MergeAction.execute(NormalMode, &mut app_data)?,
        KeyAction::CommandPalette => CommandPaletteAction.execute(NormalMode, &mut app_data)?,
        KeyAction::Cancel => CancelAction.execute(NormalMode, &mut app_data)?,

        // Not valid in Normal mode; keep legacy behavior (no-op) for now.
        KeyAction::Confirm | KeyAction::UnfocusPreview => ModeUnion::normal(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a legacy keybinding `Action` while in `ScrollingMode`, using the new
/// compile-time (State, Action) registry.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_scrolling_mode(app: &mut App, actions: Actions, action: KeyAction) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match action {
        KeyAction::NewAgent => NewAgentAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::NewAgentWithPrompt => {
            NewAgentWithPromptAction.execute(ScrollingMode, &mut app_data)?
        }
        KeyAction::Help => HelpAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Quit => QuitAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Kill => KillAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::SwitchTab => SwitchTabAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::NextAgent => NextAgentAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::PrevAgent => PrevAgentAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::ScrollUp => ScrollUpAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::ScrollDown => ScrollDownAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::ScrollTop => ScrollTopAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::ScrollBottom => ScrollBottomAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::FocusPreview => FocusPreviewAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::SpawnChildren => SpawnChildrenAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::PlanSwarm => PlanSwarmAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::AddChildren => AddChildrenAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Synthesize => SynthesizeAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::ToggleCollapse => ToggleCollapseAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Broadcast => BroadcastAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::ReviewSwarm => ReviewSwarmAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::SpawnTerminal => SpawnTerminalAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction.execute(ScrollingMode, &mut app_data)?
        }
        KeyAction::Push => PushAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::RenameBranch => RenameBranchAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::OpenPR => OpenPRAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Rebase => RebaseAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Merge => MergeAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::CommandPalette => CommandPaletteAction.execute(ScrollingMode, &mut app_data)?,
        KeyAction::Cancel => CancelAction.execute(ScrollingMode, &mut app_data)?,

        // Not valid in Scrolling mode; keep legacy behavior (no-op) for now.
        KeyAction::Confirm | KeyAction::UnfocusPreview => ScrollingMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `HelpMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_help_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match (code, modifiers) {
        (KeyCode::Up, _) => ScrollUpAction.execute(HelpMode, &mut app_data)?,
        (KeyCode::Down, _) => ScrollDownAction.execute(HelpMode, &mut app_data)?,
        (KeyCode::PageUp, _) => PageUpAction.execute(HelpMode, &mut app_data)?,
        (KeyCode::PageDown, _) => PageDownAction.execute(HelpMode, &mut app_data)?,
        (KeyCode::Char('u'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            HalfPageUpAction.execute(HelpMode, &mut app_data)?
        }
        (KeyCode::Char('d'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            HalfPageDownAction.execute(HelpMode, &mut app_data)?
        }
        (KeyCode::Char('g') | KeyCode::Home, _) => {
            ScrollTopAction.execute(HelpMode, &mut app_data)?
        }
        (KeyCode::Char('G') | KeyCode::End, _) => {
            ScrollBottomAction.execute(HelpMode, &mut app_data)?
        }
        _ => DismissAction.execute(HelpMode, &mut app_data)?,
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ErrorModalMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_error_modal_mode(app: &mut App, actions: Actions, message: String) -> Result<()> {
    let mut app_data = AppData::new(app, actions);
    let next = DismissAction.execute(ErrorModalMode { message }, &mut app_data)?;
    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `SuccessModalMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_success_modal_mode(app: &mut App, actions: Actions, message: String) -> Result<()> {
    let mut app_data = AppData::new(app, actions);
    let next = DismissAction.execute(SuccessModalMode { message }, &mut app_data)?;
    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `PreviewFocusedMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_preview_focused_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    // Ctrl+q exits preview focus mode (same key quits app when not focused).
    let next = if code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL) {
        UnfocusPreviewAction.execute(PreviewFocusedMode, &mut app_data)?
    } else {
        ForwardKeystrokeAction {
            code,
            modifiers,
            batched_keys,
        }
        .execute(PreviewFocusedMode, &mut app_data)?
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `CreatingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_creating_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, CreatingMode, code, modifiers)
}

/// Dispatch a raw key event while in `PromptingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_prompting_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, PromptingMode, code, modifiers)
}

/// Dispatch a raw key event while in `ChildPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_child_prompt_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, ChildPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `BroadcastingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_broadcasting_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, BroadcastingMode, code, modifiers)
}

/// Dispatch a raw key event while in `ReconnectPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_reconnect_prompt_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, ReconnectPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `TerminalPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_terminal_prompt_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, TerminalPromptMode, code, modifiers)
}

/// Dispatch a raw key event while in `CustomAgentCommandMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_custom_agent_command_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    dispatch_text_input_mode(app, actions, CustomAgentCommandMode, code, modifiers)
}

/// Dispatch a raw key event while in `ChildCountMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_child_count_mode(app: &mut App, actions: Actions, code: KeyCode) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Enter => SelectAction.execute(ChildCountMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(ChildCountMode, &mut app_data)?,
        KeyCode::Up => IncrementAction.execute(ChildCountMode, &mut app_data)?,
        KeyCode::Down => DecrementAction.execute(ChildCountMode, &mut app_data)?,
        _ => ChildCountMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ReviewChildCountMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_review_child_count_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Enter => SelectAction.execute(ReviewChildCountMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(ReviewChildCountMode, &mut app_data)?,
        KeyCode::Up => IncrementAction.execute(ReviewChildCountMode, &mut app_data)?,
        KeyCode::Down => DecrementAction.execute(ReviewChildCountMode, &mut app_data)?,
        _ => ReviewChildCountMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ReviewInfoMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_review_info_mode(app: &mut App, actions: Actions) -> Result<()> {
    let mut app_data = AppData::new(app, actions);
    let next = DismissAction.execute(ReviewInfoMode, &mut app_data)?;
    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `BranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_branch_selector_mode(app: &mut App, actions: Actions, code: KeyCode) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Enter => SelectAction.execute(BranchSelectorMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(BranchSelectorMode, &mut app_data)?,
        KeyCode::Up => NavigateUpAction.execute(BranchSelectorMode, &mut app_data)?,
        KeyCode::Down => NavigateDownAction.execute(BranchSelectorMode, &mut app_data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(BranchSelectorMode, &mut app_data)?,
        KeyCode::Backspace => BackspaceAction.execute(BranchSelectorMode, &mut app_data)?,
        _ => BranchSelectorMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `RebaseBranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_rebase_branch_selector_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Enter => SelectAction.execute(RebaseBranchSelectorMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(RebaseBranchSelectorMode, &mut app_data)?,
        KeyCode::Up => NavigateUpAction.execute(RebaseBranchSelectorMode, &mut app_data)?,
        KeyCode::Down => NavigateDownAction.execute(RebaseBranchSelectorMode, &mut app_data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(RebaseBranchSelectorMode, &mut app_data)?,
        KeyCode::Backspace => BackspaceAction.execute(RebaseBranchSelectorMode, &mut app_data)?,
        _ => RebaseBranchSelectorMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `MergeBranchSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_merge_branch_selector_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Enter => SelectAction.execute(MergeBranchSelectorMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(MergeBranchSelectorMode, &mut app_data)?,
        KeyCode::Up => NavigateUpAction.execute(MergeBranchSelectorMode, &mut app_data)?,
        KeyCode::Down => NavigateDownAction.execute(MergeBranchSelectorMode, &mut app_data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(MergeBranchSelectorMode, &mut app_data)?,
        KeyCode::Backspace => BackspaceAction.execute(MergeBranchSelectorMode, &mut app_data)?,
        _ => MergeBranchSelectorMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ModelSelectorMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_model_selector_mode(app: &mut App, actions: Actions, code: KeyCode) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Esc => CancelAction.execute(ModelSelectorMode, &mut app_data)?,
        KeyCode::Enter => SelectAction.execute(ModelSelectorMode, &mut app_data)?,
        KeyCode::Up => NavigateUpAction.execute(ModelSelectorMode, &mut app_data)?,
        KeyCode::Down => NavigateDownAction.execute(ModelSelectorMode, &mut app_data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(ModelSelectorMode, &mut app_data)?,
        KeyCode::Backspace => BackspaceAction.execute(ModelSelectorMode, &mut app_data)?,
        _ => ModelSelectorMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `CommandPaletteMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_command_palette_mode(app: &mut App, actions: Actions, code: KeyCode) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Esc => CancelAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Enter => SelectAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Up => NavigateUpAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Down => NavigateDownAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Backspace => BackspaceAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Delete => DeleteAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Left => CursorLeftAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Right => CursorRightAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::Home => CursorHomeAction.execute(CommandPaletteMode, &mut app_data)?,
        KeyCode::End => CursorEndAction.execute(CommandPaletteMode, &mut app_data)?,
        _ => CommandPaletteMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ConfirmPushMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_confirm_push_mode(app: &mut App, actions: Actions, code: KeyCode) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(ConfirmPushMode, &mut app_data)?,
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(ConfirmPushMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(ConfirmPushMode, &mut app_data)?,
        _ => ConfirmPushMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ConfirmPushForPRMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_confirm_push_for_pr_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Char('y' | 'Y') => {
            ConfirmYesAction.execute(ConfirmPushForPRMode, &mut app_data)?
        }
        KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(ConfirmPushForPRMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(ConfirmPushForPRMode, &mut app_data)?,
        _ => ConfirmPushForPRMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `RenameBranchMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_rename_branch_mode(app: &mut App, actions: Actions, code: KeyCode) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Enter => SubmitAction.execute(RenameBranchMode, &mut app_data)?,
        KeyCode::Esc => CancelAction.execute(RenameBranchMode, &mut app_data)?,
        KeyCode::Char(c) => CharInputAction(c).execute(RenameBranchMode, &mut app_data)?,
        KeyCode::Backspace => BackspaceAction.execute(RenameBranchMode, &mut app_data)?,
        _ => RenameBranchMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `KeyboardRemapPromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_keyboard_remap_prompt_mode(
    app: &mut App,
    actions: Actions,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Char('y' | 'Y') => {
            ConfirmYesAction.execute(KeyboardRemapPromptMode, &mut app_data)?
        }
        KeyCode::Char('n' | 'N') => {
            ConfirmNoAction.execute(KeyboardRemapPromptMode, &mut app_data)?
        }
        KeyCode::Esc => CancelAction.execute(KeyboardRemapPromptMode, &mut app_data)?,
        _ => KeyboardRemapPromptMode.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `UpdatePromptMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_update_prompt_mode(
    app: &mut App,
    actions: Actions,
    info: UpdateInfo,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);

    let next = match code {
        KeyCode::Char('y' | 'Y') => {
            ConfirmYesAction.execute(UpdatePromptMode { info }, &mut app_data)?
        }
        KeyCode::Char('n' | 'N') => {
            ConfirmNoAction.execute(UpdatePromptMode { info }, &mut app_data)?
        }
        KeyCode::Esc => CancelAction.execute(UpdatePromptMode { info }, &mut app_data)?,
        _ => UpdatePromptMode { info }.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

/// Dispatch a raw key event while in `ConfirmingMode`, using typed actions.
///
/// # Errors
///
/// Returns an error if the dispatched action fails.
pub fn dispatch_confirming_mode(
    app: &mut App,
    actions: Actions,
    action: ConfirmAction,
    code: KeyCode,
) -> Result<()> {
    let mut app_data = AppData::new(app, actions);
    let state = ConfirmingMode { action };

    let next = if action == ConfirmAction::WorktreeConflict {
        match code {
            KeyCode::Char('r' | 'R') => WorktreeReconnectAction.execute(state, &mut app_data)?,
            KeyCode::Char('d' | 'D') => WorktreeRecreateAction.execute(state, &mut app_data)?,
            KeyCode::Esc => CancelAction.execute(state, &mut app_data)?,
            _ => state.into(),
        }
    } else {
        match code {
            KeyCode::Char('y' | 'Y') => ConfirmYesAction.execute(state, &mut app_data)?,
            KeyCode::Char('n' | 'N') => ConfirmNoAction.execute(state, &mut app_data)?,
            KeyCode::Esc => CancelAction.execute(state, &mut app_data)?,
            _ => state.into(),
        }
    };

    next.apply(app_data.app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::app::{Mode, Settings};
    use crate::config::Config;
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
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        let root_branch = root.branch.clone();
        let root_worktree = root.worktree_path.clone();
        app.storage.add(root);
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            root_branch,
            root_worktree,
            None,
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 1,
            },
        );
        app.storage.add(child);
    }

    #[test]
    fn test_scrolling_mode_typed_dispatch_covers_actions() -> anyhow::Result<()> {
        let original_dir = std::env::current_dir()?;
        std::env::set_current_dir(env!("CARGO_MANIFEST_DIR"))?;

        let (mut app, _temp) = create_test_app()?;
        add_agent_with_child(&mut app);

        dispatch_normal_mode(&mut app, Actions::new(), KeyAction::ScrollUp)?;
        assert_eq!(app.mode, Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::SwitchTab)?;
        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::NextAgent)?;
        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::PrevAgent)?;
        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::ScrollDown)?;
        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::ScrollTop)?;
        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::ScrollBottom)?;

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Kill)?;
        assert!(matches!(app.mode, Mode::Confirming(_)));
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Synthesize)?;
        assert!(matches!(app.mode, Mode::Confirming(_)));
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::ToggleCollapse)?;
        assert_eq!(app.mode, Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::SpawnChildren)?;
        assert_eq!(app.mode, Mode::ChildCount);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::PlanSwarm)?;
        assert_eq!(app.mode, Mode::ChildCount);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::AddChildren)?;
        assert_eq!(app.mode, Mode::ChildCount);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::ReviewSwarm)?;
        assert_eq!(app.mode, Mode::ReviewChildCount);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Broadcast)?;
        assert_eq!(app.mode, Mode::Broadcasting);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::SpawnTerminalPrompted)?;
        assert_eq!(app.mode, Mode::TerminalPrompt);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Push)?;
        assert_eq!(app.mode, Mode::ConfirmPush);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::RenameBranch)?;
        assert_eq!(app.mode, Mode::RenameBranch);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Rebase)?;
        assert_eq!(app.mode, Mode::RebaseBranchSelector);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Merge)?;
        assert_eq!(app.mode, Mode::MergeBranchSelector);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Help)?;
        assert_eq!(app.mode, Mode::Help);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::CommandPalette)?;
        assert_eq!(app.mode, Mode::CommandPalette);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::NewAgent)?;
        assert_eq!(app.mode, Mode::Creating);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::NewAgentWithPrompt)?;
        assert_eq!(app.mode, Mode::Prompting);
        app.exit_mode();
        app.enter_mode(Mode::Scrolling);

        dispatch_scrolling_mode(&mut app, Actions::new(), KeyAction::Cancel)?;
        assert_eq!(app.mode, Mode::Normal);

        std::env::set_current_dir(original_dir)?;
        Ok(())
    }
}
