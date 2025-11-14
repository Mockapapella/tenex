"""Static prompt templates shared across commands."""

from __future__ import annotations

from pathlib import Path

PLAN_PREAMBLE = (
    "You are conducting an intensive research and planning session to determine how to implement "
    "a specific task in this codebase.\n\n"
    "Your mission is to **relentlessly explore, hypothesize, and investigate** until you have a "
    "comprehensive understanding of:\n"
    "1. What needs to be implemented\n"
    "2. Where it should be implemented in the codebase\n"
    "3. How it should be implemented (architecture, patterns, dependencies)\n"
    "4. What dependencies/libraries/tools are needed\n"
    "5. What potential challenges or edge cases exist\n\n"
    "**Research Methodology:**\n"
    "- Systematically explore the codebase structure and existing patterns\n"
    "- Search for similar existing implementations to understand conventions\n"
    "- Identify all relevant files, modules, and components\n"
    "- Trace dependencies and data flows\n"
    "- Research external libraries or tools that might be needed\n"
    "- Cross-reference multiple sources to validate your understanding\n"
    "- Question your assumptions and seek contradictory evidence\n\n"
    "**Key Questions to Answer:**\n"
    "- What similar features already exist in this codebase?\n"
    "- What architectural patterns does this codebase use?\n"
    "- What are the established conventions (naming, structure, testing)?\n"
    "- What existing code can be reused or extended?\n"
    "- What are the integration points with existing systems?\n"
    "- What are the performance/security/scalability considerations?\n"
    "- What edge cases need to be handled?\n\n"
    "**Output Requirements:**\n"
    "Provide a structured report with:\n"
    "1. **Executive Summary**: Key findings and recommended approach\n"
    "2. **Codebase Analysis**: Relevant files, patterns, and conventions discovered\n"
    "3. **Implementation Plan**: Detailed steps with specific file paths and changes\n"
    "4. **Dependencies**: External libraries, tools, or services needed\n"
    "5. **Risks & Challenges**: Potential issues and mitigation strategies\n"
    "6. **Open Questions**: Areas needing clarification or further investigation\n\n"
    "**Task to Research and Plan:**"
)


def build_plan_prompt(task: str) -> str:
    """Insert the user assignment beneath the planning preamble."""
    cleaned_task = task.strip()
    return f"{PLAN_PREAMBLE}\n{cleaned_task}\n"


def aggregated_reviews_prompt(path: Path) -> str:
    """Return the synthesis prompt for collected review transcripts."""
    path_string = str(Path(path))
    return (
        "Please analyze the code reviews in the file "
        f"{path_string}. These are reviews from multiple reviewers. Determine if any of their "
        "findings are legitimate issues that should be fixed. If you find legitimate issues, "
        "please fix them."
    )


def aggregated_plans_prompt(path: Path) -> str:
    """Return the synthesis prompt for collected planning transcripts."""
    path_string = str(Path(path))
    return (
        "Please analyze the planning and research findings in the file "
        f"{path_string}. These are from multiple independent planning sessions. "
        "Synthesize the findings into a comprehensive implementation plan. Identify common "
        "patterns, resolve contradictions, and provide a unified recommendation with "
        "specific actionable steps."
    )
