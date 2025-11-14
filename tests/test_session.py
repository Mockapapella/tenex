"""Session manager tests that touch the filesystem."""

from pathlib import Path
from typing import cast

import pytest

from tenex_cli.config import Settings
from tenex_cli.session import SessionManager
from tenex_cli.tmux import TmuxClient


class DummyTmux:
    """Simple tmux stub returning a fixed descriptor."""

    def __init__(self, descriptor: str) -> None:
        """Store the descriptor that will always be returned."""
        self.descriptor = descriptor

    def display_message(self, fmt: str) -> str:
        """Return the stored descriptor regardless of the query."""
        _ = fmt
        return self.descriptor


def build_settings(state_root: Path) -> Settings:
    """Construct Settings for tests."""
    return Settings(
        pane_count=2,
        ready_marker="ready",
        busy_marker="busy",
        codex_command="codex",
        ready_attempts=1,
        ready_interval_seconds=0.1,
        continue_cooldown_seconds=1,
        continue_poll_seconds=1,
        state_root=state_root,
    )


def test_session_manager_persists_state(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Round-trip pane data, prompt, and cleanup through SessionManager."""
    monkeypatch.setenv("TMUX", "1")
    settings = build_settings(tmp_path)
    session = SessionManager(settings, cast("TmuxClient", DummyTmux("sess-1")))

    directory = session.ensure_directory()
    assert directory.exists()

    session.store_panes(["%0", "%1"])
    assert session.load_panes() == ["%0", "%1"]

    session.store_original_pane("%0")
    assert session.load_original_pane() == "%0"

    session.store_prompt("prompt text")
    assert session.prompt_path().read_text() == "prompt text"

    session.cleanup()
    assert not directory.exists()
