"""Runtime wiring that bundles settings, logging, tmux, and session state."""

from __future__ import annotations

import os
from dataclasses import dataclass

import typer

from tenex_cli.config import Settings
from tenex_cli.logger import Logger
from tenex_cli.session import SessionManager
from tenex_cli.tmux import TmuxClient


@dataclass
class Runtime:
    """Bundle of core objects shared across commands."""

    settings: Settings
    logger: Logger
    tmux: TmuxClient
    session: SessionManager


def build_runtime() -> Runtime:
    """Instantiate runtime dependencies from configuration."""
    settings = Settings.from_env()
    logger = Logger()
    tmux = TmuxClient(logger)
    session = SessionManager(settings, tmux)
    return Runtime(settings=settings, logger=logger, tmux=tmux, session=session)


def ensure_tmux(logger: Logger) -> None:
    """Exit early when the user is not inside a tmux session."""
    if not os.environ.get("TMUX"):
        logger.error("This command must run inside a tmux session")
        raise typer.Exit(code=1)
