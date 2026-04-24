use crate::action::{CancelAction, ValidIn};
use crate::app::{Actions, App, AppData};
use crate::state::{
    AppMode, BroadcastingMode, ChildPromptMode, CreatingMode, CustomAgentCommandMode,
    ErrorModalMode, PromptingMode, ReconnectPromptMode, SynthesisPromptMode, TerminalPromptMode,
};
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use uuid::Uuid;

#[cfg(test)]
thread_local! {
    static TEST_FORCE_TEXT_INPUT_DISPATCH_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
/// Run `f` with text input dispatch forced to return an error.
///
/// This is test-only scaffolding used to assert error propagation without
/// relying on external state.
pub fn with_forced_text_input_dispatch_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    TEST_FORCE_TEXT_INPUT_DISPATCH_ERROR.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

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
    state: State,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()>
where
    State: Copy,
    AppMode: From<State>,
    CancelAction: ValidIn<State, NextState = AppMode>,
    SubmitAction: ValidIn<State, NextState = AppMode>,
    CharInputAction: ValidIn<State, NextState = AppMode>,
    BackspaceAction: ValidIn<State, NextState = AppMode>,
    DeleteAction: ValidIn<State, NextState = AppMode>,
    CursorLeftAction: ValidIn<State, NextState = AppMode>,
    CursorRightAction: ValidIn<State, NextState = AppMode>,
    CursorUpAction: ValidIn<State, NextState = AppMode>,
    CursorDownAction: ValidIn<State, NextState = AppMode>,
    CursorHomeAction: ValidIn<State, NextState = AppMode>,
    CursorEndAction: ValidIn<State, NextState = AppMode>,
    ClearLineAction: ValidIn<State, NextState = AppMode>,
    DeleteWordAction: ValidIn<State, NextState = AppMode>,
{
    #[cfg(test)]
    if TEST_FORCE_TEXT_INPUT_DISPATCH_ERROR.with(std::cell::Cell::get) {
        return Err(anyhow::anyhow!("Forced text input dispatch error"));
    }

    let app_data = &mut app.data;
    let next = match (code, modifiers) {
        (KeyCode::Enter, mods) if mods.contains(KeyModifiers::ALT) => {
            CharInputAction('\n').execute(state, app_data)
        }
        (KeyCode::Enter, _) => SubmitAction.execute(state, app_data),
        (KeyCode::Esc, _) => CancelAction.execute(state, app_data),
        (KeyCode::Char('u' | 'U'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            ClearLineAction.execute(state, app_data)
        }
        (KeyCode::Char('w' | 'W'), mods) if mods.contains(KeyModifiers::CONTROL) => {
            DeleteWordAction.execute(state, app_data)
        }
        (KeyCode::Char(c), _) => CharInputAction(c).execute(state, app_data),
        (KeyCode::Backspace, _) => BackspaceAction.execute(state, app_data),
        (KeyCode::Delete, _) => DeleteAction.execute(state, app_data),
        (KeyCode::Left, _) => CursorLeftAction.execute(state, app_data),
        (KeyCode::Right, _) => CursorRightAction.execute(state, app_data),
        (KeyCode::Up, _) => CursorUpAction.execute(state, app_data),
        (KeyCode::Down, _) => CursorDownAction.execute(state, app_data),
        (KeyCode::Home, _) => CursorHomeAction.execute(state, app_data),
        (KeyCode::End, _) => CursorEndAction.execute(state, app_data),
        _ => Ok(state.into()),
    }?;

    app.apply_mode(next);
    Ok(())
}

fn ok_or_error_modal(result: Result<AppMode>) -> Result<AppMode> {
    result.or_else(|err| {
        Ok(ErrorModalMode {
            message: format!("Failed: {err:#}"),
        }
        .into())
    })
}

impl ValidIn<CreatingMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CharInputAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for BackspaceAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_backspace();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for DeleteAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CursorLeftAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CursorRightAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CursorUpAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_up();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CursorDownAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_down();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CursorHomeAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for CursorEndAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for ClearLineAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.clear_line();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(CreatingMode.into())
    }
}

impl ValidIn<PromptingMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ChildPromptMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<BroadcastingMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(BroadcastingMode.into())
    }
}

