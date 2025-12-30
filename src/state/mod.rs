//! Compile-time state types (new architecture).

mod branch_selector;
mod broadcasting;
mod child_count;
mod child_prompt;
mod command_palette;
mod confirm_push;
mod confirm_push_for_pr;
mod confirming;
mod creating;
mod custom_agent_cmd;
mod error_modal;
mod help;
mod keyboard_remap_prompt;
mod merge_branch_selector;
mod model_selector;
mod normal;
mod preview_focused;
mod prompting;
mod rebase_branch_selector;
mod reconnect_prompt;
mod rename_branch;
mod review_child_count;
mod review_info;
mod scrolling;
mod success_modal;
mod terminal_prompt;
mod update_prompt;
mod update_requested;

pub use branch_selector::BranchSelectorMode;
pub use broadcasting::BroadcastingMode;
pub use child_count::ChildCountMode;
pub use child_prompt::ChildPromptMode;
pub use command_palette::CommandPaletteMode;
pub use confirm_push::ConfirmPushMode;
pub use confirm_push_for_pr::ConfirmPushForPRMode;
pub use confirming::ConfirmingMode;
pub use creating::CreatingMode;
pub use custom_agent_cmd::CustomAgentCommandMode;
pub use error_modal::ErrorModalMode;
pub use help::HelpMode;
pub use keyboard_remap_prompt::KeyboardRemapPromptMode;
pub use merge_branch_selector::MergeBranchSelectorMode;
pub use model_selector::ModelSelectorMode;
pub use normal::NormalMode;
pub use preview_focused::PreviewFocusedMode;
pub use prompting::PromptingMode;
pub use rebase_branch_selector::RebaseBranchSelectorMode;
pub use reconnect_prompt::ReconnectPromptMode;
pub use rename_branch::RenameBranchMode;
pub use review_child_count::ReviewChildCountMode;
pub use review_info::ReviewInfoMode;
pub use scrolling::ScrollingMode;
pub use success_modal::SuccessModalMode;
pub use terminal_prompt::TerminalPromptMode;
pub use update_prompt::UpdatePromptMode;
pub use update_requested::UpdateRequestedMode;

use crate::app::{App, Mode};

/// A transitional "next state" wrapper used during migration.
///
/// While migrating, we keep the existing runtime `Mode` enum on `App`, while
/// introducing typed state markers and a `ModeUnion` bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeUnion {
    /// Remain in (or return to) the new, dedicated `NormalMode`.
    Normal(NormalMode),
    /// Scrolling mode.
    Scrolling(ScrollingMode),
    /// Creating mode.
    Creating(CreatingMode),
    /// Prompting mode.
    Prompting(PromptingMode),
    /// Child prompt mode.
    ChildPrompt(ChildPromptMode),
    /// Broadcasting mode.
    Broadcasting(BroadcastingMode),
    /// Reconnect prompt mode.
    ReconnectPrompt(ReconnectPromptMode),
    /// Terminal prompt mode.
    TerminalPrompt(TerminalPromptMode),
    /// Custom agent command mode.
    CustomAgentCommand(CustomAgentCommandMode),
    /// Child count picker mode.
    ChildCount(ChildCountMode),
    /// Review child count picker mode.
    ReviewChildCount(ReviewChildCountMode),
    /// Review info mode.
    ReviewInfo(ReviewInfoMode),
    /// Branch selector mode.
    BranchSelector(BranchSelectorMode),
    /// Rebase branch selector mode.
    RebaseBranchSelector(RebaseBranchSelectorMode),
    /// Merge branch selector mode.
    MergeBranchSelector(MergeBranchSelectorMode),
    /// Model selector mode.
    ModelSelector(ModelSelectorMode),
    /// Command palette mode.
    CommandPalette(CommandPaletteMode),
    /// General confirmation mode (requires carrying the confirmed action).
    Confirming(ConfirmingMode),
    /// Confirm push mode.
    ConfirmPush(ConfirmPushMode),
    /// Confirm push for PR mode.
    ConfirmPushForPR(ConfirmPushForPRMode),
    /// Rename branch mode.
    RenameBranch(RenameBranchMode),
    /// Keyboard remap prompt mode.
    KeyboardRemapPrompt(KeyboardRemapPromptMode),
    /// Update prompt mode.
    UpdatePrompt(UpdatePromptMode),
    /// Update requested mode (input ignored).
    UpdateRequested(UpdateRequestedMode),
    /// Help overlay mode.
    Help(HelpMode),
    /// Error modal mode.
    ErrorModal(ErrorModalMode),
    /// Success modal mode.
    SuccessModal(SuccessModalMode),
    /// Preview focused mode.
    PreviewFocused(PreviewFocusedMode),
    /// Transition to a legacy runtime `Mode`.
    Legacy(Mode),
}

