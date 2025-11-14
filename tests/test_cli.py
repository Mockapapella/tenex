"""CLI command tests ensuring Typer wiring works."""

from __future__ import annotations

import sys
from types import SimpleNamespace
from typing import Any

import pytest
from typer.testing import CliRunner

from tenex_cli import cli


def test_cli_commands(monkeypatch: pytest.MonkeyPatch) -> None:
    """Smoke test each Typer subcommand."""
    runner = CliRunner()
    runtime = SimpleNamespace(logger=object())

    monkeypatch.setattr(cli, "build_runtime", lambda: runtime)

    recorded: dict[str, Any] = {}

    class Recorder:
        def __init__(self, name: str) -> None:
            self.name = name

        def __call__(self, *args: object) -> None:
            recorded[self.name] = args

    monkeypatch.setattr(cli, "start_review", Recorder("review"))
    monkeypatch.setattr(cli, "collect_reviews", Recorder("collect_reviews"))
    monkeypatch.setattr(cli, "start_plan", Recorder("plan"))
    monkeypatch.setattr(cli, "collect_plans", Recorder("collect_plans"))
    monkeypatch.setattr(cli, "step_prompt", Recorder("step"))
    monkeypatch.setattr(cli, "continuous_prompt", Recorder("continue"))
    monkeypatch.setattr(cli, "reset", Recorder("reset"))
    monkeypatch.setattr(
        cli,
        "gather_text",
        lambda value, *_args, **_kwargs: value or "editor",
    )

    assert runner.invoke(cli.app, ["review", "main"]).exit_code == 0
    assert recorded["review"] == (runtime, "main")

    assert runner.invoke(cli.app, ["collect-reviews"]).exit_code == 0
    assert recorded["collect_reviews"] == (runtime,)

    assert runner.invoke(cli.app, ["plan", "task"]).exit_code == 0
    assert recorded["plan"] == (runtime, "task")

    assert runner.invoke(cli.app, ["collect-plans"]).exit_code == 0
    assert recorded["collect_plans"] == (runtime,)

    assert runner.invoke(cli.app, ["step", "prompt"]).exit_code == 0
    assert recorded["step"] == (runtime, "prompt")

    assert runner.invoke(cli.app, ["continue", "prompt"]).exit_code == 0
    assert recorded["continue"] == (runtime, "prompt")

    cli.reset_command()
    assert recorded["reset"] == (runtime,)


def test_main_shows_help(monkeypatch: pytest.MonkeyPatch) -> None:
    """The top-level entrypoint should exit cleanly when showing help."""
    monkeypatch.setattr(sys, "argv", ["tenex", "--help"])
    with pytest.raises(SystemExit) as exc:
        cli.main()
    assert exc.value.code == 0
