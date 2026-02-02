//! Confirming mode state type (new architecture).

/// Actions that require confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Kill an agent.
    Kill,
    /// Send Ctrl+C to the selected agent (may terminate it).
    InterruptAgent,
    /// Reset all state.
    Reset,
    /// Restart the mux daemon (kills all agent sessions).
    RestartMuxDaemon,
    /// Quit the application.
    Quit,
    /// Synthesize children into parent.
    Synthesize,
    /// Worktree already exists - ask to reconnect or recreate.
    WorktreeConflict,
}

/// Confirming mode - yes/no (or special) confirmations for various actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmingMode {
    /// The action being confirmed.
    pub action: ConfirmAction,
}
