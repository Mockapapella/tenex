"""Exercise the TmuxClient wrapper logic via an injected runner."""

import subprocess
from collections import deque

import pytest
from rich.console import Console

from tenex_cli.logger import Logger
from tenex_cli.tmux import TmuxClient
from tenex_cli.tmux import TmuxError


def test_tmux_client_wraps_commands() -> None:
    """Ensure high-level helpers invoke the runner with expected arguments."""
    responses = deque(
        [
            "%0",
            "%1",
            "line-a\nline-b",
            "%0\n%1",
        ]
    )

    def fake_runner(
        args: list[str],
        *,
        capture_output: bool = False,
        **_: object,
    ) -> subprocess.CompletedProcess[str]:
        stdout = None
        if capture_output:
            stdout = responses[0]
            responses.rotate(-1)
        return subprocess.CompletedProcess(args, 0, stdout=stdout, stderr=None)

    client = TmuxClient(Logger(Console(record=True)), runner=fake_runner)

    pane_id = client.current_pane()
    assert pane_id == "%0"

    new_pane = client.split_window(pane_id, horizontal=True)
    assert isinstance(new_pane, str)

    client.select_layout("tiled", "%1")
    client.send_keys("%1", "ls")
    client.send_enter("%1")

    tail = client.capture_tail("%1", 1)
    assert "line" in tail

    assert client.pane_exists("%1") is True
    client.set_buffer("echo hi")
    client.paste_buffer("%1")


def test_tmux_client_run_raises_on_failure() -> None:
    """Runner errors should surface as TmuxError."""

    def failing_runner(
        args: list[str],
        **_kwargs: object,
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(args, 1, stdout="", stderr="boom")

    client = TmuxClient(Logger(Console(record=True)), runner=failing_runner)
    with pytest.raises(TmuxError):
        client.run("display-message", "-p", "#{pane_id}")
