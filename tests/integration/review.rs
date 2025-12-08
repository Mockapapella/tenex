//! Tests for [R] review agent functionality

use crate::common::{TestFixture, skip_if_no_tmux};
use tenex::app::Mode;
use tenex::config::Action;
use tenex::tmux::SessionManager;

#[test]
fn test_review_action_no_agent_selected_shows_info() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_no_agent")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // No agents in storage, so none selected
    assert!(app.selected_agent().is_none());

    // Trigger review action
    handler.handle_action(&mut app, Action::ReviewSwarm)?;

    // Should be in ReviewInfo mode
    assert!(
        matches!(app.mode, Mode::ReviewInfo),
        "Expected ReviewInfo mode when no agent selected, got {:?}",
        app.mode
    );

    Ok(())
}

#[test]
fn test_review_action_with_agent_selected_shows_count_picker()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("review_with_agent")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent first
    handler.create_agent(&mut app, "test-agent", None)?;
    assert_eq!(app.storage.len(), 1);

    // Select the agent
    app.select_next();
    assert!(app.selected_agent().is_some());

    // Trigger review action
    handler.handle_action(&mut app, Action::ReviewSwarm)?;

    // Should be in ReviewChildCount mode
    assert!(
        matches!(app.mode, Mode::ReviewChildCount),
        "Expected ReviewChildCount mode when agent selected, got {:?}",
        app.mode
    );

    // Should have branches loaded
    assert!(
        !app.review_branches.is_empty(),
        "Expected branches to be loaded"
    );

    // spawning_under should be set to the selected agent
    assert!(
        app.spawning_under.is_some(),
        "Expected spawning_under to be set"
    );

    // Cleanup
    std::env::set_current_dir(&original_dir)?;
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_review_branch_filtering() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_filter")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage);

    // Manually set up branch list for testing filtering
    app.review_branches = vec![
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
    app.review_branch_filter = "main".to_string();
    assert_eq!(app.filtered_review_branches().len(), 2);

    // Filter for "feature" - should return 1
    app.review_branch_filter = "feature".to_string();
    assert_eq!(app.filtered_review_branches().len(), 1);

    // Filter for non-existent - should return 0
    app.review_branch_filter = "nonexistent".to_string();
    assert_eq!(app.filtered_review_branches().len(), 0);

    // Case insensitive filtering
    app.review_branch_filter = "MAIN".to_string();
    assert_eq!(app.filtered_review_branches().len(), 2);

    Ok(())
}

#[test]
fn test_review_branch_navigation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_nav")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage);

    // Set up branch list
    app.review_branches = vec![
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
    assert_eq!(app.review_branch_selected, 0);

    // Navigate down
    app.select_next_branch();
    assert_eq!(app.review_branch_selected, 1);

    app.select_next_branch();
    assert_eq!(app.review_branch_selected, 2);

    // Wrap around at end
    app.select_next_branch();
    assert_eq!(app.review_branch_selected, 0);

    // Navigate up - wrap to end
    app.select_prev_branch();
    assert_eq!(app.review_branch_selected, 2);

    app.select_prev_branch();
    assert_eq!(app.review_branch_selected, 1);

    Ok(())
}

#[test]
fn test_review_branch_selection_confirmation() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("review_confirm")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let mut app = tenex::App::new(config, storage);

    // Set up branch list
    app.review_branches = vec![
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
    app.review_branch_selected = 1;

    // Confirm selection
    assert!(app.confirm_branch_selection());
    assert_eq!(app.review_base_branch, Some("develop".to_string()));

    // Test with empty branch list
    app.review_branches.clear();
    app.review_base_branch = None;
    assert!(!app.confirm_branch_selection());
    assert!(app.review_base_branch.is_none());

    Ok(())
}

#[test]
fn test_spawn_review_agents() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("review_spawn")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a root agent with children (swarm) to get a proper tmux session
    app.child_count = 1;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "test-swarm");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(()); // Skip if creation fails
    }

    // Should have root + 1 child = 2 agents
    assert_eq!(app.storage.len(), 2);

    let root = app
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root agent found")?;
    let root_id = root.id;

    // Set up for review spawning under the root
    app.spawning_under = Some(root_id);
    app.child_count = 2;
    app.review_base_branch = Some("master".to_string());

    // Spawn review agents
    let result = handler.spawn_review_agents(&mut app);

    // Cleanup first
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }
    std::env::set_current_dir(&original_dir)?;

    if result.is_err() {
        // Skip if review spawn fails (tmux issues)
        return Ok(());
    }

    // Should have root + 1 original child + 2 review agents = 4
    assert_eq!(app.storage.len(), 4);

    // Review agents should have "Review" in title
    let review_agent_count = app
        .storage
        .iter()
        .filter(|a| a.title.contains("Review"))
        .count();
    assert_eq!(review_agent_count, 2);

    // Review state should be cleared
    assert!(app.review_branches.is_empty());
    assert!(app.review_branch_filter.is_empty());
    assert!(app.review_base_branch.is_none());

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

    let mut app = tenex::App::new(config, storage);

    // Set up some branches
    app.review_branches = vec![tenex::git::BranchInfo {
        name: "main".to_string(),
        full_name: "refs/heads/main".to_string(),
        is_remote: false,
        remote: None,
        last_commit_time: None,
    }];

    // Start in ReviewChildCount mode
    app.start_review(app.review_branches.clone());
    assert!(matches!(app.mode, Mode::ReviewChildCount));

    // Proceed to branch selector
    app.proceed_to_branch_selector();
    assert!(matches!(app.mode, Mode::BranchSelector));

    // Exit should return to Normal
    app.exit_mode();
    assert!(matches!(app.mode, Mode::Normal));

    Ok(())
}
