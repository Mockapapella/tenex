use crate::action::{CancelAction, ValidIn};
use crate::app::{Actions, App, AppData};
use crate::state::{
    BroadcastingMode, ChildPromptMode, CreatingMode, CustomAgentCommandMode, ErrorModalMode,
    ModeUnion, PromptingMode, ReconnectPromptMode, TerminalPromptMode,
};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use uuid::Uuid;

/// Text-input action: insert a character.
#[derive(Debug, Clone, Copy, Default)]
pub struct CharInputAction(pub char);

/// Text-input action: delete previous character (backspace).
#[derive(Debug, Clone, Copy, Default)]
pub struct BackspaceAction;

/// Text-input action: delete character at cursor.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeleteAction;

/// Text-input action: move cursor left.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorLeftAction;

/// Text-input action: move cursor right.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorRightAction;

/// Text-input action: move cursor up (multiline).
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorUpAction;

/// Text-input action: move cursor down (multiline).
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorDownAction;

/// Text-input action: move cursor to start of line.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorHomeAction;

/// Text-input action: move cursor to end of line.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorEndAction;

/// Text-input action: clear the current input line/buffer.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClearLineAction;

/// Text-input action: delete the previous word.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeleteWordAction;

/// Text-input action: submit (Enter) in the current mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubmitAction;

/// Dispatch a raw key event in a text input state via typed actions.
///
/// This keeps raw key handling (Enter/Esc/backspace/cursor keys) out of the
/// legacy runtime mode checks.
///
/// # Errors
///
/// Returns an error if a submitted action fails.
pub fn dispatch_text_input_mode<State>(
    app: &mut App,
    actions: Actions,
    state: State,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()>
where
    State: Copy,
    ModeUnion: From<State>,
    CancelAction: ValidIn<State, NextState = ModeUnion>,
    SubmitAction: ValidIn<State, NextState = ModeUnion>,
    CharInputAction: ValidIn<State, NextState = ModeUnion>,
    BackspaceAction: ValidIn<State, NextState = ModeUnion>,
    DeleteAction: ValidIn<State, NextState = ModeUnion>,
    CursorLeftAction: ValidIn<State, NextState = ModeUnion>,
    CursorRightAction: ValidIn<State, NextState = ModeUnion>,
    CursorUpAction: ValidIn<State, NextState = ModeUnion>,
    CursorDownAction: ValidIn<State, NextState = ModeUnion>,
    CursorHomeAction: ValidIn<State, NextState = ModeUnion>,
    CursorEndAction: ValidIn<State, NextState = ModeUnion>,
    ClearLineAction: ValidIn<State, NextState = ModeUnion>,
    DeleteWordAction: ValidIn<State, NextState = ModeUnion>,
{
    let mut app_data = AppData::new(app, actions);

    let next = match (code, modifiers) {
        (KeyCode::Enter, mods) if mods.contains(KeyModifiers::ALT) => {
            CharInputAction('\n').execute(state, &mut app_data)?
        }
        (KeyCode::Enter, _) => SubmitAction.execute(state, &mut app_data)?,
        (KeyCode::Esc, _) => CancelAction.execute(state, &mut app_data)?,
        (KeyCode::Char('u' | 'U'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            ClearLineAction.execute(state, &mut app_data)?
        }
        (KeyCode::Char('w' | 'W'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            DeleteWordAction.execute(state, &mut app_data)?
        }
        (KeyCode::Char(c), _) => CharInputAction(c).execute(state, &mut app_data)?,
        (KeyCode::Backspace, _) => BackspaceAction.execute(state, &mut app_data)?,
        (KeyCode::Delete, _) => DeleteAction.execute(state, &mut app_data)?,
        (KeyCode::Left, _) => CursorLeftAction.execute(state, &mut app_data)?,
        (KeyCode::Right, _) => CursorRightAction.execute(state, &mut app_data)?,
        (KeyCode::Up, _) => CursorUpAction.execute(state, &mut app_data)?,
        (KeyCode::Down, _) => CursorDownAction.execute(state, &mut app_data)?,
        (KeyCode::Home, _) => CursorHomeAction.execute(state, &mut app_data)?,
        (KeyCode::End, _) => CursorEndAction.execute(state, &mut app_data)?,

        _ => state.into(),
    };

    next.apply(app_data.app);
    Ok(())
}

impl ValidIn<CreatingMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for ClearLineAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for DeleteWordAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<CreatingMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if input.is_empty() {
            return Ok(ModeUnion::normal());
        }

        let original_mode = app_data.mode.clone();
        let result = app_data.actions.create_agent(app_data.app, &input, None);

        match result {
            Ok(()) => {
                if app_data.mode == original_mode {
                    Ok(ModeUnion::normal())
                } else {
                    Ok(ModeUnion::Legacy(app_data.mode.clone()))
                }
            }
            Err(err) => Ok(ErrorModalMode {
                message: format!("Failed: {err:#}"),
            }
            .into()),
        }
    }
}

impl ValidIn<PromptingMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let short_id = &Uuid::new_v4().to_string()[..8];
        let title = format!("Agent ({short_id})");
        let prompt = if input.is_empty() {
            None
        } else {
            Some(input.as_str())
        };

        let original_mode = app_data.mode.clone();
        let result = app_data.actions.create_agent(app_data.app, &title, prompt);

        match result {
            Ok(()) => {
                if app_data.mode == original_mode {
                    Ok(ModeUnion::normal())
                } else {
                    Ok(ModeUnion::Legacy(app_data.mode.clone()))
                }
            }
            Err(err) => Ok(ErrorModalMode {
                message: format!("Failed: {err:#}"),
            }
            .into()),
        }
    }
}

