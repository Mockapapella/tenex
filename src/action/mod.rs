//! Compile-time action types (new architecture).

mod agent;
mod git;
mod misc;
mod navigation;
mod text_input;

pub use agent::*;
pub use git::*;
pub use misc::*;
pub use navigation::*;
pub use text_input::*;

use crate::app::{Actions, App, AppData};
use crate::config::Action as KeyAction;
use crate::state::{
    BroadcastingMode, ChildPromptMode, CreatingMode, CustomAgentCommandMode, ModeUnion, NormalMode,
    PromptingMode, ReconnectPromptMode, ScrollingMode, TerminalPromptMode,
};
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
