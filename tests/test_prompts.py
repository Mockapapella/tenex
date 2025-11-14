"""Prompt helpers tests."""

from pathlib import Path

from tenex_cli.prompts import aggregated_plans_prompt
from tenex_cli.prompts import aggregated_reviews_prompt
from tenex_cli.prompts import build_plan_prompt


def test_build_plan_prompt_includes_task() -> None:
    """The user task should be appended to the template."""
    task = "Ship it"
    prompt = build_plan_prompt(task)
    assert task in prompt
    assert "research" in prompt.lower()


def test_aggregated_prompts_reference_path(tmp_path: Path) -> None:
    """Generated prompts should mention the backing file path."""
    plan_file = tmp_path / "plan.md"
    review_file = tmp_path / "review.md"

    plan = aggregated_plans_prompt(plan_file)
    review = aggregated_reviews_prompt(review_file)

    assert str(plan_file) in plan
    assert str(review_file) in review
