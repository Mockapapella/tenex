"""Codex helper tests using lightweight tmux stubs."""

from collections import deque
from pathlib import Path
from typing import cast

from rich.console import Console

from tenex_cli.codex import pane_is_busy
from tenex_cli.codex import send_text
from tenex_cli.codex import wait_for_ready
from tenex_cli.config import Settings
from tenex_cli.logger import Logger
from tenex_cli.tmux import TmuxClient


class StubTmux:
    """Minimal tmux stub capturing buffer operations."""

    def __init__(self, tails: deque[str]) -> None:
        """Store initial pane outputs."""
        self.tails = tails
        self.buffer: list[str] = []
        self.pasted: list[str] = []

    def capture_tail(self, pane_id: str, lines: int) -> str:
        """Return the head of the deque while simulating rotation."""
        _ = (pane_id, lines)
        value = self.tails[0]
        if len(self.tails) > 1:
            self.tails.popleft()
        return value

    def set_buffer(self, text: str) -> None:
        """Record the latest buffer value for assertions."""
        self.buffer.append(text)

    def paste_buffer(self, pane_id: str) -> None:
        """Track pasted entries to verify send_text behavior."""
        _ = pane_id
        if self.buffer:
            self.pasted.append(self.buffer[-1])

    def send_enter(self, pane_id: str, times: int = 1, delay_seconds: float = 0.1) -> None:
        """Satisfy the interface without performing real I/O."""
        _ = (pane_id, times, delay_seconds)


def build_settings() -> Settings:
    """Return a Settings object with deterministic timings for tests."""
    return Settings(
        pane_count=1,
        ready_marker="ready",
        busy_marker="busy",
        codex_command="codex",
        ready_attempts=2,
        ready_interval_seconds=0,
        continue_cooldown_seconds=1,
        continue_poll_seconds=1,
        state_root=Path(),
    )


def test_wait_for_ready_stops_after_match() -> None:
    """wait_for_ready succeeds when the marker eventually appears."""
    tmux = StubTmux(deque(["pending", "ready"]))
    settings = build_settings()
    logger = Logger(Console(record=True))

    assert wait_for_ready(cast("TmuxClient", tmux), "%1", settings, logger) is True

    tmux_fail = StubTmux(deque(["pending"]))
    settings.ready_attempts = 1
    assert wait_for_ready(cast("TmuxClient", tmux_fail), "%1", settings, logger) is False


def test_pane_busy_and_send_text() -> None:
    """pane_is_busy toggles with markers and send_text stores buffer data."""
    tmux = StubTmux(deque(["busy", "ready"]))
    settings = build_settings()

    assert pane_is_busy(cast("TmuxClient", tmux), "%1", settings) is True
    tmux.tails.append("ready")
    assert pane_is_busy(cast("TmuxClient", tmux), "%1", settings) is False

    send_text(cast("TmuxClient", tmux), "%1", "echo hi")
    assert tmux.pasted[-1] == "echo hi"
