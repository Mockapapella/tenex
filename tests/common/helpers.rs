//! Helper functions for test setup and common operations

/// Check if tmux is available on the system
pub fn tmux_available() -> bool {
    tenex::tmux::is_available()
}

/// Skip a test if tmux is not available. Returns true if test should be skipped.
pub fn skip_if_no_tmux() -> bool {
    if !tmux_available() {
        eprintln!("Skipping test: tmux not available");
        return true;
    }
    false
}
