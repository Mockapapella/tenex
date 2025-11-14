"""Input helpers that read from CLI arguments, stdin, or $EDITOR."""

from __future__ import annotations

import os
import shutil
import sys
from typing import Protocol
from typing import TextIO

import typer

from tenex_cli.logger import Logger


class EditorFunc(Protocol):
    """Protocol describing editor callbacks."""

    def __call__(self, prompt_type: str) -> str:  # pragma: no cover - protocol stub
        """Return user-supplied text."""
        ...


def gather_text(
    value: str | None,
    prompt_type: str,
    logger: Logger,
    *,
    stdin: TextIO | None = None,
    editor_func: EditorFunc | None = None,
) -> str:
    """Return CLI text prioritising explicit values, stdin, then $EDITOR."""
    direct_value = value.strip() if value else ""
    if direct_value:
        return direct_value

    stream = stdin or sys.stdin
    if not stream.isatty():
        stdin_value = stream.read().strip()
        if stdin_value:
            return stdin_value

    logger.info("Opening editor for input...")
    editor_callable = editor_func or open_in_editor
    if editor_func is not None and not callable(editor_func):
        raise TypeError("editor_func must be callable")
    edited = editor_callable(prompt_type)
    if not edited.strip():
        logger.error("No input provided; aborting")
        raise SystemExit(1)
    return edited.strip()


def open_in_editor(prompt_type: str) -> str:
    """Launch the resolved editor via Typer and return the captured text."""
    editor = resolve_editor()
    original_editor = os.environ.get("EDITOR")
    initial_text = f"# Enter {prompt_type} here\n"
    try:
        os.environ["EDITOR"] = editor
        contents = typer.edit(text=initial_text)
    finally:
        if original_editor is not None:
            os.environ["EDITOR"] = original_editor
        else:
            os.environ.pop("EDITOR", None)
    if not contents:
        return ""
    stripped = "\n".join(line for line in contents.splitlines() if not line.startswith("# "))
    return stripped.strip()


def resolve_editor() -> str:
    """Determine which editor command to run for text input."""
    preferred = os.environ.get("EDITOR")
    if preferred and shutil.which(preferred.split()[0]):
        return preferred
    for candidate in ("vim", "nano"):
        if shutil.which(candidate):
            return candidate
    raise RuntimeError("No suitable editor found. Set the EDITOR environment variable.")
