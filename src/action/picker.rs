//! Picker-mode action types (new architecture).

use crate::action::{
    BackspaceAction, CancelAction, CharInputAction, CursorEndAction, CursorHomeAction,
    CursorLeftAction, CursorRightAction, DeleteAction, ValidIn,
};
use crate::app::{Actions, AppData};
use crate::state::{
    BranchSelectorMode, ChildCountMode, ChildPromptMode, CommandPaletteMode,
    MergeBranchSelectorMode, ModeUnion, ModelSelectorMode, RebaseBranchSelectorMode,
    ReviewChildCountMode, ReviewInfoMode,
};
use anyhow::Result;

/// Picker action: increment a count (Up key in count pickers).
#[derive(Debug, Clone, Copy, Default)]
pub struct IncrementAction;

/// Picker action: decrement a count (Down key in count pickers).
#[derive(Debug, Clone, Copy, Default)]
pub struct DecrementAction;

/// Picker action: confirm the current selection (Enter key).
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectAction;

/// Picker action: move selection up (Up key in list pickers).
#[derive(Debug, Clone, Copy, Default)]
pub struct NavigateUpAction;

/// Picker action: move selection down (Down key in list pickers).
#[derive(Debug, Clone, Copy, Default)]
pub struct NavigateDownAction;

/// Picker action: dismiss an informational modal (any key).
#[derive(Debug, Clone, Copy, Default)]
pub struct DismissAction;

impl ValidIn<ChildCountMode> for IncrementAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildCountMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.increment_child_count();
        Ok(ChildCountMode.into())
    }
}

impl ValidIn<ReviewChildCountMode> for IncrementAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReviewChildCountMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.increment_child_count();
        Ok(ReviewChildCountMode.into())
    }
}

impl ValidIn<ChildCountMode> for DecrementAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildCountMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.decrement_child_count();
        Ok(ChildCountMode.into())
    }
}

impl ValidIn<ReviewChildCountMode> for DecrementAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReviewChildCountMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.decrement_child_count();
        Ok(ReviewChildCountMode.into())
    }
}

impl ValidIn<ChildCountMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildCountMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ChildPromptMode.into())
    }
}

impl ValidIn<ReviewChildCountMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReviewChildCountMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(BranchSelectorMode.into())
    }
}

impl ValidIn<ReviewInfoMode> for DismissAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReviewInfoMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ChildCountMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ChildCountMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ReviewChildCountMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReviewChildCountMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ReviewInfoMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ReviewInfoMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<BranchSelectorMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_review_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<RebaseBranchSelectorMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RebaseBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        app_data.clear_review_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<MergeBranchSelectorMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: MergeBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.clear_git_op_state();
        app_data.clear_review_state();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ModelSelectorMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ModelSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.model_selector.clear();
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<CommandPaletteMode> for CancelAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<BranchSelectorMode> for NavigateUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_prev_branch();
        Ok(BranchSelectorMode.into())
    }
}

impl ValidIn<BranchSelectorMode> for NavigateDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_next_branch();
        Ok(BranchSelectorMode.into())
    }
}

impl ValidIn<RebaseBranchSelectorMode> for NavigateUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RebaseBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_prev_branch();
        Ok(RebaseBranchSelectorMode.into())
    }
}

impl ValidIn<RebaseBranchSelectorMode> for NavigateDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RebaseBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_next_branch();
        Ok(RebaseBranchSelectorMode.into())
    }
}

impl ValidIn<MergeBranchSelectorMode> for NavigateUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: MergeBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_prev_branch();
        Ok(MergeBranchSelectorMode.into())
    }
}

impl ValidIn<MergeBranchSelectorMode> for NavigateDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: MergeBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_next_branch();
        Ok(MergeBranchSelectorMode.into())
    }
}

impl ValidIn<ModelSelectorMode> for NavigateUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ModelSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_prev_model_program();
        Ok(ModelSelectorMode.into())
    }
}

impl ValidIn<ModelSelectorMode> for NavigateDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ModelSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_next_model_program();
        Ok(ModelSelectorMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for NavigateUpAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_prev_slash_command();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for NavigateDownAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.select_next_slash_command();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<BranchSelectorMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        if app_data.confirm_branch_selection()
            && let Err(err) = app_data.actions.spawn_review_agents(app_data.app)
        {
            app_data.set_error(format!("Failed to spawn review agents: {err:#}"));
        }

        Ok(ModeUnion::normal())
    }
}

impl ValidIn<RebaseBranchSelectorMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: RebaseBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();

        if app_data.confirm_rebase_merge_branch()
            && let Err(err) = Actions::execute_rebase(app_data.app)
        {
            app_data.set_error(format!("Rebase failed: {err:#}"));
        }

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<MergeBranchSelectorMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: MergeBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();

        if app_data.confirm_rebase_merge_branch()
            && let Err(err) = Actions::execute_merge(app_data.app)
        {
            app_data.set_error(format!("Merge failed: {err:#}"));
        }

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<ModelSelectorMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: ModelSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();
        app_data.confirm_model_program_selection();

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<CommandPaletteMode> for SelectAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        let original_mode = app_data.mode.clone();
        app_data.confirm_slash_command_selection();

        if app_data.mode == original_mode {
            Ok(state.into())
        } else {
            Ok(ModeUnion::Legacy(app_data.mode.clone()))
        }
    }
}

impl ValidIn<BranchSelectorMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_branch_filter_char(self.0);
        Ok(BranchSelectorMode.into())
    }
}

impl ValidIn<BranchSelectorMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: BranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_branch_filter_backspace();
        Ok(BranchSelectorMode.into())
    }
}

impl ValidIn<RebaseBranchSelectorMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RebaseBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_branch_filter_char(self.0);
        Ok(RebaseBranchSelectorMode.into())
    }
}

impl ValidIn<RebaseBranchSelectorMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: RebaseBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_branch_filter_backspace();
        Ok(RebaseBranchSelectorMode.into())
    }
}

impl ValidIn<MergeBranchSelectorMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: MergeBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_branch_filter_char(self.0);
        Ok(MergeBranchSelectorMode.into())
    }
}

impl ValidIn<MergeBranchSelectorMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: MergeBranchSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_branch_filter_backspace();
        Ok(MergeBranchSelectorMode.into())
    }
}

impl ValidIn<ModelSelectorMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ModelSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_model_filter_char(self.0);
        Ok(ModelSelectorMode.into())
    }
}

impl ValidIn<ModelSelectorMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ModelSelectorMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_model_filter_backspace();
        Ok(ModelSelectorMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for CharInputAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_char(self.0);
        app_data.reset_slash_command_selection();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for BackspaceAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        if app_data.input.buffer.trim() == "/" {
            return Ok(ModeUnion::normal());
        }

        app_data.handle_backspace();
        app_data.reset_slash_command_selection();

        if app_data.input.buffer.trim().is_empty() {
            Ok(ModeUnion::normal())
        } else {
            Ok(CommandPaletteMode.into())
        }
    }
}

impl ValidIn<CommandPaletteMode> for DeleteAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.handle_delete();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for CursorLeftAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_left();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for CursorRightAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_right();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for CursorHomeAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_home();
        Ok(CommandPaletteMode.into())
    }
}

impl ValidIn<CommandPaletteMode> for CursorEndAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: CommandPaletteMode,
        app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        app_data.input.cursor_end();
        Ok(CommandPaletteMode.into())
    }
}
