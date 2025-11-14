"""Session management for per-window tenex state on disk."""

from __future__ import annotations

import getpass
import os
import shutil
from pathlib import Path

from tenex_cli.config import Settings
from tenex_cli.tmux import TmuxClient


class SessionManager:
    """Persist tmux pane metadata and prompts on disk."""

    def __init__(self, settings: Settings, tmux: TmuxClient) -> None:
        """Store references to settings and tmux helper."""
        self.settings = settings
        self.tmux = tmux
        self.session_dir_cache: Path | None = None

    def session_dir(self) -> Path:
        """Return the cached session directory path for this tmux window."""
        if self.session_dir_cache is None:
            descriptor = self.session_descriptor()
            base_root = Path(self.settings.state_root)
            self.session_dir_cache = base_root / f"codex-review-{getpass.getuser()}-{descriptor}"
        return self.session_dir_cache

    def ensure_directory(self) -> Path:
        """Create the session directory if it does not already exist."""
        path = self.session_dir()
        path.mkdir(parents=True, exist_ok=True)
        return path

    def session_exists(self) -> bool:
        """Return True when the session directory exists."""
        return self.session_dir().exists()

    def pane_ids_path(self) -> Path:
        """Return the path storing pane identifiers."""
        return self.session_dir() / "pane_ids"

    def original_pane_path(self) -> Path:
        """Return the path storing the originating pane id."""
        return self.session_dir() / "original_pane"

    def prompt_path(self) -> Path:
        """Return the path holding the active prompt text."""
        return self.session_dir() / "prompt.txt"

    def aggregated_reviews_path(self) -> Path:
        """Return the path used for aggregated review outputs."""
        return self.session_dir() / "aggregated_reviews.txt"

    def aggregated_plans_path(self) -> Path:
        """Return the path used for aggregated planning outputs."""
        return self.session_dir() / "aggregated_plans.txt"

    def store_panes(self, panes: list[str]) -> None:
        """Persist the list of pane identifiers to disk."""
        self.ensure_directory()
        with self.pane_ids_path().open("w", encoding="utf-8") as handle:
            for pane in panes:
                handle.write(f"{pane}\n")

    def load_panes(self) -> list[str]:
        """Load pane identifiers from the stored file."""
        with self.pane_ids_path().open("r", encoding="utf-8") as handle:
            return [line.strip() for line in handle if line.strip()]

    def store_original_pane(self, pane_id: str) -> None:
        """Persist the originating pane id for later restoration."""
        self.ensure_directory()
        self.original_pane_path().write_text(pane_id, encoding="utf-8")

    def load_original_pane(self) -> str:
        """Read the previously stored originating pane id."""
        return self.original_pane_path().read_text(encoding="utf-8").strip()

    def store_prompt(self, text: str) -> None:
        """Persist the generated prompt to disk."""
        self.ensure_directory()
        self.prompt_path().write_text(text, encoding="utf-8")

    def cleanup(self) -> None:
        """Delete the entire session directory tree."""
        session_dir = self.session_dir()
        if session_dir.exists():
            shutil.rmtree(session_dir)

    def session_descriptor(self) -> str:
        """Return a stable identifier derived from tmux or the current PID."""
        if os.environ.get("TMUX"):
            return self.tmux.display_message("#{session_name}-#{window_id}")
        return f"shell-{os.getpid()}"
