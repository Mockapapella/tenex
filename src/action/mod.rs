//! Compile-time action types (new architecture).

mod agent;
mod git;
mod misc;
mod navigation;

pub use agent::*;
pub use git::*;
pub use misc::*;
pub use navigation::*;

use crate::app::{Actions, App, AppData};
use crate::config::Action as KeyAction;
use crate::state::{ModeUnion, NormalMode};
use anyhow::Result;

/// Marker trait: This action is valid in this state.
///
/// Each impl is an explicit entry in the "registry" of valid combinations.
pub trait ValidIn<State> {
    /// The next state produced after executing this action in `State`.
    type NextState;

    /// Execute this action in `State`, producing the next state.
    ///
    /// # Errors
    ///
    /// Returns an error if executing the action fails.
    fn execute(state: State, app_data: &mut AppData<'_>) -> Result<Self::NextState>;
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
        KeyAction::NewAgent => NewAgentAction::execute(NormalMode, &mut app_data)?,
        KeyAction::NewAgentWithPrompt => {
            NewAgentWithPromptAction::execute(NormalMode, &mut app_data)?
        }
        KeyAction::Help => HelpAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Quit => QuitAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Kill => KillAction::execute(NormalMode, &mut app_data)?,
        KeyAction::SwitchTab => SwitchTabAction::execute(NormalMode, &mut app_data)?,
        KeyAction::NextAgent => NextAgentAction::execute(NormalMode, &mut app_data)?,
        KeyAction::PrevAgent => PrevAgentAction::execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollUp => ScrollUpAction::execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollDown => ScrollDownAction::execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollTop => ScrollTopAction::execute(NormalMode, &mut app_data)?,
        KeyAction::ScrollBottom => ScrollBottomAction::execute(NormalMode, &mut app_data)?,
        KeyAction::FocusPreview => FocusPreviewAction::execute(NormalMode, &mut app_data)?,
        KeyAction::SpawnChildren => SpawnChildrenAction::execute(NormalMode, &mut app_data)?,
        KeyAction::PlanSwarm => PlanSwarmAction::execute(NormalMode, &mut app_data)?,
        KeyAction::AddChildren => AddChildrenAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Synthesize => SynthesizeAction::execute(NormalMode, &mut app_data)?,
        KeyAction::ToggleCollapse => ToggleCollapseAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Broadcast => BroadcastAction::execute(NormalMode, &mut app_data)?,
        KeyAction::ReviewSwarm => ReviewSwarmAction::execute(NormalMode, &mut app_data)?,
        KeyAction::SpawnTerminal => SpawnTerminalAction::execute(NormalMode, &mut app_data)?,
        KeyAction::SpawnTerminalPrompted => {
            SpawnTerminalPromptedAction::execute(NormalMode, &mut app_data)?
        }
        KeyAction::Push => PushAction::execute(NormalMode, &mut app_data)?,
        KeyAction::RenameBranch => RenameBranchAction::execute(NormalMode, &mut app_data)?,
        KeyAction::OpenPR => OpenPRAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Rebase => RebaseAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Merge => MergeAction::execute(NormalMode, &mut app_data)?,
        KeyAction::CommandPalette => CommandPaletteAction::execute(NormalMode, &mut app_data)?,
        KeyAction::Cancel => CancelAction::execute(NormalMode, &mut app_data)?,

        // Not valid in Normal mode; keep legacy behavior (no-op) for now.
        KeyAction::Confirm | KeyAction::UnfocusPreview => ModeUnion::normal(),
    };

    next.apply(app_data.app);
    Ok(())
}
