//! Planning prompts for child agents

/// Preamble for planning-only child agents
///
/// This prompt instructs agents to focus on research and planning
/// rather than implementation.
pub const PLAN_PREAMBLE: &str = r"You are conducting an intensive research and planning session to determine how to implement a specific task in this codebase.

Your mission is to **relentlessly explore, hypothesize, and investigate** until you have a comprehensive understanding of:
1. What needs to be implemented
2. Where it should be implemented in the codebase
3. How it should be implemented (architecture, patterns, dependencies)
4. What dependencies/libraries/tools are needed
5. What potential challenges or edge cases exist

**Research Methodology:**
- Systematically explore the codebase structure and existing patterns
- Search for similar existing implementations to understand conventions
- Identify all relevant files, modules, and components
- Trace dependencies and data flows
- Research external libraries or tools that might be needed
- Cross-reference multiple sources to validate your understanding
- Question your assumptions and seek contradictory evidence

**Output Requirements:**
Provide a structured report with:
1. **Executive Summary**: Key findings and recommended approach
2. **Codebase Analysis**: Relevant files, patterns, and conventions discovered
3. **Implementation Plan**: Detailed steps with specific file paths and changes
4. **Risks & Challenges**: Potential issues and mitigation strategies

**Task to Research and Plan:**";

/// Build a complete planning prompt with the task appended
#[must_use]
pub fn build_plan_prompt(task: &str) -> String {
    format!("{}\n{}", PLAN_PREAMBLE, task.trim())
}

/// Synthesis prompt template for aggregating child agent findings
pub const SYNTHESIS_TEMPLATE: &str = r"Here are findings from $COUNT parallel research sessions:

$FINDINGS

Please synthesize these findings and proceed with implementation. Focus on:
1. Common themes and agreements across agents
2. Unique insights from individual agents
3. Any contradictions and how to resolve them
4. A unified implementation approach based on the collective research";

/// Build a synthesis prompt from multiple agent findings
#[must_use]
pub fn build_synthesis_prompt(findings: &[(String, String)]) -> String {
    let count = findings.len();
    let findings_text = findings
        .iter()
        .enumerate()
        .map(|(i, (title, content))| format!("## Session {}: {}\n{}", i + 1, title, content))
        .collect::<Vec<_>>()
        .join("\n\n");

    SYNTHESIS_TEMPLATE
        .replace("$COUNT", &count.to_string())
        .replace("$FINDINGS", &findings_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_plan_prompt() {
        let prompt = build_plan_prompt("Implement user authentication");
        assert!(prompt.contains(PLAN_PREAMBLE));
        assert!(prompt.contains("Implement user authentication"));
    }

    #[test]
    fn test_build_plan_prompt_trims_whitespace() {
        let prompt = build_plan_prompt("  Task with spaces  ");
        assert!(prompt.ends_with("Task with spaces"));
    }

    #[test]
    fn test_build_synthesis_prompt() {
        let findings = vec![
            ("Agent 1".to_string(), "Finding 1".to_string()),
            ("Agent 2".to_string(), "Finding 2".to_string()),
        ];
        let prompt = build_synthesis_prompt(&findings);

        assert!(prompt.contains("2 parallel research sessions"));
        assert!(prompt.contains("Session 1: Agent 1"));
        assert!(prompt.contains("Finding 1"));
        assert!(prompt.contains("Session 2: Agent 2"));
        assert!(prompt.contains("Finding 2"));
    }
}
