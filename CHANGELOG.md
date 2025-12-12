# Changelog

## [1.0.1] - 2025-12-12

### Added

- **Self-update feature**: Tenex now checks crates.io for newer versions on startup. If an update is available, a modal prompts to update; accepting runs `cargo install tenex --locked --force` and restarts in-place.
- **Rebase flow** (`Ctrl+r`): New interactive rebase operation with branch selector and success modal on completion.
- **Merge flow** (`Ctrl+m`): New merge operation with branch selector. If the target branch has a worktree, merges there. On conflicts, spawns a terminal for resolution. Shows success modal on completion.
- **Kitty keyboard protocol support**: Enables Kitty keyboard enhancement when supported so `Ctrl+m` is distinguishable from Enter. If not supported, Tenex prompts once at startup to remap merge to `Ctrl+n`, persists choice in settings.json.
- **State path override**: `TENEX_STATE_PATH` environment variable can override where Tenex reads/writes its persistent state.
- **Shift+Tab in preview mode**: Now correctly forwarded to tmux (as BTab).

### Changed

- **Default agent command**: Claude agents now include `--allow-dangerously-skip-permissions` flag by default.
- **Removed agent limit**: The `max_agents` configuration limit has been removed. Tenex no longer restricts the number of agents.
- **Removed config system**: The `tenex config` subcommand and `--set-agent` flag have been removed. Tenex always uses default configuration (agent state is still persisted).
- **Error modals**: Now word-wrap long messages for better readability.
- **Success modals**: New success modal displayed after git operations complete.

### Fixed

- **Deleted agents reappearing**: Fixed bug where deleted agents would reappear after restart due to orphaned worktrees. Worktree cleanup on delete is now retried with backoff and verified.
- **Worktree cleanup on rename**: Renaming a root agent now correctly moves/renames its worktree directory and git worktree metadata, and updates descendant worktree paths.
- **Reset cleanup warnings**: `tenex reset` and startup git-exclude/log clearing now print warnings instead of silently ignoring tmux/worktree/branch cleanup errors.

## [1.0.0] - 2025-12-09

Initial release on crates.io.
