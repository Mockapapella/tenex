"""Tests for the editor helpers without relying on monkeypatch."""

from io import StringIO
from typing import cast

import pytest
from rich.console import Console

from tenex_cli.editor import EditorFunc
from tenex_cli.editor import gather_text
from tenex_cli.editor import open_in_editor
from tenex_cli.editor import resolve_editor
from tenex_cli.logger import Logger


class FakeInput(StringIO):
    """StringIO that always reports non-tty behavior."""

    def isatty(self) -> bool:
        """Mimic a non-interactive stream."""
        return False


def test_gather_text_prefers_direct_value() -> None:
    """Direct CLI text bypasses stdin/editor flows."""
    logger = Logger(Console(record=True))
    assert gather_text("direct", "plan", logger) == "direct"


def test_gather_text_reads_from_stdin() -> None:
    """When stdin has data, it is preferred over launching an editor."""
    logger = Logger(Console(record=True))
    fake_input = FakeInput("stdin value")
    assert gather_text(None, "plan", logger, stdin=fake_input) == "stdin value"


def test_gather_text_uses_editor_callable() -> None:
    """Fallback to an injected editor function when stdin is empty."""
    logger = Logger(Console(record=True))
    fake_input = FakeInput("")

    def fake_editor(prompt_type: str) -> str:
        return f"{prompt_type} text"

    result = gather_text(None, "plan", logger, stdin=fake_input, editor_func=fake_editor)
    assert result == "plan text"


def test_gather_text_rejects_non_callable_editor() -> None:
    """Non-callable editors should trigger a TypeError."""
    logger = Logger(Console(record=True))
    fake_input = FakeInput("")
    with pytest.raises(TypeError):
        gather_text(
            None,
            "plan",
            logger,
            stdin=fake_input,
            editor_func=cast("EditorFunc", object()),
        )


def test_gather_text_errors_when_editor_returns_empty() -> None:
    """An empty editor result should raise SystemExit."""
    logger = Logger(Console(record=True))
    fake_input = FakeInput("")

    def empty_editor(_prompt: str) -> str:
        return "   "

    with pytest.raises(SystemExit):
        gather_text(
            None,
            "plan",
            logger,
            stdin=fake_input,
            editor_func=cast("EditorFunc", empty_editor),
        )


def test_open_in_editor_uses_typer(monkeypatch: pytest.MonkeyPatch) -> None:
    """open_in_editor should rely on typer.edit."""
    monkeypatch.setattr("tenex_cli.editor.resolve_editor", lambda: "vim")
    monkeypatch.setattr("tenex_cli.editor.typer.edit", lambda **_kwargs: "# note\nvalue")
    result = open_in_editor("plan")
    assert result == "value"


def test_open_in_editor_handles_empty(monkeypatch: pytest.MonkeyPatch) -> None:
    """None returned from typer.edit should become an empty string."""
    monkeypatch.setattr("tenex_cli.editor.resolve_editor", lambda: "vim")
    monkeypatch.setattr("tenex_cli.editor.typer.edit", lambda **_kwargs: None)
    assert open_in_editor("plan") == ""


def test_resolve_editor_prefers_environment(monkeypatch: pytest.MonkeyPatch) -> None:
    """resolve_editor should respect EDITOR when executable exists."""
    monkeypatch.setenv("EDITOR", "nano -w")
    monkeypatch.setattr(
        "tenex_cli.editor.shutil.which",
        lambda cmd: "/usr/bin/" + cmd if cmd == "nano" else None,
    )
    assert resolve_editor() == "nano -w"


def test_resolve_editor_raises_without_candidates(monkeypatch: pytest.MonkeyPatch) -> None:
    """If no editors are available raise a RuntimeError."""
    monkeypatch.delenv("EDITOR", raising=False)
    monkeypatch.setattr("tenex_cli.editor.shutil.which", lambda _cmd: None)
    with pytest.raises(RuntimeError):
        resolve_editor()