impl ModeUnion {
    /// Convenience constructor for `ModeUnion::Normal`.
    #[must_use]
    pub const fn normal() -> Self {
        Self::Normal(NormalMode)
    }

    /// Apply the mode transition to the legacy `App` state.
    #[expect(
        clippy::too_many_lines,
        reason = "This is the central match dispatcher for mode transitions during migration."
    )]
    pub fn apply(self, app: &mut App) {
        match self {
            Self::Normal(_) => {
                if app.mode != Mode::Normal {
                    app.exit_mode();
                }
            }
            Self::Scrolling(_) => {
                if app.mode != Mode::Scrolling {
                    app.enter_mode(Mode::Scrolling);
                }
            }
            Self::Creating(_) => {
                if app.mode != Mode::Creating {
                    app.enter_mode(Mode::Creating);
                }
            }
            Self::Prompting(_) => {
                if app.mode != Mode::Prompting {
                    app.enter_mode(Mode::Prompting);
                }
            }
            Self::ChildPrompt(_) => {
                if app.mode != Mode::ChildPrompt {
                    app.enter_mode(Mode::ChildPrompt);
                }
            }
            Self::Broadcasting(_) => {
                if app.mode != Mode::Broadcasting {
                    app.enter_mode(Mode::Broadcasting);
                }
            }
            Self::ReconnectPrompt(_) => {
                if app.mode != Mode::ReconnectPrompt {
                    app.enter_mode(Mode::ReconnectPrompt);
                }
            }
            Self::TerminalPrompt(_) => {
                if app.mode != Mode::TerminalPrompt {
                    app.enter_mode(Mode::TerminalPrompt);
                }
            }
            Self::CustomAgentCommand(_) => {
                if app.mode != Mode::CustomAgentCommand {
                    app.enter_mode(Mode::CustomAgentCommand);
                }
            }
            Self::ChildCount(_) => {
                if app.mode != Mode::ChildCount {
                    app.enter_mode(Mode::ChildCount);
                }
            }
            Self::ReviewChildCount(_) => {
                if app.mode != Mode::ReviewChildCount {
                    app.enter_mode(Mode::ReviewChildCount);
                }
            }
            Self::ReviewInfo(_) => {
                if app.mode != Mode::ReviewInfo {
                    app.enter_mode(Mode::ReviewInfo);
                }
            }
            Self::BranchSelector(_) => {
                if app.mode != Mode::BranchSelector {
                    app.enter_mode(Mode::BranchSelector);
                }
            }
            Self::RebaseBranchSelector(_) => {
                if app.mode != Mode::RebaseBranchSelector {
                    app.enter_mode(Mode::RebaseBranchSelector);
                }
            }
            Self::MergeBranchSelector(_) => {
                if app.mode != Mode::MergeBranchSelector {
                    app.enter_mode(Mode::MergeBranchSelector);
                }
            }
            Self::ModelSelector(_) => {
                if app.mode != Mode::ModelSelector {
                    app.start_model_selector();
                }
            }
            Self::CommandPalette(_) => {
                if app.mode != Mode::CommandPalette {
                    app.start_command_palette();
                }
            }
            Self::Confirming(state) => {
                if app.mode != Mode::Confirming(state.action) {
                    app.enter_mode(Mode::Confirming(state.action));
                }
            }
            Self::ConfirmPush(_) => {
                if app.mode != Mode::ConfirmPush {
                    app.enter_mode(Mode::ConfirmPush);
                }
            }
            Self::ConfirmPushForPR(_) => {
                if app.mode != Mode::ConfirmPushForPR {
                    app.enter_mode(Mode::ConfirmPushForPR);
                }
            }
            Self::RenameBranch(_) => {
                if app.mode != Mode::RenameBranch {
                    app.enter_mode(Mode::RenameBranch);
                }
            }
            Self::KeyboardRemapPrompt(_) => {
                if app.mode != Mode::KeyboardRemapPrompt {
                    app.enter_mode(Mode::KeyboardRemapPrompt);
                }
            }
            Self::UpdatePrompt(state) => match &app.mode {
                Mode::UpdatePrompt(info) if info == &state.info => {}
                _ => app.enter_mode(Mode::UpdatePrompt(state.info)),
            },
            Self::UpdateRequested(state) => match &app.mode {
                Mode::UpdateRequested(info) if info == &state.info => {}
                _ => app.enter_mode(Mode::UpdateRequested(state.info)),
            },
            Self::Help(_) => {
                if app.mode != Mode::Help {
                    app.enter_mode(Mode::Help);
                }
            }
            Self::ErrorModal(state) => match &app.mode {
                Mode::ErrorModal(message) if message == &state.message => {}
                _ => app.set_error(state.message),
            },
            Self::SuccessModal(state) => match &app.mode {
                Mode::SuccessModal(message) if message == &state.message => {}
                _ => app.show_success(state.message),
            },
            Self::PreviewFocused(_) => {
                if app.mode != Mode::PreviewFocused {
                    app.enter_mode(Mode::PreviewFocused);
                }
            }
            Self::Legacy(mode) => {
                if app.mode == mode {
                    return;
                }
                match mode {
                    Mode::CommandPalette => app.start_command_palette(),
                    Mode::ModelSelector => app.start_model_selector(),
                    Mode::ErrorModal(message) => app.set_error(message),
                    Mode::SuccessModal(message) => app.show_success(message),
                    other => app.enter_mode(other),
                }
            }
        }
    }
}

