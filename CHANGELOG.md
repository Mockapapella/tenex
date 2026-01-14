# Changelog

## [1.0.5] - 2026-01-14

### Added

- **Interactive Diff tab**: Select a range (line/hunk/file) and revert directly from the TUI.
- **Commits tab**: View commits for the current branch.
- **Mouse support**: Mouse wheel scrolling in preview/diff, plus click selection and modal dismiss.
- **Agent activity indicators**: Shows whether each agent is producing output or waiting (and whether waiting output has been seen).
- **Per-role agent settings**: Persist agent program selection separately for default, planner, and review swarms.
- **Reset scope** (`tenex reset`): Choose to reset only the current Tenex instance or all Tenex sessions on the machine.

### Changed

- **Platform support**: Tenex is now Linux-only.
- **State directory**: Default state moved to `~/.tenex/` (migrating legacy `~/.local/share/tenex/` data when possible).
- **`TENEX_STATE_PATH` scoping**: When set, Tenex treats the state file's parent directory as the instance root (state, settings, and worktrees live alongside it).
- **UI polish**: Improved color highlights; Diff/Commits tab notifications repositioned.

### Fixed

- **Startup recovery**: After reboot/crash, Tenex can respawn missing agent mux sessions/windows from saved state.
- **Safer agent pruning**: Avoids deleting agents when mux session listing is unavailable or transiently empty.
- **Scrolling**: Fix preview scrolling/history capture; enable scrollback for alt-screen TUIs (e.g. Codex).
- **Preview performance**: Fix preview stutter while moving the mouse.
- **Claude Code broadcast**: Fix broadcast submit behavior.
- **Worktree reliability**: Copy agent instruction files into worktrees for consistent agent behavior.
- **Developer safety**: Tests no longer overwrite real user settings.

## [1.0.4] - 2025-12-20

### Changed

- **PTY mux**: Removed the tmux dependency by replacing it with the built-in PTY mux.
- **Input modal**: Long input text now wraps in the modal.

### Fixed

- **Paste handling**: Limit the Codex paste path to Codex panes to avoid affecting other panes.
- **Terminal preview cursor**: Correct cursor rendering in the terminal preview.

### Documentation

- **Windows setup**: Clarified install steps, including rustup and MSVC guidance.

## [1.0.3] - 2025-12-18

### Added

- **Windows support**: Tenex now runs on Windows with MSYS2 tmux, including native TLS for update checks. Support should be considered functional but experimental. Expect bugs.

## [1.0.2] - 2025-12-15

### Added

- **Slash command palette** (`/`): Open a command palette to run slash commands like `/agents` and `/help`.
- **Agent program selection** (`/agents`): Choose the default agent program for new agents (`claude`, `codex`, or `custom`) and persist it in settings.
- **Scrollbars**: Agent list, preview, diff, and the help overlay now show scrollbar indicators when content overflows.

### Changed

- **Planning swarm** (`P`): Now spawns planners under the selected agent (consistent with review swarms and `+`).
- **Navigation keys**: Removed `j/k` navigation; use arrow keys (`↑/↓`) for list navigation.
- **Diff view performance**: Optimized diff rendering and refresh cadence to reduce CPU usage.
- **Terminal attach UX**: Clearer attached/read-only affordances when entering/leaving terminal mode.

### Fixed

- **Codex input submission**: Improved reliability when using the `codex` CLI by using a paste-and-submit path for bracketed paste-aware apps.

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
