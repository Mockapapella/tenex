# Tenex

**Terminal multiplexer for AI coding agents**

[![CI](https://github.com/Mockapapella/tenex/actions/workflows/ci.yml/badge.svg)](https://github.com/Mockapapella/tenex/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

Tenex lets you run multiple AI coding agents in parallel, each in an isolated git worktree with its own branch. Spawn agent swarms for research, planning, or code review—then synthesize their findings back together.

## Features

- **Parallel agents** — Run Claude, Aider, or any CLI tool simultaneously
- **Git isolation** — Each agent works in its own worktree and branch
- **Swarm workflows** — Spawn planning or review swarms with one keystroke
- **Synthesis** — Aggregate outputs from child agents into a parent
- **Live preview** — Watch agent output in real-time with vim-style navigation
- **Git operations** — Push, rename branches, and open PRs from the TUI
- **Persistent state** — Agents survive restarts; reconnect to existing sessions

## Requirements

- **tmux** — Required for session management
- **git** — Required for worktree isolation
- **Rust 1.91+** — For building from source
- **An AI CLI** — Claude (`claude`), Aider (`aider`), or any command-line tool

## Installation

```bash
# Build from source
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
| `a` | Add agent |
| `A` | Add agent with prompt |
| `d` | Delete agent and sub-agents |
| `S` | Spawn swarm (root + children) |
| `P` | Planning swarm (with research prompt) |
| `R` | Review swarm (code review against base branch) |
| `+` | Add children to selected agent |
| `s` | Synthesize sub-agent outputs into parent |
| `B` | Broadcast message to leaf sub-agents |

### Terminals

| Key | Action |
|-----|--------|
| `t` | Spawn terminal (plain shell) |
| `T` | Spawn terminal with startup command |

### Git

| Key | Action |
|-----|--------|
| `Ctrl+p` | Push branch to remote |
| `r` | Rename branch |
| `Ctrl+o` | Open pull request |

### Navigation

| Key | Action |
|-----|--------|
| `j` / `↓` | Next agent |
| `k` / `↑` | Previous agent |
| `Enter` | Focus preview (forward keystrokes to agent) |
| `Ctrl+q` | Unfocus preview / Quit |
| `Tab` | Switch between preview and diff tabs |
| `Space` | Collapse/expand agent tree |
| `Ctrl+u` | Scroll up |
| `Ctrl+d` | Scroll down |
| `g` | Scroll to top |
| `G` | Scroll to bottom |
| `?` | Help |

## Setting Your AI Agent

```bash
tenex --set-agent claude
```

Whatever you set is executed as a shell command. Claude and Codex have been tested; other AI CLIs may work but haven't been tested.

## Workflows

### Planning Swarm

Use `P` to spawn a planning swarm. Child agents receive a research prompt and work independently. When they're done, press `s` to synthesize their findings into the parent agent.

### Code Review

Use `R` to spawn a review swarm against a base branch. Each child agent reviews the diff and provides feedback. Synthesize to aggregate the reviews.

### Broadcasting

Select an agent and press `B` to send a message to that agent and all its leaf descendants. Useful for giving the same instructions to all sub-agents in a swarm.

## License

[Apache-2.0](LICENSE)
