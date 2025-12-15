# Tenex

**Terminal multiplexer for AI coding agents**

[![CI](https://github.com/Mockapapella/tenex/actions/workflows/ci.yml/badge.svg)](https://github.com/Mockapapella/tenex/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/tenex.svg)](https://crates.io/crates/tenex)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Tenex lets you run multiple AI coding agents in parallel, each in an isolated git worktree with its own branch. Spawn agent swarms for research, planning, or code review—then synthesize their findings back together.

## Features

- **Parallel agents** — Run multiple AI coding agents simultaneously (Claude CLI by default; also supports Codex or a custom command)
- **Git isolation** — Each root agent works in its own worktree and branch; child agents share the root's worktree
- **Swarm workflows** — Spawn planning or review swarms with one keystroke
- **Synthesis** — Aggregate outputs from descendant agents into a parent (captures last ~5000 lines from each, writes to markdown, then sends to parent)
- **Live preview** — Watch agent output in real-time with ANSI color support; auto-follows bottom unless you scroll
- **Diff view** — See uncommitted changes (staged + unstaged + untracked) vs HEAD in the selected agent's worktree
- **Git operations** — Push, rebase, merge, rename branches, and open PRs from the TUI
- **Command palette** — Run slash commands like `/agents` and `/help`
- **Persistent state** — Agents survive restarts; auto-reconnects to existing worktrees on startup
- **Auto-update** — Checks crates.io for updates on startup and prompts to install

## Requirements

- **tmux** — Required for session management (recent version recommended)
- **git** — Required for worktree isolation
- **gh** — GitHub CLI, required for opening pull requests (`Ctrl+o`)
- **An agent CLI** — `claude` (default) or `codex` (or configure a custom command)
- **Rust 1.91+** — For building from source
- **cargo** — Required for auto-update functionality

## Installation

```bash
# Or build from source
git clone https://github.com/Mockapapella/tenex
cd tenex
cargo install --path .
```

## Quick Start

```bash
# Navigate to any git repository
cd your-project

# Launch Tenex
tenex

# Press 'a' to create your first agent
# Press '?' for help
```

## Keybindings

### Agents

| Key | Action |
|-----|--------|
| `a` | Add agent (no prompt) |
| `A` | Add agent with prompt |
| `d` | Delete agent and all descendants |
| `S` | Spawn swarm (new root + N children) |
| `P` | Planning swarm (spawn N planners for selected agent) |
| `R` | Review swarm (spawn N reviewers for selected agent, then pick base branch) |
| `+` | Spawn N sub-agents for selected agent |
| `s` | Synthesize descendant outputs into parent |
| `B` | Broadcast message to leaf agents only (excludes terminals) |

### Terminals

| Key | Action |
|-----|--------|
| `t` | Spawn terminal (bash shell as child of selected root) |
| `T` | Spawn terminal with startup command |

### Git

| Key | Action |
|-----|--------|
| `Ctrl+p` | Push branch to remote |
| `r` | Rename (root: branch + session + worktree; child: title + window only) |
| `Ctrl+o` | Open pull request (via `gh pr create --web`) |
| `Ctrl+r` | Rebase onto selected branch |
| `Ctrl+m` | Merge selected branch into current |
| `Ctrl+n` | Merge (fallback for terminals that can't distinguish Ctrl+m from Enter) |

### Navigation

| Key | Action |
|-----|--------|
| `↓` | Next agent |
| `↑` | Previous agent |
| `Enter` | Attach terminal (forward keystrokes to agent) |
| `Ctrl+q` | Detach terminal / Quit (with confirm if agents running) |
| `Esc` | Cancel current modal or flow |
| `Tab` | Switch between Preview and Diff tabs |
| `Space` | Collapse/expand agent tree |
| `Ctrl+u` | Scroll preview/diff up |
| `Ctrl+d` | Scroll preview/diff down |
| `g` | Scroll to top |
| `G` | Scroll to bottom |
| `?` | Help |
| `/` | Command palette (`/agents`, `/help`) |

## Configuration

The default agent command is `claude --allow-dangerously-skip-permissions`. Press `/` to open the command palette, run `/agents`, and choose the default program for new agents (`claude`, `codex`, or `custom` — which will prompt for a command and save it to `settings.json`).

### Data Storage

| File | Location | Description |
|------|----------|-------------|
| State | `~/.local/share/tenex/state.json` | Agent list and hierarchy |
| Settings | `~/.local/share/tenex/settings.json` | Tenex settings |
| Logs | `/tmp/tenex.log` | Debug logs (when enabled) |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `DEBUG` | Log level: `0` off, `1` warn, `2` info, `3` debug |
| `TENEX_STATE_PATH` | Override state file location |

### CLI Commands

```bash
tenex              # Launch the TUI
tenex reset        # Clear all agents and state
tenex reset --force # Force reset without confirmation
tenex --help       # Show help
tenex --version    # Show version
```

## Workflows

### Spawn Swarm

Press `S` to create a new root agent with N child agents. You'll be prompted for:
1. Number of children
2. Task prompt (sent to all children)

### Planning Swarm

Press `P` for a planning-focused swarm. Children receive a planning preamble prompt and are titled "Planner N". Use `s` to synthesize their findings when done.

### Review Swarm

Press `R` to spawn code reviewers:
1. Pick number of reviewers
2. Select base branch (searchable list with ↑/↓ navigation)

Reviewers get a strict review preamble with the chosen base branch. They're titled "Reviewer N".

### Synthesis

Press `s` to synthesize. This:
1. Captures the last ~5000 lines from each descendant's tmux pane
2. Writes combined output to `.tenex/<uuid>.md` in the parent's worktree
3. Kills and removes all descendants
4. Sends the parent a command to read the synthesized file

### Broadcasting

Press `B` to send a message to all leaf agents (agents with no children). Terminals are excluded. Useful for giving the same instructions to all workers in a swarm.

### Merge Conflicts

When rebase or merge encounters conflicts, Tenex opens a terminal window titled "Merge Conflict" or "Rebase Conflict" in the worktree, runs `git status`, and leaves resolution to you.

## Keyboard Compatibility

On first launch, Tenex checks if your terminal supports the Kitty keyboard protocol (to distinguish `Ctrl+m` from Enter). If not supported, you'll be prompted to remap the merge key to `Ctrl+n`. This choice is saved to `settings.json`.

## License

[Apache-2.0](LICENSE)
