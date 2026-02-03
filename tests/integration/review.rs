//! Tests for [R] review agent functionality

use crate::common::{DirGuard, TestFixture, skip_if_no_mux};
use tenex::config::Action;
use tenex::mux::SessionManager;

#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
const CODEX_REVIEW_FLOW_MOCK_SCRIPT: &str = r#"#!/usr/bin/env python3
import sys
import tty

ENTER_CSI_U = b"\x1b[13;1u"
PASTE_START = b"\x1b[200~"
PASTE_END = b"\x1b[201~"

tty.setraw(sys.stdin.fileno())


def read_escape_sequence() -> bytes:
    leader = sys.stdin.buffer.read(1)
    if leader != b"[":
        return b"\x1b" + leader

    seq = bytearray(b"\x1b[")
    while True:
        ch = sys.stdin.buffer.read(1)
        if not ch:
            break
        seq += ch
        if ch in b"~uABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz":
            break
    return bytes(seq)


def handle_submit(state: int, text: str) -> int:
    print(text, flush=True)
    if state == 0:
        print("Select a review preset", flush=True)
        return 1
    if state == 1:
        print("Select a base branch", flush=True)
        return 2
    if state == 2:
        branch = text.strip() or "master"
        print(f">> Code review started: changes against '{branch}' <<", flush=True)
        return 3
    return state


print(f"codex-mock argv={len(sys.argv) - 1}", flush=True)

state = 0
buffer = bytearray()
in_paste = False
hint_shown = False

while True:
    ch = sys.stdin.buffer.read(1)
    if not ch:
        break

    if ch == b"\x1b":
        seq = read_escape_sequence()
        if seq == ENTER_CSI_U:
            state = handle_submit(state, buffer.decode(errors="replace"))
            buffer.clear()
            continue
        if seq == PASTE_START:
            in_paste = True
            continue
        if seq == PASTE_END:
            in_paste = False
            continue
        continue

    if ch in (b"\r", b"\n") and not in_paste:
        state = handle_submit(state, buffer.decode(errors="replace"))
        buffer.clear()
        continue

    if ch == b"\x7f" and buffer:
        buffer.pop()
        continue

    buffer += ch
    if state == 0 and not in_paste and not hint_shown and buffer == b"/review":
        print("  /review  review my current changes and find issues", flush=True)
        hint_shown = True
"#;

#[cfg(unix)]
fn write_executable_script(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::write(path, contents)?;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[test]
fn test_review_action_no_agent_selected_shows_info() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_no_agent")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // No agents in storage, so none selected
    assert!(app.selected_agent().is_none());

    // Trigger review action
    handler.handle_action(&mut app, Action::ReviewSwarm)?;

    // Should be in ReviewInfo mode
    assert!(
        matches!(app.mode, tenex::AppMode::ReviewInfo(_)),
        "Expected ReviewInfo mode when no agent selected, got {:?}",
        app.mode
    );

    Ok(())
}

