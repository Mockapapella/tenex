"""Simple Rich-backed logger used for CLI output."""

from __future__ import annotations

from rich.console import Console
from rich.theme import Theme


class Logger:
    """Lightweight logger that wraps Rich for consistent styling."""

    def __init__(self, console: Console | None = None) -> None:
        """Initialise the Rich console with the tenex theme."""
        theme = Theme(
            {
                "info": "green",
                "warn": "yellow",
                "error": "red",
            }
        )
        self.console = console or Console(theme=theme)

    def info(self, message: str) -> None:
        """Print an informational message."""
        self.console.print(f"[info][INFO][/info] {message}")

    def warn(self, message: str) -> None:
        """Print a warning message."""
        self.console.print(f"[warn][WARN][/warn] {message}")

    def warning(self, message: str) -> None:
        """Alias for :meth:`warn` to match stdlib logging API."""
        self.warn(message)

    def error(self, message: str) -> None:
        """Print an error message."""
        self.console.print(f"[error][ERROR][/error] {message}")
