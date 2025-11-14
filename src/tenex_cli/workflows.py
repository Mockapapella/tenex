"""High-level workflows for review, planning, collection, and reset."""

from __future__ import annotations

import shlex
import time
from pathlib import Path

from tenex_cli.codex import pane_is_busy
from tenex_cli.codex import send_text
from tenex_cli.codex import wait_for_ready
from tenex_cli.prompts import aggregated_plans_prompt
from tenex_cli.prompts import aggregated_reviews_prompt
from tenex_cli.prompts import build_plan_prompt
from tenex_cli.runtime import Runtime
from tenex_cli.runtime import ensure_tmux


def start_review(runtime: Runtime, branch: str) -> None:
    """Spin up Codex panes and trigger /review across them."""
    ensure_tmux(runtime.logger)
    original_pane = runtime.tmux.current_pane()
    runtime.session.ensure_directory()
    runtime.session.store_original_pane(original_pane)

    pane_ids = create_codex_panes(runtime, original_pane, runtime.settings.codex_command)
    runtime.session.store_panes(pane_ids)

    runtime.logger.info("Waiting for codex instances to launch...")
    time.sleep(5)

    for index, pane_id in enumerate(pane_ids[1:], start=1):
        runtime.logger.info(
            f"Starting review in pane {pane_id} ({index}/{runtime.settings.pane_count})..."
        )
        if not wait_for_ready(runtime.tmux, pane_id, runtime.settings, runtime.logger):
            continue
        runtime.tmux.send_keys(pane_id, "/review")
        time.sleep(0.1)
        runtime.tmux.send_enter(pane_id, times=2)
        time.sleep(2)
        runtime.tmux.send_keys(pane_id, branch)
        runtime.tmux.send_enter(pane_id)
    runtime.logger.info("Reviews started. Run 'tenex collect-reviews' when ready.")


def start_plan(runtime: Runtime, task: str) -> None:
    """Launch Codex panes seeded with the long-form planning prompt."""
    ensure_tmux(runtime.logger)
    original_pane = runtime.tmux.current_pane()
    runtime.session.ensure_directory()
    runtime.session.store_original_pane(original_pane)

    prompt_text = build_plan_prompt(task)
    runtime.session.store_prompt(prompt_text)
    quoted_prompt = shlex.quote(prompt_text)
    command = f"{runtime.settings.codex_command} {quoted_prompt}"

    pane_ids = create_codex_panes(runtime, original_pane, command)
    runtime.session.store_panes(pane_ids)
    runtime.logger.info("Planning sessions started. Run 'tenex collect-plans' when ready.")


def create_codex_panes(runtime: Runtime, original_pane: str, command: str) -> list[str]:
    """Create panes relative to the original and launch `command` in each."""
    pane_ids = [original_pane]
    window_id = runtime.tmux.current_window()
    for index in range(runtime.settings.pane_count):
        horizontal = (index + 1) % 2 == 1
        runtime.logger.info(f"Creating codex pane {index + 1}/{runtime.settings.pane_count}...")
        new_pane = runtime.tmux.split_window(original_pane, horizontal=horizontal)
        pane_ids.append(new_pane)
        runtime.tmux.select_layout("tiled", window_id)
        runtime.tmux.send_keys(new_pane, command)
        runtime.tmux.send_enter(new_pane)
        time.sleep(0.1)
    return pane_ids


def collect_reviews(runtime: Runtime) -> Path:
    """Capture review transcripts, aggregate them, and launch synthesis."""
    ensure_tmux(runtime.logger)
    pane_ids = load_panes(runtime)
    original_pane = pane_ids[0]
    aggregated_file = runtime.session.aggregated_reviews_path()
    write_header(aggregated_file, "Aggregated Code Reviews")

    collect_from_panes(runtime, pane_ids, aggregated_file, "Review")

    runtime.tmux.select_pane(original_pane)
    prompt = aggregated_reviews_prompt(aggregated_file)
    launch_codex_with_prompt(runtime, prompt)
    runtime.logger.info(f"Codex is now analyzing {aggregated_file}.")
    return aggregated_file


def collect_plans(runtime: Runtime) -> Path:
    """Capture planning transcripts, aggregate them, and launch synthesis."""
    ensure_tmux(runtime.logger)
    pane_ids = load_panes(runtime)
    original_pane = pane_ids[0]
    aggregated_file = runtime.session.aggregated_plans_path()
    write_header(aggregated_file, "Aggregated Planning & Research Findings")

    collect_from_panes(runtime, pane_ids, aggregated_file, "Planning Session")

    runtime.tmux.select_pane(original_pane)
    prompt = aggregated_plans_prompt(aggregated_file)
    launch_codex_with_prompt(runtime, prompt)
    runtime.logger.info(f"Codex is now synthesizing {aggregated_file}.")
    return aggregated_file


