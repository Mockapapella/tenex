//! Compile-time state types (new architecture).

mod branch_selector;
mod broadcasting;
mod changelog;
mod child_count;
mod child_prompt;
mod command_palette;
mod confirm_push;
mod confirm_push_for_pr;
mod confirming;
mod creating;
mod custom_agent_cmd;
mod diff_focused;
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
mod settings_menu;
mod success_modal;
mod synthesis_prompt;
mod terminal_prompt;
mod update_prompt;
mod update_requested;

pub use branch_selector::BranchSelectorMode;
pub use broadcasting::BroadcastingMode;
pub use changelog::ChangelogMode;
pub use child_count::ChildCountMode;
pub use child_prompt::ChildPromptMode;
pub use command_palette::CommandPaletteMode;
pub use confirm_push::ConfirmPushMode;
pub use confirm_push_for_pr::ConfirmPushForPRMode;
pub use confirming::{ConfirmAction, ConfirmingMode};
pub use creating::CreatingMode;
pub use custom_agent_cmd::CustomAgentCommandMode;
pub use diff_focused::DiffFocusedMode;
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
pub use settings_menu::SettingsMenuMode;
pub use success_modal::SuccessModalMode;
pub use synthesis_prompt::SynthesisPromptMode;
pub use terminal_prompt::TerminalPromptMode;
pub use update_prompt::UpdatePromptMode;
pub use update_requested::UpdateRequestedMode;

/// The application's current mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Normal operation mode.
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
    /// Synthesis prompt mode.
    SynthesisPrompt(SynthesisPromptMode),
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
    /// Settings menu mode.
    SettingsMenu(SettingsMenuMode),
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
    /// Changelog / "What's New" modal mode.
    Changelog(ChangelogMode),
    /// Help overlay mode.
    Help(HelpMode),
    /// Error modal mode.
    ErrorModal(ErrorModalMode),
    /// Success modal mode.
    SuccessModal(SuccessModalMode),
    /// Preview focused mode.
    PreviewFocused(PreviewFocusedMode),
    /// Diff focused mode.
    DiffFocused(DiffFocusedMode),
}

impl AppMode {
    /// Convenience constructor for `AppMode::Normal`.
    #[must_use]
    pub const fn normal() -> Self {
        Self::Normal(NormalMode)
    }
}

impl Default for AppMode {
    fn default() -> Self {
        Self::normal()
    }
}

impl From<NormalMode> for AppMode {
    fn from(_: NormalMode) -> Self {
        Self::Normal(NormalMode)
    }
}

impl From<DiffFocusedMode> for AppMode {
    fn from(_: DiffFocusedMode) -> Self {
        Self::DiffFocused(DiffFocusedMode)
    }
}

impl From<ScrollingMode> for AppMode {
    fn from(_: ScrollingMode) -> Self {
        Self::Scrolling(ScrollingMode)
    }
}

impl From<CreatingMode> for AppMode {
    fn from(_: CreatingMode) -> Self {
        Self::Creating(CreatingMode)
    }
}

impl From<PromptingMode> for AppMode {
    fn from(_: PromptingMode) -> Self {
        Self::Prompting(PromptingMode)
    }
}

impl From<ChildPromptMode> for AppMode {
    fn from(_: ChildPromptMode) -> Self {
        Self::ChildPrompt(ChildPromptMode)
    }
}

impl From<BroadcastingMode> for AppMode {
    fn from(_: BroadcastingMode) -> Self {
        Self::Broadcasting(BroadcastingMode)
    }
}

impl From<ReconnectPromptMode> for AppMode {
    fn from(_: ReconnectPromptMode) -> Self {
        Self::ReconnectPrompt(ReconnectPromptMode)
    }
}

impl From<TerminalPromptMode> for AppMode {
    fn from(_: TerminalPromptMode) -> Self {
        Self::TerminalPrompt(TerminalPromptMode)
    }
}

impl From<CustomAgentCommandMode> for AppMode {
    fn from(_: CustomAgentCommandMode) -> Self {
        Self::CustomAgentCommand(CustomAgentCommandMode)
    }
}

