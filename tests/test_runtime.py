"""Runtime-level behavior tests."""

import pytest
import typer

from tenex_cli.config import Settings
from tenex_cli.logger import Logger
from tenex_cli.runtime import build_runtime
from tenex_cli.runtime import ensure_tmux


def test_ensure_tmux_requires_environment(monkeypatch: pytest.MonkeyPatch) -> None:
    """ensure_tmux exits without TMUX and returns silently otherwise."""
    logger = Logger()
    monkeypatch.delenv("TMUX", raising=False)
    with pytest.raises(typer.Exit):
        ensure_tmux(logger)

    monkeypatch.setenv("TMUX", "1")
    ensure_tmux(logger)


def test_build_runtime_constructs_dependencies(monkeypatch: pytest.MonkeyPatch) -> None:
    """build_runtime should instantiate its collaborators exactly once."""

    class FakeTmux:
        def __init__(self, logger: Logger) -> None:
            self.logger = logger

    class FakeSession:
        def __init__(self, settings: Settings, tmux: object) -> None:
            self.settings = settings
            self.tmux = tmux

    monkeypatch.setenv("TMUX", "1")
    monkeypatch.setattr("tenex_cli.runtime.TmuxClient", FakeTmux)
    monkeypatch.setattr("tenex_cli.runtime.SessionManager", FakeSession)
    runtime = build_runtime()
    assert isinstance(runtime.tmux, FakeTmux)
    assert isinstance(runtime.session, FakeSession)