impl From<Mode> for ModeUnion {
    fn from(mode: Mode) -> Self {
        Self::Legacy(mode)
    }
}

impl From<NormalMode> for ModeUnion {
    fn from(_: NormalMode) -> Self {
        Self::Normal(NormalMode)
    }
}

impl From<ScrollingMode> for ModeUnion {
    fn from(_: ScrollingMode) -> Self {
        Self::Scrolling(ScrollingMode)
    }
}

impl From<CreatingMode> for ModeUnion {
    fn from(_: CreatingMode) -> Self {
        Self::Creating(CreatingMode)
    }
}

impl From<PromptingMode> for ModeUnion {
    fn from(_: PromptingMode) -> Self {
        Self::Prompting(PromptingMode)
    }
}

impl From<ChildPromptMode> for ModeUnion {
    fn from(_: ChildPromptMode) -> Self {
        Self::ChildPrompt(ChildPromptMode)
    }
}

impl From<BroadcastingMode> for ModeUnion {
    fn from(_: BroadcastingMode) -> Self {
        Self::Broadcasting(BroadcastingMode)
    }
}

impl From<ReconnectPromptMode> for ModeUnion {
    fn from(_: ReconnectPromptMode) -> Self {
        Self::ReconnectPrompt(ReconnectPromptMode)
    }
}

