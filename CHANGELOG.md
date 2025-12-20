# Changelog

## [1.0.4] - 2025-12-20

### Changed

- **Session backend**: Removed the tmux dependency and replaced it with a built-in PTY mux (no external multiplexer required).
- **Windows installation**: Simplified installation flow; no MSYS2/tmux required (uses MSVC build tools + rustup).

### Added

- **Mux socket override**: `TENEX_MUX_SOCKET` environment variable to override the mux daemon socket name/path.

### Fixed

- **Terminal preview rendering**: Fixed cursor rendering in the terminal preview.
- **Input modals**: Long input text now wraps correctly.

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
