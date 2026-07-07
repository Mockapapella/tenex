use tenex::prompts::{PLAN_PREAMBLE, build_plan_prompt, build_synthesis_prompt};

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