impl From<TerminalPromptMode> for ModeUnion {
    fn from(_: TerminalPromptMode) -> Self {
        Self::TerminalPrompt(TerminalPromptMode)
    }
}

impl From<CustomAgentCommandMode> for ModeUnion {
    fn from(_: CustomAgentCommandMode) -> Self {
        Self::CustomAgentCommand(CustomAgentCommandMode)
    }
}

impl From<ChildCountMode> for ModeUnion {
    fn from(_: ChildCountMode) -> Self {
        Self::ChildCount(ChildCountMode)
    }
}

impl From<ReviewChildCountMode> for ModeUnion {
    fn from(_: ReviewChildCountMode) -> Self {
        Self::ReviewChildCount(ReviewChildCountMode)
    }
}

impl From<ReviewInfoMode> for ModeUnion {
    fn from(_: ReviewInfoMode) -> Self {
        Self::ReviewInfo(ReviewInfoMode)
    }
}

impl From<BranchSelectorMode> for ModeUnion {
    fn from(_: BranchSelectorMode) -> Self {
        Self::BranchSelector(BranchSelectorMode)
    }
}

impl From<RebaseBranchSelectorMode> for ModeUnion {
    fn from(_: RebaseBranchSelectorMode) -> Self {
        Self::RebaseBranchSelector(RebaseBranchSelectorMode)
    }
}

impl From<MergeBranchSelectorMode> for ModeUnion {
    fn from(_: MergeBranchSelectorMode) -> Self {
        Self::MergeBranchSelector(MergeBranchSelectorMode)
    }
}

impl From<ModelSelectorMode> for ModeUnion {
    fn from(_: ModelSelectorMode) -> Self {
        Self::ModelSelector(ModelSelectorMode)
    }
}

impl From<CommandPaletteMode> for ModeUnion {
    fn from(_: CommandPaletteMode) -> Self {
        Self::CommandPalette(CommandPaletteMode)
    }
}

impl From<ConfirmingMode> for ModeUnion {
    fn from(state: ConfirmingMode) -> Self {
        Self::Confirming(state)
    }
}

impl From<ConfirmPushMode> for ModeUnion {
    fn from(_: ConfirmPushMode) -> Self {
        Self::ConfirmPush(ConfirmPushMode)
    }
}

impl From<ConfirmPushForPRMode> for ModeUnion {
    fn from(_: ConfirmPushForPRMode) -> Self {
        Self::ConfirmPushForPR(ConfirmPushForPRMode)
    }
}

impl From<RenameBranchMode> for ModeUnion {
    fn from(_: RenameBranchMode) -> Self {
        Self::RenameBranch(RenameBranchMode)
    }
}

impl From<KeyboardRemapPromptMode> for ModeUnion {
    fn from(_: KeyboardRemapPromptMode) -> Self {
        Self::KeyboardRemapPrompt(KeyboardRemapPromptMode)
    }
}

impl From<UpdatePromptMode> for ModeUnion {
    fn from(state: UpdatePromptMode) -> Self {
        Self::UpdatePrompt(state)
    }
}

impl From<UpdateRequestedMode> for ModeUnion {
    fn from(state: UpdateRequestedMode) -> Self {
        Self::UpdateRequested(state)
    }
}

impl From<HelpMode> for ModeUnion {
    fn from(_: HelpMode) -> Self {
        Self::Help(HelpMode)
    }
}

impl From<ErrorModalMode> for ModeUnion {
    fn from(state: ErrorModalMode) -> Self {
        Self::ErrorModal(state)
    }
}

impl From<SuccessModalMode> for ModeUnion {
    fn from(state: SuccessModalMode) -> Self {
        Self::SuccessModal(state)
    }
}

impl From<PreviewFocusedMode> for ModeUnion {
    fn from(_: PreviewFocusedMode) -> Self {
        Self::PreviewFocused(PreviewFocusedMode)
    }
}