impl ValidIn<ReconnectPromptMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(ReconnectPromptMode.into())
    }
}

impl ValidIn<TerminalPromptMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(TerminalPromptMode.into())
    }
}

impl ValidIn<CustomAgentCommandMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(CustomAgentCommandMode.into())
    }
}

impl ValidIn<SynthesisPromptMode> for DeleteWordAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.input.delete_word();
        Ok(SynthesisPromptMode.into())
    }
}

impl ValidIn<CreatingMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if input.is_empty() {
            return Ok(AppMode::normal());
        }

        ok_or_error_modal(Actions::new().create_agent(app_data, &input, None))
    }
}

impl ValidIn<PromptingMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let short_id = &Uuid::new_v4().to_string()[..8];
        let title = format!("Agent ({short_id})");
        let prompt = if input.is_empty() {
            None
        } else {
            Some(input.as_str())
        };

        ok_or_error_modal(Actions::new().create_agent(app_data, &title, prompt))
    }
}

impl ValidIn<ChildPromptMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let prompt = if input.is_empty() {
            None
        } else {
            Some(input.as_str())
        };

        ok_or_error_modal(Actions::new().spawn_children(app_data, prompt))
    }
}

impl ValidIn<BroadcastingMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if input.is_empty() {
            return Ok(AppMode::normal());
        }

        ok_or_error_modal(Actions::new().broadcast_to_leaves(app_data, &input))
    }
}

impl ValidIn<ReconnectPromptMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if let Some(ref mut conflict) = app_data.spawn.worktree_conflict {
            conflict.prompt = if input.is_empty() { None } else { Some(input) };
        }

        ok_or_error_modal(Actions::new().reconnect_to_worktree(app_data))
    }
}

impl ValidIn<TerminalPromptMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let command = if input.is_empty() {
            None
        } else {
            Some(input.as_str())
        };

        ok_or_error_modal(Actions::new().spawn_terminal(app_data, command))
    }
}

impl ValidIn<CustomAgentCommandMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();

        if input.trim().is_empty() {
            app_data.set_status("Custom agent command cannot be empty");
            return Ok(CustomAgentCommandMode.into());
        }

        let role = app_data.model_selector.role;
        let command = input.trim().to_string();

        match role {
            crate::app::AgentRole::Default => {
                app_data.settings.custom_agent_command = command;
                app_data.settings.agent_program = crate::app::AgentProgram::Custom;
            }
            crate::app::AgentRole::Planner => {
                app_data.settings.planner_custom_agent_command = command;
                app_data.settings.planner_agent_program = crate::app::AgentProgram::Custom;
            }
            crate::app::AgentRole::Review => {
                app_data.settings.review_custom_agent_command = command;
                app_data.settings.review_agent_program = crate::app::AgentProgram::Custom;
            }
        }

        if let Err(err) = app_data.settings.save() {
            return Ok(ErrorModalMode {
                message: format!("Failed to save settings: {err}"),
            }
            .into());
        }

        app_data.set_status(format!("{} set to custom", role.menu_label()));
        Ok(AppMode::normal())
    }
}

impl ValidIn<SynthesisPromptMode> for SubmitAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        let input = app_data.input.buffer.clone();
        let prompt = input.trim();
        let prompt = if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        };

        ok_or_error_modal(Actions::new().synthesize_with_prompt(app_data, prompt))
    }
}

impl ValidIn<CreatingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: CreatingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<PromptingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: PromptingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<ChildPromptMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: ChildPromptMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<BroadcastingMode> for CancelAction {
    type NextState = AppMode;

    fn execute(self, _state: BroadcastingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<ReconnectPromptMode> for CancelAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: ReconnectPromptMode,
        app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        app_data.spawn.worktree_conflict = None;
        Ok(AppMode::normal())
    }
}

impl ValidIn<TerminalPromptMode> for CancelAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: TerminalPromptMode,
        _app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<CustomAgentCommandMode> for CancelAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: CustomAgentCommandMode,
        _app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

impl ValidIn<SynthesisPromptMode> for CancelAction {
    type NextState = AppMode;