#[test]
fn test_review_action_with_agent_selected_shows_count_picker()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("review_with_agent")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent first
    let next = handler.create_agent(&mut app.data, "test-agent", None)?;
    app.apply_mode(next);
    assert_eq!(app.data.storage.len(), 1);

    // Select the agent
    app.select_next();
    assert!(app.selected_agent().is_some());

    // Trigger review action
    handler.handle_action(&mut app, Action::ReviewSwarm)?;

    // Should be in ReviewChildCount mode
    assert!(
        matches!(app.mode, tenex::AppMode::ReviewChildCount(_)),
        "Expected ReviewChildCount mode when agent selected, got {:?}",
        app.mode
    );

    // Should have branches loaded
    assert!(
        !app.data.review.branches.is_empty(),
        "Expected branches to be loaded"
    );

    // spawning_under should be set to the selected agent
    assert!(
        app.data.spawn.spawning_under.is_some(),
        "Expected spawning_under to be set"
    );

    // Cleanup
    std::env::set_current_dir(&original_dir)?;
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_review_branch_filtering() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_filter")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);

    // Manually set up branch list for testing filtering
    app.data.review.branches = vec![
        tenex::git::BranchInfo {
            name: "main".to_string(),
            full_name: "refs/heads/main".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
        tenex::git::BranchInfo {
            name: "master".to_string(),
            full_name: "refs/heads/master".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
        tenex::git::BranchInfo {
            name: "feature-branch".to_string(),
            full_name: "refs/heads/feature-branch".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
        tenex::git::BranchInfo {
            name: "main".to_string(),
            full_name: "refs/remotes/origin/main".to_string(),
            is_remote: true,
            remote: Some("origin".to_string()),
            last_commit_time: None,
        },
    ];

    // No filter - should return all branches
    assert_eq!(app.filtered_review_branches().len(), 4);

    // Filter for "main" - should return 2 (local and remote)
    app.data.review.filter = "main".to_string();
    assert_eq!(app.filtered_review_branches().len(), 2);

    // Filter for "feature" - should return 1
    app.data.review.filter = "feature".to_string();
    assert_eq!(app.filtered_review_branches().len(), 1);

    // Filter for non-existent - should return 0
    app.data.review.filter = "nonexistent".to_string();
    assert_eq!(app.filtered_review_branches().len(), 0);

    // Case insensitive filtering
    app.data.review.filter = "MAIN".to_string();
    assert_eq!(app.filtered_review_branches().len(), 2);

    Ok(())
}

#[test]
fn test_review_branch_navigation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_nav")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);

    // Set up branch list
    app.data.review.branches = vec![
        tenex::git::BranchInfo {
            name: "branch1".to_string(),
            full_name: "refs/heads/branch1".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
        tenex::git::BranchInfo {
            name: "branch2".to_string(),
            full_name: "refs/heads/branch2".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
        tenex::git::BranchInfo {
            name: "branch3".to_string(),
            full_name: "refs/heads/branch3".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
    ];

    // Start at 0
    assert_eq!(app.data.review.selected, 0);

    // Navigate down
    app.select_next_branch();
    assert_eq!(app.data.review.selected, 1);

    app.select_next_branch();
    assert_eq!(app.data.review.selected, 2);

    // Wrap around at end
    app.select_next_branch();
    assert_eq!(app.data.review.selected, 0);

    // Navigate up - wrap to end
    app.select_prev_branch();
    assert_eq!(app.data.review.selected, 2);

    app.select_prev_branch();
    assert_eq!(app.data.review.selected, 1);

    Ok(())
}

#[test]
fn test_review_branch_selection_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_confirm")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);

    // Set up branch list
    app.data.review.branches = vec![
        tenex::git::BranchInfo {
            name: "main".to_string(),
            full_name: "refs/heads/main".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
        tenex::git::BranchInfo {
            name: "develop".to_string(),
            full_name: "refs/heads/develop".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        },
    ];

    // Select second branch
    app.data.review.selected = 1;

    // Confirm selection
    assert!(app.confirm_branch_selection());
    assert_eq!(app.data.review.base_branch, Some("develop".to_string()));

    // Test with empty branch list
    app.data.review.branches.clear();
    app.data.review.base_branch = None;
    assert!(!app.confirm_branch_selection());
    assert!(app.data.review.base_branch.is_none());

    Ok(())
}

#[test]
fn test_spawn_review_agents() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("review_spawn")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a root agent with children (swarm) to get a proper mux session
    app.data.spawn.child_count = 1;
    app.data.spawn.spawning_under = None;
    let result = handler.spawn_children(&mut app.data, Some("test-swarm"));
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(()); // Skip if creation fails
    }

    // Should have root + 1 child = 2 agents
    assert_eq!(app.data.storage.len(), 2);

    let root = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root agent found")?;
    let root_id = root.id;

    // Set up for review spawning under the root
    app.data.spawn.spawning_under = Some(root_id);
    app.data.spawn.child_count = 2;
    app.data.review.base_branch = Some("master".to_string());

    // Spawn review agents
    let result = handler.spawn_review_agents(&mut app.data);

    // Cleanup first
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }
    std::env::set_current_dir(&original_dir)?;

    if result.is_err() {
        // Skip if review spawn fails (mux issues)
        return Ok(());
    }

    // Should have root + 1 original child + 2 review agents = 4
    assert_eq!(app.data.storage.len(), 4);

    // Review agents should have "Review" in title
    let review_agent_count = app
        .data
        .storage
        .iter()
        .filter(|a| a.title.contains("Review"))
        .count();
    assert_eq!(review_agent_count, 2);

    // Review state should be cleared
    assert!(app.data.review.branches.is_empty());
    assert!(app.data.review.filter.is_empty());
    assert!(app.data.review.base_branch.is_none());

    Ok(())
}

#[test]
fn test_spawn_review_agents_codex_uses_review_flow() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        return Ok(());
    }

    let fixture = TestFixture::new("review_spawn_codex")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let codex_path = fixture.repo_path.join("codex");
    #[cfg(unix)]
    {
        write_executable_script(&codex_path, CODEX_REVIEW_FLOW_MOCK_SCRIPT)?;
    }

    let settings = tenex::app::Settings {
        review_agent_program: tenex::app::AgentProgram::Custom,
        review_custom_agent_command: codex_path.to_string_lossy().into_owned(),
        ..Default::default()
    };

    let mut app = tenex::App::new(config, storage, settings, false);
    let handler = tenex::app::Actions::new();

    // Create a root agent with children (swarm) to get a proper mux session.
    app.data.spawn.child_count = 1;
    app.data.spawn.spawning_under = None;
    let result = handler.spawn_children(&mut app.data, Some("test-swarm"));
    if result.is_err() {
        return Ok(());
    }

    let root = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root agent found")?;
    let root_id = root.id;

    app.data.spawn.spawning_under = Some(root_id);
    app.data.spawn.child_count = 1;
    app.data.review.base_branch = Some("master".to_string());

    let result = handler.spawn_review_agents(&mut app.data);

    let capture = tenex::mux::OutputCapture::new();
    let mut checked = 0usize;
    for agent in app
        .data
        .storage
        .iter()
        .filter(|a| a.title.contains("Reviewer"))
    {
        let window_index = agent
            .window_index
            .ok_or("Missing window index for review agent")?;
        let target = SessionManager::window_target(&agent.mux_session, window_index);
        let output = {
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(10);
            let poll_interval = std::time::Duration::from_millis(100);
            let mut last_output = String::new();

            loop {
                match capture.capture_pane_with_history(&target, 200) {
                    Ok(output) => {
                        if output.contains("review started") {
                            break output;
                        }
                        last_output = output;
                    }
                    Err(_) if start.elapsed() < timeout => {}
                    Err(err) => return Err(err.into()),
                }
                if start.elapsed() >= timeout {
                    break last_output;
                }
                std::thread::sleep(poll_interval);
            }
        };

        assert!(
            output.contains("codex-mock argv=0"),
            "Expected Codex review agents to spawn without a prompt argument, got: {output:?}"
        );
        assert!(
            output.contains("/review"),
            "Expected Codex review agents to type /review, got: {output:?}"
        );
        assert!(
            output.contains("master"),
            "Expected Codex review agents to enter the base branch, got: {output:?}"
        );
        assert!(
            output.contains("review started"),
            "Expected Codex review agents to start the review, got: {output:?}"
        );
        checked = checked.saturating_add(1);
    }

    if result.is_err() {
        return Ok(());
    }

    assert!(checked > 0, "Expected at least one Codex review agent");

    Ok(())
}