def write_header(path: Path, title: str) -> None:
    """Write the markdown header that prefixes aggregate files."""
    target_path = Path(path)
    target_path.parent.mkdir(parents=True, exist_ok=True)
    with target_path.open("w", encoding="utf-8") as handle:
        handle.write(f"# {title}\n")
        handle.write(f"# Generated: {time.strftime('%c')}\n\n")


def collect_from_panes(
    runtime: Runtime,
    pane_ids: list[str],
    aggregated_file: Path,
    section_title: str,
) -> None:
    """Append each pane's output into the aggregate file and close panes."""
    target_file = Path(aggregated_file)
    with target_file.open("a", encoding="utf-8") as handle:
        for index, pane_id in enumerate(pane_ids[1:], start=1):
            if not runtime.tmux.pane_exists(pane_id):
                runtime.logger.warning(f"Pane {pane_id} missing, skipping...")
                continue
            runtime.logger.info(
                f"Collecting from pane {pane_id} ({index}/{runtime.settings.pane_count})..."
            )
            handle.write(f"## {section_title} from Pane {index} ({pane_id})\n\n")
            handle.write(runtime.tmux.capture_pane(pane_id, entire=True))
            handle.write("\n---\n\n")
            runtime.tmux.kill_pane(pane_id)


def launch_codex_with_prompt(runtime: Runtime, prompt: str) -> None:
    """Send a codex command referencing a prompt to the original pane."""
    original_pane = runtime.session.load_original_pane()
    command = f"{runtime.settings.codex_command} {shlex.quote(prompt)}"
    runtime.tmux.send_keys(original_pane, command)
    runtime.tmux.send_enter(original_pane)


def load_panes(runtime: Runtime) -> list[str]:
    """Load pane metadata, failing if no session data exists."""
    if not runtime.session.session_exists():
        runtime.logger.error("No active session found. Start a review or plan first.")
        raise SystemExit(1)
    return runtime.session.load_panes()


def step_prompt(runtime: Runtime, prompt: str) -> None:
    """Broadcast a prompt to each Codex pane once."""
    ensure_tmux(runtime.logger)
    pane_ids = load_panes(runtime)
    for index, pane_id in enumerate(pane_ids[1:], start=1):
        if not runtime.tmux.pane_exists(pane_id):
            runtime.logger.warning(f"Pane {pane_id} missing, skipping...")
            continue
        runtime.logger.info(
            f"Sending prompt to pane {pane_id} ({index}/{runtime.settings.pane_count})..."
        )
        send_text(runtime.tmux, pane_id, prompt)
    runtime.logger.info("Prompt broadcast complete.")


def continuous_prompt(runtime: Runtime, prompt: str) -> None:
    """Continuously send prompts whenever panes report readiness."""
    ensure_tmux(runtime.logger)
    pane_ids = load_panes(runtime)
    last_sent: dict[str, float] = {}
    runtime.logger.info("Continuous mode enabled. Press Ctrl+C to stop.")
    try:
        while True:
            current_time = time.time()
            for pane_id in pane_ids[1:]:
                if not runtime.tmux.pane_exists(pane_id):
                    continue
                if pane_is_busy(runtime.tmux, pane_id, runtime.settings):
                    continue
                previous_time = last_sent.get(pane_id, 0)
                if current_time - previous_time < runtime.settings.continue_cooldown_seconds:
                    continue
                runtime.logger.info(f"Pane {pane_id} ready, sending prompt...")
                send_text(runtime.tmux, pane_id, prompt)
                last_sent[pane_id] = current_time
            time.sleep(runtime.settings.continue_poll_seconds)
    except KeyboardInterrupt:
        runtime.logger.warning("Continuous mode stopped by user.")


def reset(runtime: Runtime) -> None:
    """Tear down spawned panes and delete the session directory."""
    ensure_tmux(runtime.logger)
    if not runtime.session.session_exists():
        runtime.logger.error("No session data found to reset.")
        raise SystemExit(1)
    pane_ids = runtime.session.load_panes()
    original_pane = pane_ids[0]
    for pane_id in pane_ids[1:]:
        if runtime.tmux.pane_exists(pane_id):
            runtime.logger.info(f"Closing pane {pane_id}...")
            runtime.tmux.kill_pane(pane_id)
    runtime.tmux.select_pane(original_pane)
    runtime.session.cleanup()
    runtime.logger.info("All codex panes closed and session data removed.")