    fn execute(
        self,
        _state: SynthesisPromptMode,
        _app_data: &mut AppData,
    ) -> Result<Self::NextState> {
        Ok(AppMode::normal())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::app::{AgentProgram, AgentRole, WorktreeConflictInfo};
    use crate::config::Config;
    use anyhow::anyhow;
    use tempfile::{NamedTempFile, TempDir};

    fn error_message(mode: AppMode) -> Option<String> {
        match mode {
            AppMode::ErrorModal(state) => Some(state.message),
            _ => None,
        }
    }

    #[test]
    fn test_ok_or_error_modal_returns_error_modal_for_err() {
        let ok_mode = ok_or_error_modal(Ok(AppMode::normal())).unwrap();
        assert_eq!(error_message(ok_mode), None);

        let modal_mode = ok_or_error_modal(Err(anyhow!("boom"))).unwrap();
        let message = error_message(modal_mode).unwrap();
        assert!(message.starts_with("Failed:"));
        assert!(message.contains("boom"));
    }

    #[derive(Clone, Copy)]
    struct TestState;

    impl From<TestState> for AppMode {
        fn from(_value: TestState) -> Self {
            Self::normal()
        }
    }

    impl ValidIn<TestState> for CancelAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for SubmitAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CharInputAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            if self.0 == 'x' {
                return Err(anyhow!("boom"));
            }
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for BackspaceAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for DeleteAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CursorLeftAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CursorRightAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CursorUpAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CursorDownAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CursorHomeAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for CursorEndAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for ClearLineAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    impl ValidIn<TestState> for DeleteWordAction {
        type NextState = AppMode;

        fn execute(self, _state: TestState, _app_data: &mut AppData) -> Result<Self::NextState> {
            Ok(AppMode::normal())
        }
    }

    #[test]
    fn test_dispatch_text_input_mode_propagates_errors_from_actions() {
        let mut app = App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        let error =
            dispatch_text_input_mode(&mut app, TestState, KeyCode::Char('x'), KeyModifiers::NONE)
                .unwrap_err();
        assert!(format!("{error}").contains("boom"));
    }

    #[test]
    fn test_dispatch_text_input_mode_forced_error_covers_test_state_branch() {
        with_forced_text_input_dispatch_error_for_tests(|| {
            let mut app = App::new(
                Config::default(),
                Storage::default(),
                Settings::default(),
                false,
            );

            let error = dispatch_text_input_mode(
                &mut app,
                TestState,
                KeyCode::Char('a'),
                KeyModifiers::NONE,
            )
            .unwrap_err();
            assert!(format!("{error}").contains("Forced text input dispatch error"));
        });
    }

    #[test]
    fn test_dispatch_text_input_mode_applies_mode_on_success() {
        let mut app = App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );

        dispatch_text_input_mode(&mut app, TestState, KeyCode::Enter, KeyModifiers::ALT).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Enter, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Esc, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(
            &mut app,
            TestState,
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )
        .unwrap();
        dispatch_text_input_mode(
            &mut app,
            TestState,
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        )
        .unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Char('a'), KeyModifiers::NONE)
            .unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Backspace, KeyModifiers::NONE)
            .unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Delete, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Left, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Right, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Up, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Down, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::Home, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::End, KeyModifiers::NONE).unwrap();
        dispatch_text_input_mode(&mut app, TestState, KeyCode::F(13), KeyModifiers::NONE).unwrap();
        assert_eq!(app.mode, AppMode::normal());
    }

    #[test]
    fn test_dispatch_text_input_mode_covers_modifier_branches_for_real_modes() {
        fn exercise<State>(state: State)
        where
            State: Copy,
            AppMode: From<State>,
            CancelAction: ValidIn<State, NextState = AppMode>,
            SubmitAction: ValidIn<State, NextState = AppMode>,
            CharInputAction: ValidIn<State, NextState = AppMode>,
            BackspaceAction: ValidIn<State, NextState = AppMode>,
            DeleteAction: ValidIn<State, NextState = AppMode>,
            CursorLeftAction: ValidIn<State, NextState = AppMode>,
            CursorRightAction: ValidIn<State, NextState = AppMode>,
            CursorUpAction: ValidIn<State, NextState = AppMode>,
            CursorDownAction: ValidIn<State, NextState = AppMode>,
            CursorHomeAction: ValidIn<State, NextState = AppMode>,
            CursorEndAction: ValidIn<State, NextState = AppMode>,
            ClearLineAction: ValidIn<State, NextState = AppMode>,
            DeleteWordAction: ValidIn<State, NextState = AppMode>,
        {
            let mut app = App::new(
                Config::default(),
                Storage::default(),
                Settings::default(),
                false,
            );

            dispatch_text_input_mode(&mut app, state, KeyCode::Enter, KeyModifiers::NONE).unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Enter, KeyModifiers::ALT).unwrap();

            dispatch_text_input_mode(&mut app, state, KeyCode::Char('u'), KeyModifiers::NONE)
                .unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Char('u'), KeyModifiers::CONTROL)
                .unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Char('U'), KeyModifiers::NONE)
                .unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Char('U'), KeyModifiers::CONTROL)
                .unwrap();

            dispatch_text_input_mode(&mut app, state, KeyCode::Char('w'), KeyModifiers::NONE)
                .unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Char('w'), KeyModifiers::CONTROL)
                .unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Char('W'), KeyModifiers::NONE)
                .unwrap();
            dispatch_text_input_mode(&mut app, state, KeyCode::Char('W'), KeyModifiers::CONTROL)
                .unwrap();
        }

        exercise(CreatingMode);
        exercise(PromptingMode);
        exercise(ChildPromptMode);
        exercise(BroadcastingMode);
        exercise(ReconnectPromptMode);
        exercise(TerminalPromptMode);
        exercise(CustomAgentCommandMode);
        exercise(SynthesisPromptMode);
    }

    fn create_test_data() -> (AppData, NamedTempFile) {
        let temp_file = NamedTempFile::new().expect("temp state file should be created");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            AppData::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    #[test]
    fn test_submit_action_prompting_mode_covers_prompt_branches() {
        let docker_dir = TempDir::new().expect("docker script dir");
        let docker_path = docker_dir.path().join("docker");
        std::fs::write(&docker_path, "#!/usr/bin/env sh\nexit 1\n").expect("write docker script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(&docker_path)
                .expect("read docker script metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&docker_path, perms).expect("chmod docker script");
        }

        crate::runtime::with_docker_program_override_for_tests(docker_path, || {
            let cwd_dir = TempDir::new().expect("cwd dir");
            for buffer in ["", "hello"] {
                let (mut data, _temp) = create_test_data();
                data.settings.docker_for_new_roots = true;
                data.cwd_project_root = Some(cwd_dir.path().to_path_buf());
                data.input.buffer = buffer.to_string();

                let next = SubmitAction
                    .execute(PromptingMode, &mut data)
                    .expect("submit prompting mode");
                let message = error_message(next).expect("expected error modal");
                assert!(message.starts_with("Failed:"));
            }
        });
    }

    #[test]
    fn test_submit_action_child_prompt_mode_covers_prompt_branches() {
        for buffer in ["", "do the thing"] {
            let (mut data, _temp) = create_test_data();
            data.spawn.spawning_under = Some(uuid::Uuid::new_v4());
            data.input.buffer = buffer.to_string();

            let next = SubmitAction
                .execute(ChildPromptMode, &mut data)
                .expect("submit child prompt mode");
            let message = error_message(next).expect("expected error modal");
            assert!(message.starts_with("Failed:"));
        }
    }

    #[test]
    fn test_submit_action_reconnect_prompt_mode_covers_prompt_branches() {
        let docker_dir = TempDir::new().expect("docker script dir");
        let docker_path = docker_dir.path().join("docker");
        std::fs::write(&docker_path, "#!/usr/bin/env sh\nexit 1\n").expect("write docker script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = std::fs::metadata(&docker_path)
                .expect("read docker script metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&docker_path, perms).expect("chmod docker script");
        }

        crate::runtime::with_docker_program_override_for_tests(docker_path, || {
            let worktree_dir = TempDir::new().expect("worktree dir");
            let repo_root = TempDir::new().expect("repo root");

            for buffer in ["", "prompt"] {
                let (mut data, _temp) = create_test_data();
                data.settings.docker_for_new_roots = true;
                data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
                    title: "test agent".to_string(),
                    prompt: None,
                    branch: "agent/test".to_string(),
                    worktree_path: worktree_dir.path().to_path_buf(),
                    repo_root: repo_root.path().to_path_buf(),
                    existing_branch: None,
                    existing_commit: None,
                    current_branch: "master".to_string(),
                    current_commit: "deadbeef".to_string(),
                    swarm_child_count: None,
                });
                data.input.buffer = buffer.to_string();

                let next = SubmitAction
                    .execute(ReconnectPromptMode, &mut data)
                    .expect("submit reconnect prompt mode");
                let message = error_message(next).expect("expected error modal");
                assert!(message.starts_with("Failed:"));
            }
        });
    }

    #[test]
    fn test_submit_action_terminal_prompt_mode_covers_command_branches() {
        for buffer in ["", "git status"] {
            let (mut data, _temp) = create_test_data();
            data.input.buffer = buffer.to_string();

            let next = SubmitAction
                .execute(TerminalPromptMode, &mut data)
                .expect("submit terminal prompt mode");
            let message = error_message(next).expect("expected error modal");
            assert!(message.starts_with("Failed:"));
        }
    }

    #[test]
    fn test_submit_action_custom_agent_command_mode_rejects_empty_input() {
        let (mut data, _temp) = create_test_data();
        data.model_selector.role = AgentRole::Default;
        data.input.buffer = "   ".to_string();

        let next = SubmitAction
            .execute(CustomAgentCommandMode, &mut data)
            .expect("submit custom agent command");
        assert_eq!(
            std::mem::discriminant(&next),
            std::mem::discriminant(&AppMode::CustomAgentCommand(CustomAgentCommandMode))
        );
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Custom agent command cannot be empty")
        );
    }

    #[test]
    fn test_submit_action_custom_agent_command_mode_sets_custom_command_for_each_role() {
        let temp_dir = TempDir::new().expect("temp settings dir");
        Settings::set_test_path_override(temp_dir.path().join("settings.json"))
            .expect("set test settings path override");

        let (mut data, _temp) = create_test_data();
        let command = "echo hello";

        for role in AgentRole::ALL {
            data.settings = Settings::default();
            data.model_selector.role = *role;
            data.input.buffer = command.to_string();

            let next = SubmitAction
                .execute(CustomAgentCommandMode, &mut data)
                .expect("submit custom agent command");
            assert_eq!(next, AppMode::normal());

            match *role {
                AgentRole::Default => {
                    assert_eq!(data.settings.custom_agent_command, command);
                    assert_eq!(data.settings.agent_program, AgentProgram::Custom);
                }
                AgentRole::Planner => {
                    assert_eq!(data.settings.planner_custom_agent_command, command);
                    assert_eq!(data.settings.planner_agent_program, AgentProgram::Custom);
                }
                AgentRole::Review => {
                    assert_eq!(data.settings.review_custom_agent_command, command);
                    assert_eq!(data.settings.review_agent_program, AgentProgram::Custom);
                }
            }

            let expected_status = format!("{} set to custom", role.menu_label());
            assert_eq!(
                data.ui.status_message.as_deref(),
                Some(expected_status.as_str())
            );
        }
    }

    #[test]
    fn test_submit_action_custom_agent_command_mode_returns_error_modal_when_save_fails() {
        let temp_file = NamedTempFile::new().expect("temp state file should be created");
        Settings::set_test_path_override(temp_file.path().join("settings.json"))
            .expect("set test settings path override");

        let (mut data, _temp) = create_test_data();
        data.model_selector.role = AgentRole::Default;
        data.input.buffer = "echo hello".to_string();

        let next = SubmitAction
            .execute(CustomAgentCommandMode, &mut data)
            .expect("submit custom agent command");
        let message = error_message(next).expect("expected error modal");
        assert!(message.starts_with("Failed to save settings:"));
    }

    #[test]
    fn test_submit_action_synthesis_prompt_mode_covers_prompt_branches() {
        for buffer in ["   ", "follow these steps"] {
            let (mut data, _temp) = create_test_data();
            data.input.buffer = buffer.to_string();

            let next = SubmitAction
                .execute(SynthesisPromptMode, &mut data)
                .expect("submit synthesis prompt mode");
            let message = error_message(next).expect("expected error modal");
            assert_eq!(message, "No agent selected");
        }
    }
}