impl ValidIn<ChildPromptMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let prompt = if input.is_empty() {
            None
        } else {
            Some(input.as_str())
        };

        let original_mode = app_data.mode.clone();
        let result = app_data.actions.spawn_children(app_data.app, prompt);

        match result {
            Ok(()) => {
                if app_data.mode == original_mode {
                    Ok(ModeUnion::normal())
                } else {
                    Ok(ModeUnion::Legacy(app_data.mode.clone()))
                }
            }
            Err(err) => Ok(ErrorModalMode {
                message: format!("Failed: {err:#}"),
            }
            .into()),
        }
    }
}

impl ValidIn<BroadcastingMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if input.is_empty() {
            return Ok(ModeUnion::normal());
        }

        let original_mode = app_data.mode.clone();
        let result = app_data.actions.broadcast_to_leaves(app_data.app, &input);

        match result {
            Ok(()) => {
                if app_data.mode == original_mode {
                    Ok(ModeUnion::normal())
                } else {
                    Ok(ModeUnion::Legacy(app_data.mode.clone()))
                }
            }
            Err(err) => Ok(ErrorModalMode {
                message: format!("Failed: {err:#}"),
            }
            .into()),
        }
    }
}

impl ValidIn<ReconnectPromptMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if let Some(ref mut conflict) = app_data.spawn.worktree_conflict {
            conflict.prompt = if input.is_empty() { None } else { Some(input) };
        }

        let original_mode = app_data.mode.clone();
        let result = app_data.actions.reconnect_to_worktree(app_data.app);

        match result {
            Ok(()) => {
                if app_data.mode == original_mode {
                    Ok(ModeUnion::normal())
                } else {
                    Ok(ModeUnion::Legacy(app_data.mode.clone()))
                }
            }
            Err(err) => Ok(ErrorModalMode {
                message: format!("Failed: {err:#}"),
            }
            .into()),
        }
    }
}

impl ValidIn<TerminalPromptMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let command = if input.is_empty() {
            None
        } else {
            Some(input.as_str())
        };

        let original_mode = app_data.mode.clone();
        let result = app_data.actions.spawn_terminal(app_data.app, command);

        match result {
            Ok(()) => {
                if app_data.mode == original_mode {
                    Ok(ModeUnion::normal())
                } else {
                    Ok(ModeUnion::Legacy(app_data.mode.clone()))
                }
            }
            Err(err) => Ok(ErrorModalMode {
                message: format!("Failed: {err:#}"),
            }
            .into()),
        }
    }
}

impl ValidIn<CustomAgentCommandMode> for SubmitAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if input.trim().is_empty() {
            app_data.set_status("Custom agent command cannot be empty");
            return Ok(CustomAgentCommandMode.into());
        }

        let command = input.trim().to_string();
        let original_mode = app_data.mode.clone();
        app_data.set_custom_agent_command_and_save(command);

        if app_data.mode == original_mode {
            Ok(ModeUnion::normal())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<CreatingMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(self, _state: CreatingMode, _app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<PromptingMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: PromptingMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ChildPromptMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildPromptMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<BroadcastingMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BroadcastingMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ReconnectPromptMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.spawn.worktree_conflict = None;
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<TerminalPromptMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: TerminalPromptMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<CustomAgentCommandMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}