impl From<SynthesisPromptMode> for AppMode {
    fn from(_: SynthesisPromptMode) -> Self {
        Self::SynthesisPrompt(SynthesisPromptMode)
    }
}

impl From<ChildCountMode> for AppMode {
    fn from(_: ChildCountMode) -> Self {
        Self::ChildCount(ChildCountMode)
    }
}

impl From<ReviewChildCountMode> for AppMode {
    fn from(_: ReviewChildCountMode) -> Self {
        Self::ReviewChildCount(ReviewChildCountMode)
    }
}

impl From<ReviewInfoMode> for AppMode {
    fn from(_: ReviewInfoMode) -> Self {
        Self::ReviewInfo(ReviewInfoMode)
    }
}

impl From<BranchSelectorMode> for AppMode {
    fn from(_: BranchSelectorMode) -> Self {
        Self::BranchSelector(BranchSelectorMode)
    }
}

impl From<RebaseBranchSelectorMode> for AppMode {
    fn from(_: RebaseBranchSelectorMode) -> Self {
        Self::RebaseBranchSelector(RebaseBranchSelectorMode)
    }
}

impl From<MergeBranchSelectorMode> for AppMode {
    fn from(_: MergeBranchSelectorMode) -> Self {
        Self::MergeBranchSelector(MergeBranchSelectorMode)
    }
}

impl From<ModelSelectorMode> for AppMode {
    fn from(_: ModelSelectorMode) -> Self {
        Self::ModelSelector(ModelSelectorMode)
    }
}

impl From<SettingsMenuMode> for AppMode {
    fn from(_: SettingsMenuMode) -> Self {
        Self::SettingsMenu(SettingsMenuMode)
    }
}

impl From<CommandPaletteMode> for AppMode {
    fn from(_: CommandPaletteMode) -> Self {
        Self::CommandPalette(CommandPaletteMode)
    }
}

impl From<ConfirmingMode> for AppMode {
    fn from(state: ConfirmingMode) -> Self {
        Self::Confirming(state)
    }
}

impl From<ConfirmPushMode> for AppMode {
    fn from(_: ConfirmPushMode) -> Self {
        Self::ConfirmPush(ConfirmPushMode)
    }
}

impl From<ConfirmPushForPRMode> for AppMode {
    fn from(_: ConfirmPushForPRMode) -> Self {
        Self::ConfirmPushForPR(ConfirmPushForPRMode)
    }
}

impl From<RenameBranchMode> for AppMode {
    fn from(_: RenameBranchMode) -> Self {
        Self::RenameBranch(RenameBranchMode)
    }
}

impl From<KeyboardRemapPromptMode> for AppMode {
    fn from(_: KeyboardRemapPromptMode) -> Self {
        Self::KeyboardRemapPrompt(KeyboardRemapPromptMode)
    }
}

impl From<UpdatePromptMode> for AppMode {
    fn from(state: UpdatePromptMode) -> Self {
        Self::UpdatePrompt(state)
    }
}

impl From<UpdateRequestedMode> for AppMode {
    fn from(state: UpdateRequestedMode) -> Self {
        Self::UpdateRequested(state)
    }
}

impl From<ChangelogMode> for AppMode {
    fn from(state: ChangelogMode) -> Self {
        Self::Changelog(state)
    }
}

impl From<HelpMode> for AppMode {
    fn from(_: HelpMode) -> Self {
        Self::Help(HelpMode)
    }
}

impl From<ErrorModalMode> for AppMode {
    fn from(state: ErrorModalMode) -> Self {
        Self::ErrorModal(state)
    }
}

impl From<SuccessModalMode> for AppMode {
    fn from(state: SuccessModalMode) -> Self {
        Self::SuccessModal(state)
    }
}

impl From<PreviewFocusedMode> for AppMode {
    fn from(_: PreviewFocusedMode) -> Self {
        Self::PreviewFocused(PreviewFocusedMode)
    }
}
