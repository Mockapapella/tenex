//! Compile-time state types (new architecture).

mod branch_selector;
mod broadcasting;
mod child_count;
mod child_prompt;
mod command_palette;
mod creating;
mod custom_agent_cmd;
mod merge_branch_selector;
mod model_selector;
mod normal;
mod prompting;
mod rebase_branch_selector;
mod reconnect_prompt;
mod review_child_count;
mod review_info;
mod scrolling;
mod terminal_prompt;

pub use branch_selector::BranchSelectorMode;
pub use broadcasting::BroadcastingMode;
pub use child_count::ChildCountMode;
pub use child_prompt::ChildPromptMode;
pub use command_palette::CommandPaletteMode;
pub use creating::CreatingMode;
pub use custom_agent_cmd::CustomAgentCommandMode;
pub use merge_branch_selector::MergeBranchSelectorMode;
pub use model_selector::ModelSelectorMode;
pub use normal::NormalMode;
pub use prompting::PromptingMode;
pub use rebase_branch_selector::RebaseBranchSelectorMode;
pub use reconnect_prompt::ReconnectPromptMode;
pub use review_child_count::ReviewChildCountMode;
pub use review_info::ReviewInfoMode;
pub use scrolling::ScrollingMode;
pub use terminal_prompt::TerminalPromptMode;

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
