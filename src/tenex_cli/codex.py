"""Codex-facing helpers for readiness detection and prompt transmission."""

from __future__ import annotations

import time

from tenex_cli.config import Settings
from tenex_cli.logger import Logger
from tenex_cli.tmux import TmuxClient


def wait_for_ready(tmux: TmuxClient, pane_id: str, settings: Settings, logger: Logger) -> bool:
    """Poll a pane until Codex displays the configured readiness marker."""
    logger.info(f"Waiting for codex to be ready in pane {pane_id}...")
    for _ in range(settings.ready_attempts):
        output = tmux.capture_tail(pane_id, 5)
        if settings.ready_marker in output:
            logger.info(f"Codex ready in pane {pane_id}")
            return True
        time.sleep(settings.ready_interval_seconds)
    logger.warning(f"Timeout waiting for codex readiness in pane {pane_id}")
    return False


def pane_is_busy(tmux: TmuxClient, pane_id: str, settings: Settings) -> bool:
    """Return True when the busy marker is present in the pane output."""
    output = tmux.capture_tail(pane_id, 10)
    return settings.busy_marker in output


def send_text(tmux: TmuxClient, pane_id: str, message: str, newline_count: int = 2) -> None:
    """Send a message via tmux buffer followed by the requested ENTER presses."""
    tmux.set_buffer(message)
    tmux.paste_buffer(pane_id)
    tmux.send_enter(pane_id, times=newline_count)
