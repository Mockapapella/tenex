"""Workflow helper tests built around lightweight runtime stubs."""

import shlex
from pathlib import Path
from typing import cast

import pytest
from rich.console import Console

from tenex_cli.config import Settings
from tenex_cli.logger import Logger
from tenex_cli.runtime import Runtime
from tenex_cli.workflows import collect_from_panes
from tenex_cli.workflows import collect_plans
from tenex_cli.workflows import collect_reviews
from tenex_cli.workflows import continuous_prompt
from tenex_cli.workflows import launch_codex_with_prompt
from tenex_cli.workflows import reset
from tenex_cli.workflows import start_plan
from tenex_cli.workflows import start_review
from tenex_cli.workflows import step_prompt
from tenex_cli.workflows import write_header


class DummyTmux:
    """Capture tmux invocations without touching the real binary."""

    def __init__(self) -> None:
        """Initialise in-memory tracking structures."""
        self.captured: dict[str, str] = {"%1": "content"}
        self.killed: list[str] = []
        self.selected: list[str] = []
        self.sent_commands: list[str] = []
        self.current = "%0"
        self.window_id = "@0"
        self.next_pane = 2
        self.buffers: list[str] = []

    def pane_exists(self, pane_id: str) -> bool:
        """Return True when the requested pane is known."""
        return pane_id in self.captured

    def capture_pane(self, pane_id: str, *, entire: bool = False) -> str:
        """Return stored content for the pane."""
        _ = entire
        return self.captured[pane_id]

    def capture_tail(self, pane_id: str, lines: int) -> str:
        """Return readiness markers for wait loops."""
        _ = (pane_id, lines)
        return "100% context left"

    def kill_pane(self, pane_id: str) -> None:
        """Record pane termination."""
        self.killed.append(pane_id)
        self.captured.pop(pane_id, None)

    def select_pane(self, pane_id: str) -> None:
        """Track selected panes."""
        self.selected.append(pane_id)

    def send_keys(self, pane_id: str, command: str) -> None:
        """Record commands that would be sent to tmux."""
        _ = pane_id
        self.sent_commands.append(command)

    def send_enter(self, pane_id: str, times: int = 1, delay_seconds: float = 0.1) -> None:
        """Simulate pressing ENTER with optional repeats."""
        _ = (pane_id, delay_seconds)
        for _ in range(times):
            self.sent_commands.append("<enter>")

    def current_pane(self) -> str:
        """Return the anchor pane id."""
        return self.current

    def current_window(self) -> str:
        """Return the tmux window id."""
        return self.window_id

    def split_window(self, target: str, *, horizontal: bool) -> str:
        """Simulate creating a pane."""
        _ = (target, horizontal)
        pane_id = f"%{self.next_pane}"
        self.next_pane += 1
        self.captured[pane_id] = ""
        return pane_id

    def select_layout(self, layout: str, target: str) -> None:
        """Record layout operations."""
        self.sent_commands.append(f"layout:{layout}:{target}")

    def set_buffer(self, text: str) -> None:
        """Record buffer contents."""
        self.buffers.append(text)

    def paste_buffer(self, pane_id: str) -> None:
        """Record buffer pastes."""
        self.sent_commands.append(f"paste:{pane_id}")


class DummySession:
    """Persist pane metadata under a temporary directory."""

    def __init__(self, base: Path) -> None:
        """Store base directory for file writes."""
        self.base = base
        self._panes: list[str] = ["%0", "%1"]
        self.cleaned = False
        self.last_prompt: str | None = None
        self.original: str | None = None

    def set_panes(self, panes: list[str]) -> None:
        """Replace the stored pane ids."""
        self._panes = panes

    def load_panes(self) -> list[str]:
        """Return saved pane identifiers."""
        return self._panes

    def load_original_pane(self) -> str:
        """Return the first pane id."""
        return self._panes[0]

    def aggregated_reviews_path(self) -> Path:
        """Return the fake aggregated review path."""
        return self.base / "agg.txt"

    def aggregated_plans_path(self) -> Path:
        """Return the fake aggregated plan path."""
        return self.base / "plan.txt"

    def session_exists(self) -> bool:
        """Return True to simulate an active session."""
        return True

    def cleanup(self) -> None:
        """Mark the session as cleaned."""
        self.cleaned = True

    def ensure_directory(self) -> None:
        """Create the backing directory if missing."""
        self.base.mkdir(parents=True, exist_ok=True)

    def store_panes(self, panes: list[str]) -> None:
        """Persist pane ids in memory."""
        self._panes = list(panes)

    def store_original_pane(self, pane_id: str) -> None:
        """Remember the original pane id."""
        self.original = pane_id

    def store_prompt(self, text: str) -> None:
        """Persist the planning prompt text."""
        self.last_prompt = text
        self.ensure_directory()
        self.prompt_path().write_text(text, encoding="utf-8")

    def prompt_path(self) -> Path:
        """Return the stored prompt path."""
        return self.base / "prompt.txt"


