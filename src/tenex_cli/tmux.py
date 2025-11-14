"""Thin subprocess-based tmux client used throughout tenex."""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from time import sleep
from typing import cast

from tenex_cli.logger import Logger


class TmuxError(RuntimeError):
    """Raised when a tmux subprocess exits with a failure."""


Runner = Callable[..., subprocess.CompletedProcess[str]]
DEFAULT_RUNNER = cast("Runner", subprocess.run)


@dataclass
class TmuxClient:
    """Wrap tmux invocation with friendly helpers for reuse."""

    logger: Logger
    buffer_name: str = "tenex-buffer"
    runner: Runner = DEFAULT_RUNNER

    def run(
        self,
        *args: str,
        capture_output: bool = False,
        input_data: str | None = None,
    ) -> subprocess.CompletedProcess[str]:
        """Execute a tmux command and raise on non-zero exit codes."""
        process: subprocess.CompletedProcess[str] = self.runner(
            ["tmux", *args],
            check=False,
            capture_output=capture_output,
            text=True,
            input=input_data,
        )
        if process.returncode != 0:
            stderr = process.stderr.strip() if process.stderr else "unknown error"
            raise TmuxError(f"tmux {' '.join(args)} failed: {stderr}")
        return process

    def display_message(self, fmt: str) -> str:
        """Return a tmux format expansion."""
        return self.run("display-message", "-p", fmt, capture_output=True).stdout.strip()

    def current_pane(self) -> str:
        """Return the active pane identifier."""
        return self.display_message("#{pane_id}")

    def current_window(self) -> str:
        """Return the active window identifier."""
        return self.display_message("#{window_id}")

    def split_window(self, target: str, *, horizontal: bool) -> str:
        """Split the target pane and return the newly created pane id."""
        flag = "-h" if horizontal else "-v"
        self.run("split-window", flag, "-t", target)
        return self.current_pane()

    def select_layout(self, layout: str, target: str) -> None:
        """Apply a tmux layout to the window containing the target pane."""
        self.run("select-layout", "-t", target, layout)

    def send_keys(self, pane_id: str, *keys: str) -> None:
        """Send raw key sequences to a pane."""
        if not keys:
            return
        self.run("send-keys", "-t", pane_id, *keys)

    def send_enter(self, pane_id: str, times: int = 1, delay_seconds: float = 0.1) -> None:
        """Send carriage return multiple times with a configurable delay."""
        for _ in range(times):
            self.send_keys(pane_id, "C-m")
            sleep(delay_seconds)

    def capture_pane(self, pane_id: str, *, entire: bool = False) -> str:
        """Capture pane contents and return them as text."""
        args = ["capture-pane", "-t", pane_id, "-p"]
        if entire:
            args.extend(["-S", "-"])
        return self.run(*args, capture_output=True).stdout

    def capture_tail(self, pane_id: str, lines: int) -> str:
        """Return the last `lines` lines of a pane."""
        text = self.capture_pane(pane_id)
        if lines <= 0:
            return text
        return "\n".join(text.splitlines()[-lines:])

    def pane_exists(self, pane_id: str) -> bool:
        """Check whether the pane is still attached anywhere."""
        listed = self.run(
            "list-panes",
            "-a",
            "-F",
            "#{pane_id}",
            capture_output=True,
        ).stdout.splitlines()
        return pane_id in listed

    def kill_pane(self, pane_id: str) -> None:
        """Terminate a pane if it still exists."""
        if self.pane_exists(pane_id):
            self.run("kill-pane", "-t", pane_id)

    def select_pane(self, pane_id: str) -> None:
        """Focus the provided pane."""
        self.run("select-pane", "-t", pane_id)

    def set_buffer(self, text: str) -> None:
        """Populate tmux's buffer with text for later paste."""
        self.run("set-buffer", "-b", self.buffer_name, "--", text)

    def paste_buffer(self, pane_id: str) -> None:
        """Paste the stored buffer into the pane."""
        self.run("paste-buffer", "-b", self.buffer_name, "-t", pane_id)
