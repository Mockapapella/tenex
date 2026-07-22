# Tenex

**Terminal multiplexer for AI coding agents**

[![CI](https://github.com/Mockapapella/tenex/actions/workflows/ci.yml/badge.svg)](https://github.com/Mockapapella/tenex/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/tenex.svg)](https://crates.io/crates/tenex)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Tenex runs AI coding agents in one terminal interface. In a Git repository, each root agent gets a Tenex-managed worktree and branch. Child agents and terminals share the root agent's worktree. In a regular directory, each root tree shares that directory and Tenex disables Git-only actions.

## Features

- **Parallel agents** - Run Claude, Codex, or a custom agent command at the same time.
- **Project groups** - Manage agents from multiple repositories and directories in one sidebar.
- **Git worktrees** - Give each root agent in a Git project its own branch and worktree.
- **Optional Docker runtime** - Run new root agents in a shipped worker image. Their children and terminals inherit the same container.
- **Swarm workflows** - Create general, planning, and review agent groups from the TUI.
- **Selective synthesis** - Collect all descendant results or mark only the subtrees that you want to collect.
- **Live preview** - Watch terminal output with ANSI colors and scrollback.
- **Interactive diff** - View uncommitted changes and revert selected lines or hunks.
- **Commit view** - View commits on the selected agent branch relative to its base branch.
- **Git actions** - Push, rebase, merge, rename branches, switch branches, and open pull requests.
- **Persistent state** - Save agent metadata and try to reconnect or resume agents after a restart.
- **Published updates** - Check crates.io at startup and offer to install a newer release.

## Requirements

- Linux or macOS. Windows users can run Tenex in WSL2.
- A terminal that supports a full-screen TUI.
- Claude Code or Codex, unless you configure a custom agent command.
- Git for worktree isolation and all Git actions.
- GitHub CLI `gh` for opening pull requests.
- Cargo for installation and in-app updates.
- Rust 1.97.1 or newer when you build from source.
- Docker with a running daemon if you enable the Docker runtime.

Tenex includes its own terminal multiplexer. It does not require tmux or another external multiplexer.

## Installation

Install the latest published release from crates.io.

```bash
curl https://sh.rustup.rs -sSf | sh
cargo install tenex --locked
```

The published release can be older than the current repository. The rest of this README describes the current repository version.

Install the current repository version directly from GitHub.

```bash
cargo install --git https://github.com/Mockapapella/tenex --locked
```

To work from a local checkout, install that checkout.

```bash
git clone https://github.com/Mockapapella/tenex
cd tenex
cargo install --path . --locked
```

## Quick start

Start Tenex from a project directory.

```bash
cd your-project
tenex
```

Press `a` to name and create a root agent. Press `A` to create a root agent with an initial prompt and an automatic name. Press `?` from the detached sidebar view to open the built-in key reference.

If the project is a Git repository, Tenex creates the root in `~/.tenex/worktrees/<project>/` by default on an `agent/<name>` branch. If the project is not a Git repository, Tenex starts the root in the current directory.

## Keybindings

### Agents

| Key | Action |
|-----|--------|
| `a` | Create a named agent without an initial prompt |
| `A` | Create an automatically named agent with an initial prompt |
| `d` | Delete the selected agent and its descendants |
| `S` | Create a new root agent and a group of child agents |
| `P` | Create planning agents under the selected agent |
| `R` | Create review agents under the selected agent |
| `+` | Create child agents under the selected agent |
| `s` | Collect descendant output into the selected parent agent |
| `m` | Mark or unmark the selected descendant subtree for collection |
| `B` | Send one message to each non-terminal leaf in the selected subtree |

### Terminals

| Key | Action |
|-----|--------|
| `t` | Create a shell in the selected agent tree's root workspace |
| `T` | Create the same shell and run an initial command |

### Git

These actions require an agent in a Git worktree.

| Key | Action |
|-----|--------|
| `Ctrl+p` | Push the selected agent branch |
| `r` | Rename the selected agent. A Git root also renames its local branch, worktree, and session |
| `Ctrl+o` | Push when needed, then open `gh pr create --web` |
| `Ctrl+r` | Rebase the selected agent branch onto a branch that you choose |
| `Ctrl+m` | Merge the selected agent branch into a branch that you choose |
| `Ctrl+n` | Run merge when the terminal cannot distinguish `Ctrl+m` from Enter |
| `Ctrl+s` | Switch the selected agent tree to a branch that you choose and restart its sessions |

Renaming a child agent, terminal, or root agent in a regular directory changes its title. Tenex also renames the mux window when the item has one. A Git root rename does not delete the old remote branch.

### Navigation

| Key | Action |
|-----|--------|
| `Down` | Select the next visible sidebar item |
| `Up` | Select the previous visible sidebar item |
| `Left` | Select the current project header |
| `Right` | Select the first agent in the selected project |
| `Space` | Collapse or expand the selected project or agent tree |
| `Tab` | Cycle Preview, Diff, and Commits while the content pane is detached |
| `Enter` | Attach Preview or enter interactive Diff. Commits has no interactive mode |
| `Ctrl+q` | Leave content focus. Quit from the detached sidebar view |
| `Ctrl+u` | Scroll the detached content view up by half a page |
| `Ctrl+d` | Scroll the detached content view down by half a page |
| `g` | Move the detached content view to the top |
| `G` | Move the detached content view to the bottom |
| `?` | Open the key reference |
| `/` | Open the command palette |
| `Esc` | Cancel the current modal or selection flow |

When Preview is attached, Tenex forwards normal keys, including `Tab`, to the selected process. Tenex asks for confirmation before it sends `Ctrl+c` to a non-terminal agent.

### Interactive diff

Open the Diff tab and press `Enter` before you use these keys.

| Key | Action |
|-----|--------|
| `Up` | Move the diff cursor up |
| `Down` | Move the diff cursor down |
| `Shift+v` | Start or clear a block selection |
| `x` | Revert the selected changed line, hunk, or block |
| `Ctrl+z` | Undo the last diff edit |
| `Ctrl+y` | Redo the last undone diff edit |
| `Space` | Collapse or expand the hunk under the cursor |
| `Ctrl+q` | Leave interactive Diff |

## Configuration

Tenex uses `claude --allow-dangerously-skip-permissions` for new, planning, and review agents by default. Open `/agents` to set the program for each role to Claude, Codex, or a custom command.

The command palette contains these commands.

| Command | Action |
|---------|--------|
| `/agents` | Configure the default, planning, and review agent programs |
| `/toggle_docker` | Enable or disable Docker for new root agents |
| `/changelog` | Show the changelog for the running version |
| `/help` | Open the key reference |

### Docker

Run `/toggle_docker` to enable Docker for root agents that you create after the change. Docker must be installed and its daemon must be running. Each configured role must invoke `claude` or `codex` because the shipped worker image does not support other executables.

Tenex builds the worker image the first time that it needs it. A Docker root agent owns one container. Child agents and terminals in that tree use the same container and worktree. Existing roots keep their current runtime when you toggle the setting.

### Data storage

| Data | Default location |
|------|------------------|
| State | `~/.tenex/state.json` |
| Settings | `~/.tenex/settings.json` |
| Worktrees | `~/.tenex/worktrees/` |
| Docker runtime data | `~/.tenex/docker-runtime/` |
| Debug log | The OS temporary directory, such as `/tmp/tenex.log` on Linux |

On startup, Tenex migrates missing `state.json`, `settings.json`, and backup files from `${XDG_DATA_HOME:-~/.local/share}/tenex/` to `~/.tenex/`. It does not run this migration when `TENEX_STATE_PATH` is set, and it does not replace files that already exist at the destination.

### Environment variables

| Variable | Action |
|----------|--------|
| `DEBUG` | Set file logging to `0` for off, `1` for warnings, `2` for information, or `3` for debug output |
| `TENEX_DISABLE_MOUSE` | Set a truthy value to disable Tenex mouse capture and use terminal-native selection |
| `TENEX_MUX_SOCKET` | Override the mux daemon socket name or path for this process |
| `TENEX_STATE_PATH` | Override the state file. Tenex puts settings, worktrees, and its socket fallback beside that file |

A relative `TENEX_STATE_PATH` starts from the current working directory.

### CLI commands

```bash
tenex                # Start the TUI
tenex reset          # Show and confirm a reset plan
tenex reset --force  # Reset the current instance without prompts
tenex --help         # Show CLI help
tenex --version      # Show the installed version
```

An interactive reset always removes the stored agents in the current Tenex instance. It asks whether orphaned mux cleanup must cover only that instance or all Tenex instances on the machine, shows the cleanup plan, and asks for confirmation. Cleanup stops mux sessions and Docker containers. It also removes Tenex worktrees and local branches when it can open the current Git repository. `--force` selects only the current instance and skips both prompts.

## Workflows

### General swarm

Press `S`, choose the child count, and enter a task. Tenex creates a new root agent and sends the task to each child. In a Git project, the whole tree uses the new root worktree. In a regular directory, the whole tree uses the selected project directory.

### Planning swarm

Select a non-terminal agent and press `P`. Choose the child count and enter the task. Tenex creates planning agents under the selected agent and adds its planning instructions to the task.

### Review swarm

Select a non-terminal agent in a Git project and press `R`. Choose the reviewer count, then choose the base branch from the searchable branch list. Tenex titles the children `Reviewer N` and starts each review against that base.

Review commands other than Codex receive the Tenex review prompt. Any review command that invokes Codex uses the native `/review` flow and the selected base branch.

### Synthesis

Select a parent agent and press `s`. Tenex shows the agents that it will collect and asks for confirmation. It then lets you add optional instructions for the parent.

Tenex captures up to 5000 lines from each selected non-terminal descendant, writes the combined result to `.tenex/<uuid>.md` in the parent's workspace, terminates the collected subtrees, and tells the parent to read the file. Terminal descendants are not included in the file, but Tenex removes terminal descendants that belong to a collected subtree.

Press `m` on a visible non-terminal descendant to mark its whole subtree. If the selected parent has marks below it, synthesis uses only those marked subtrees. If it has no marks below it, synthesis uses all non-terminal descendants.

### Broadcast

Select any agent and press `B`. Enter a message to send it to each agent in the selected subtree that has no children. Tenex excludes terminal windows. If the selected agent has no children and is not a terminal, it receives the message.

### Merge and rebase conflicts

If a rebase finds conflicts, Tenex opens a `Rebase Conflict` terminal in the selected agent worktree and runs `git status`.

If a merge finds conflicts in a worktree that belongs to a Tenex root agent, Tenex opens a conflict terminal in that root and runs `git status`. If the target worktree does not belong to a Tenex root, Tenex reports the path for manual resolution. A conflict from a merge in the main repository opens a terminal in the selected agent tree. Tenex leaves conflict resolution to you.

## Keyboard compatibility

On first launch, Tenex checks whether the terminal supports the Kitty keyboard protocol. Tenex uses this support to distinguish `Ctrl+m` from Enter. If the terminal does not support it, Tenex offers to show `Ctrl+n` as the merge key and saves the choice in `settings.json`.

## Copy text

Tenex captures the mouse so it can scroll individual panes and select text in Preview. Click and drag across Preview text, then release to copy it with OSC 52. If the terminal blocks OSC 52, disable Tenex mouse handling and use the terminal's native selection.

```bash
TENEX_DISABLE_MOUSE=1 tenex
```

## Agent startup problems

If a new agent appears and then disappears, its process probably exited during startup. Enable debug logging, reproduce the problem, and inspect the log in the OS temporary directory.

```bash
DEBUG=3 tenex
```

Tenex stores the chosen mux socket in the state file while agents exist so those sessions can survive a rebuild or upgrade. If Tenex detects an older mux daemon, it asks to restart all running agent sessions. You can also start a separate instance with an explicit socket.

```bash
TENEX_MUX_SOCKET=/tmp/tenex-mux.sock tenex
```

## License

[Apache-2.0](LICENSE)