class DummyRuntime:
    """Bundle fake settings, logger, tmux, and session objects."""

    def __init__(self, base: Path) -> None:
        """Initialise runtime-like state pointing at `base`."""
        self.settings = Settings(
            pane_count=1,
            ready_marker="ready",
            busy_marker="busy",
            codex_command="codex",
            ready_attempts=1,
            ready_interval_seconds=0.1,
            continue_cooldown_seconds=1,
            continue_poll_seconds=1,
            state_root=base,
        )
        self.logger = Logger(Console(record=True))
        self.tmux = DummyTmux()
        self.session = DummySession(base)


def test_collect_from_panes_and_header(tmp_path: Path) -> None:
    """collect_from_panes should gather content and kill Codex panes."""
    runtime = DummyRuntime(tmp_path)
    target = tmp_path / "out.md"
    write_header(target, "Title")
    collect_from_panes(cast("Runtime", runtime), ["%0", "%1"], target, "Section")

    data = target.read_text()
    assert "Title" in data
    assert "Section from Pane" in data
    assert runtime.tmux.killed == ["%1"]


def test_launch_and_reset(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """launch_codex_with_prompt should send commands and reset cleans up."""
    runtime = DummyRuntime(tmp_path)
    runtime.session.set_panes(["%0", "%1", "%2"])
    runtime.tmux.captured["%2"] = "text"
    monkeypatch.setenv("TMUX", "1")
    launch_codex_with_prompt(cast("Runtime", runtime), "prompt text")
    assert "prompt text" in runtime.tmux.sent_commands[0]
    runtime.tmux.captured["%1"] = "data"
    reset(cast("Runtime", runtime))
    assert runtime.tmux.killed
    assert runtime.session.cleaned is True


def test_start_plan_records_prompt(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """start_plan should store prompts and pane metadata."""
    runtime = DummyRuntime(tmp_path)
    monkeypatch.setenv("TMUX", "1")
    monkeypatch.setattr("tenex_cli.workflows.time.sleep", lambda _seconds: None)
    monkeypatch.setattr("tenex_cli.workflows.wait_for_ready", lambda *_args, **_kwargs: True)
    start_plan(cast("Runtime", runtime), "Implement feature")
    assert runtime.session.last_prompt is not None
    assert "Implement feature" in runtime.session.last_prompt
    assert len(runtime.session.load_panes()) == runtime.settings.pane_count + 1
    assert any("codex" in command for command in runtime.tmux.sent_commands)


def test_start_plan_escapes_prompt_text(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """start_plan should shell-escape the prompt text."""
    runtime = DummyRuntime(tmp_path)
    quoted_base = tmp_path / "user's state"
    runtime.session = DummySession(quoted_base)
    monkeypatch.setenv("TMUX", "1")
    monkeypatch.setattr("tenex_cli.workflows.time.sleep", lambda _seconds: None)
    monkeypatch.setattr("tenex_cli.workflows.wait_for_ready", lambda *_args, **_kwargs: True)

    task = "Implement feature in user's area"
    start_plan(cast("Runtime", runtime), task)

    sent_command = next(command for command in runtime.tmux.sent_commands if "codex" in command)
    expected_quoted_prompt = shlex.quote(runtime.session.last_prompt or "")
    assert expected_quoted_prompt in sent_command


def test_start_review_triggers_commands(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """start_review should send /review commands to panes."""
    runtime = DummyRuntime(tmp_path)
    monkeypatch.setenv("TMUX", "1")
    monkeypatch.setattr("tenex_cli.workflows.time.sleep", lambda _seconds: None)
    monkeypatch.setattr("tenex_cli.workflows.wait_for_ready", lambda *_args, **_kwargs: True)
    start_review(cast("Runtime", runtime), "main")
    assert any("/review" in cmd for cmd in runtime.tmux.sent_commands)


def test_step_and_continuous_prompt(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """step_prompt and continuous_prompt should send messages to panes."""
    runtime = DummyRuntime(tmp_path)
    runtime.session.set_panes(["%0", "%1"])
    runtime.tmux.captured["%1"] = ""
    monkeypatch.setenv("TMUX", "1")
    step_prompt(cast("Runtime", runtime), "do it")
    assert any("paste:%1" in cmd for cmd in runtime.tmux.sent_commands)

    monkeypatch.setattr("tenex_cli.workflows.pane_is_busy", lambda *_args, **_kwargs: False)

    def fake_sleep(_seconds: int) -> None:
        raise KeyboardInterrupt

    monkeypatch.setattr("tenex_cli.workflows.time.sleep", fake_sleep)
    continuous_prompt(cast("Runtime", runtime), "loop it")
    assert runtime.tmux.buffers


def test_collect_reviews(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """collect_reviews should capture panes and write aggregated file."""
    runtime = DummyRuntime(tmp_path)
    runtime.session.set_panes(["%0", "%1"])
    runtime.tmux.captured["%1"] = "review body"
    monkeypatch.setenv("TMUX", "1")
    path = collect_reviews(cast("Runtime", runtime))
    assert path.exists()
    data = path.read_text()
    assert "review body" in data


def test_collect_plans(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """collect_plans should capture panes and write aggregated plans file."""
    runtime = DummyRuntime(tmp_path)
    runtime.session.set_panes(["%0", "%1"])
    runtime.tmux.captured["%1"] = "plan body"
    monkeypatch.setenv("TMUX", "1")
    path = collect_plans(cast("Runtime", runtime))
    assert path.exists()
    data = path.read_text()
    assert "plan body" in data
