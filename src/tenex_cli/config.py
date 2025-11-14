"""Configuration helpers for tenex CLI."""

from __future__ import annotations

import os
import tempfile
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Settings:
    """Container for user-tunable runtime parameters."""

    pane_count: int
    ready_marker: str
    busy_marker: str
    codex_command: str
    ready_attempts: int
    ready_interval_seconds: float
    continue_cooldown_seconds: int
    continue_poll_seconds: int
    state_root: Path

    @classmethod
    def from_env(cls) -> Settings:
        """Construct settings using environment overrides when present."""
        pane_count = int(os.environ.get("TENEX_PANES", "10"))
        ready_marker = os.environ.get("TENEX_READY_MARKER", "100% context left")
        busy_marker = os.environ.get("TENEX_BUSY_MARKER", "esc to interrupt")
        codex_command = os.environ.get("TENEX_CODEX_COMMAND", "codex")
        ready_attempts = int(os.environ.get("TENEX_READY_ATTEMPTS", "30"))
        ready_interval_seconds = float(os.environ.get("TENEX_READY_INTERVAL", "1"))
        continue_cooldown_seconds = int(os.environ.get("TENEX_CONTINUE_COOLDOWN", "10"))
        continue_poll_seconds = int(os.environ.get("TENEX_CONTINUE_POLL", "10"))
        default_state_root = Path(tempfile.gettempdir())
        state_root = Path(os.environ.get("TENEX_STATE_DIR", str(default_state_root)))
        return cls(
            pane_count=pane_count,
            ready_marker=ready_marker,
            busy_marker=busy_marker,
            codex_command=codex_command,
            ready_attempts=ready_attempts,
            ready_interval_seconds=ready_interval_seconds,
            continue_cooldown_seconds=continue_cooldown_seconds,
            continue_poll_seconds=continue_poll_seconds,
            state_root=state_root,
        )
