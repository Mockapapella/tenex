"""Configuration parsing tests."""

from pathlib import Path

import pytest

from tenex_cli.config import Settings


def test_settings_from_env(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path_factory: pytest.TempPathFactory,
) -> None:
    """Environment variables override defaults and populate paths."""
    state_dir = tmp_path_factory.mktemp("tenex-state") / "session"
    monkeypatch.setenv("TENEX_PANES", "5")
    monkeypatch.setenv("TENEX_READY_MARKER", "ready")
    monkeypatch.setenv("TENEX_BUSY_MARKER", "busy")
    monkeypatch.setenv("TENEX_CODEX_COMMAND", "custom")
    monkeypatch.setenv("TENEX_READY_ATTEMPTS", "5")
    monkeypatch.setenv("TENEX_READY_INTERVAL", "0.5")
    monkeypatch.setenv("TENEX_CONTINUE_COOLDOWN", "2")
    monkeypatch.setenv("TENEX_CONTINUE_POLL", "3")
    monkeypatch.setenv("TENEX_STATE_DIR", str(state_dir))

    settings = Settings.from_env()

    expected_ready_interval = 0.5
    expected_cooldown = 2
    expected_poll = 3
    expected_panes = 5
    expected_ready_attempts = 5

    assert settings.pane_count == expected_panes
    assert settings.ready_marker == "ready"
    assert settings.busy_marker == "busy"
    assert settings.codex_command == "custom"
    assert settings.ready_attempts == expected_ready_attempts
    assert settings.ready_interval_seconds == expected_ready_interval
    assert settings.continue_cooldown_seconds == expected_cooldown
    assert settings.continue_poll_seconds == expected_poll
    assert settings.state_root == Path(state_dir)