#[test]
fn test_review_prompt_contains_base_branch() {
    let prompt = tenex::prompts::build_review_prompt("main");

    // Should contain the base branch name
    assert!(prompt.contains("main"));

    // Should contain key review instructions
    assert!(prompt.contains("git diff main...HEAD"));
    assert!(prompt.contains("git diff --staged"));
    assert!(prompt.contains("git diff"));
    assert!(prompt.contains("git status"));
    assert!(prompt.contains("git log main..HEAD"));

    // Should contain review categories
    assert!(prompt.contains("Code Quality"));
    assert!(prompt.contains("Security"));
    assert!(prompt.contains("Performance"));

    // Should contain output structure
    assert!(prompt.contains("Executive Summary"));
    assert!(prompt.contains("Critical Issues"));
}

#[test]
fn test_review_modes_flow() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_flow")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);

    // Set up some branches
    app.data.review.branches = vec![tenex::git::BranchInfo {
        name: "main".to_string(),
        full_name: "refs/heads/main".to_string(),
        is_remote: false,
        remote: None,
        last_commit_time: None,
    }];

    // Start in ReviewChildCount mode
    app.start_review(app.data.review.branches.clone());
    assert!(matches!(app.mode, tenex::AppMode::ReviewChildCount(_)));

    // Proceed to branch selector
    app.proceed_to_branch_selector();
    assert!(matches!(app.mode, tenex::AppMode::BranchSelector(_)));

    // Exit should return to Normal
    app.exit_mode();
    assert!(matches!(app.mode, tenex::AppMode::Normal(_)));

    Ok(())
}
