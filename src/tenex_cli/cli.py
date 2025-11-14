"""Typer entrypoints for the tenex workflow."""

from __future__ import annotations

import typer

from tenex_cli.editor import gather_text
from tenex_cli.runtime import build_runtime
from tenex_cli.workflows import collect_plans
from tenex_cli.workflows import collect_reviews
from tenex_cli.workflows import continuous_prompt
from tenex_cli.workflows import reset
from tenex_cli.workflows import start_plan
from tenex_cli.workflows import start_review
from tenex_cli.workflows import step_prompt

app = typer.Typer(
    help="Modern Python implementation of the tenex automation workflow.",
    no_args_is_help=True,
    add_completion=False,
)


@app.command()
def review(branch: str = typer.Argument(..., help="Git branch to review")) -> None:
    """Launch the multi-pane Codex review workflow for the provided branch."""
    runtime = build_runtime()
    start_review(runtime, branch)


@app.command("collect-reviews")
def collect_reviews_command() -> None:
    """Capture pane output, aggregate reviews, and launch a synthesis session."""
    runtime = build_runtime()
    collect_reviews(runtime)


@app.command()
def plan(task: str | None = typer.Argument(None, help="Task to research")) -> None:
    """Fan out Codex research panes to investigate a task."""
    runtime = build_runtime()
    resolved_task = gather_text(task, "plan", runtime.logger)
    start_plan(runtime, resolved_task)


@app.command("collect-plans")
def collect_plans_command() -> None:
    """Aggregate research transcripts and start a synthesis prompt."""
    runtime = build_runtime()
    collect_plans(runtime)


@app.command()
def step(prompt: str | None = typer.Argument(None, help="Prompt to broadcast")) -> None:
    """Send a single prompt to every Codex pane immediately."""
    runtime = build_runtime()
    resolved_prompt = gather_text(prompt, "step", runtime.logger)
    step_prompt(runtime, resolved_prompt)


@app.command(name="continue")
def continue_command(prompt: str | None = typer.Argument(None, help="Prompt to loop")) -> None:
    """Keep broadcasting prompts when panes report readiness."""
    runtime = build_runtime()
    resolved_prompt = gather_text(prompt, "continue", runtime.logger)
    continuous_prompt(runtime, resolved_prompt)


@app.command("reset")
def reset_command() -> None:
    """Close Codex panes and delete the session directory."""
    runtime = build_runtime()
    reset(runtime)


def main() -> None:
    """Entrypoint for `tenex`."""
    app()


if __name__ == "__main__":  # pragma: no cover
    main()
