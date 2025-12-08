//! Worktree conflict detection and resolution tests

use std::fs;

use crate::common::{TestFixture, skip_if_no_tmux};
use tenex::app::{Actions, App};

/// Test that creating an agent detects existing worktree and enters conflict mode
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_detection_single_agent() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_conflict_single")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);
    let handler = Actions::new();

    // First, create a worktree manually to simulate existing state
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/existing-agent", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Now try to create an agent with the same name
    // This should detect the conflict and enter Confirming mode
    handler.create_agent(&mut app, "existing-agent", Some("test prompt"))?;

    // Should be in Confirming(WorktreeConflict) mode
    assert!(
        matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Expected Confirming(WorktreeConflict) mode, got {:?}",
        app.mode
    );

    // Conflict info should be populated
    let conflict = app
        .worktree_conflict
        .as_ref()
        .expect("Conflict info should be set");
    assert_eq!(conflict.title, "existing-agent");
    assert_eq!(conflict.prompt, Some("test prompt".to_string()));
    assert!(
        conflict.swarm_child_count.is_none(),
        "Should not be a swarm"
    );

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test reconnecting to existing worktree for a single agent
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_reconnect_single_agent() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_reconnect_single")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);
    let handler = Actions::new();

    // Create a worktree manually
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/reconnect-test", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Trigger conflict detection
    handler.create_agent(&mut app, "reconnect-test", Some("original prompt"))?;

    // Verify we're in conflict mode
    assert!(matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ));

    // Modify the prompt before reconnecting (simulating user editing)
    if let Some(ref mut conflict) = app.worktree_conflict {
        conflict.prompt = Some("modified prompt".to_string());
    }

    // Now reconnect
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.reconnect_to_worktree(&mut app)?;

    // Should have created an agent
    assert_eq!(app.storage.len(), 1, "Should have one agent");

    let agent = app.storage.iter().next().expect("Should have an agent");
    assert_eq!(agent.title, "reconnect-test");
    assert_eq!(agent.initial_prompt, Some("modified prompt".to_string()));

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test recreating worktree (delete and create fresh)
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_recreate_single_agent() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_recreate_single")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);
    let handler = Actions::new();

    // Create a worktree manually with some content
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/recreate-test", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Add a marker file to the old worktree
    let marker_path = worktree_path.join("old_marker.txt");
    fs::write(&marker_path, "old worktree")?;
    assert!(
        marker_path.exists(),
        "Marker file should exist before recreate"
    );

    // Trigger conflict detection
    handler.create_agent(&mut app, "recreate-test", Some("new prompt"))?;

    // Verify we're in conflict mode
    assert!(matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ));

    // Now recreate (delete and create fresh)
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.recreate_worktree(&mut app)?;

    // Should have created an agent
    assert_eq!(app.storage.len(), 1, "Should have one agent");

    let agent = app.storage.iter().next().expect("Should have an agent");
    assert_eq!(agent.title, "recreate-test");
    assert_eq!(agent.initial_prompt, Some("new prompt".to_string()));

    // The old marker file should be gone (worktree was recreated)
    assert!(
        !marker_path.exists(),
        "Old marker file should be gone after recreate"
    );

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test worktree conflict detection for swarm creation (S key)
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_detection_swarm() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_conflict_swarm")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // Create a worktree manually that matches what spawn_children would create
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/swarm-task", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Set up for swarm spawning (simulating S key flow)
    app.spawning_under = None; // No parent = new root swarm
    app.child_count = 3;

    // Try to spawn children - should detect conflict
    let handler = Actions::new();
    handler.spawn_children(&mut app, "swarm-task")?;

    // Should be in Confirming(WorktreeConflict) mode
    assert!(
        matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Expected Confirming(WorktreeConflict) mode, got {:?}",
        app.mode
    );

    // Conflict info should indicate this is a swarm
    let conflict = app
        .worktree_conflict
        .as_ref()
        .expect("Conflict info should be set");
    assert_eq!(
        conflict.swarm_child_count,
        Some(3),
        "Should remember child count"
    );
    assert_eq!(conflict.prompt, Some("swarm-task".to_string()));

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test reconnecting to existing worktree for swarm - verifies children get the updated prompt
#[test]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test assertions")]
fn test_worktree_conflict_reconnect_swarm_children_get_prompt()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_reconnect_swarm")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Use sleep command to keep sessions alive for swarm tests
    let mut config = fixture.config();
    config.default_program = "sleep 60".to_string();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // Create a worktree manually with a branch name that matches what spawn_children will generate
    // spawn_children uses the task as the title, which gets converted to a branch name
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let task = "swarm-reconnect-task";
    let branch_name = app.config.generate_branch_name(task);
    let worktree_path = app.config.worktree_dir.join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Set up for swarm spawning
    app.spawning_under = None;
    app.child_count = 2;

    // Trigger conflict detection - use the same task so branch names match
    let handler = Actions::new();
    handler.spawn_children(&mut app, task)?;

    // Verify we're in conflict mode with swarm info
    assert!(
        matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Expected Confirming(WorktreeConflict) mode, got {:?}",
        app.mode
    );
    assert_eq!(
        app.worktree_conflict.as_ref().unwrap().swarm_child_count,
        Some(2)
    );
    assert_eq!(
        app.worktree_conflict.as_ref().unwrap().prompt,
        Some(task.to_string())
    );

    // Modify the prompt before reconnecting (simulating user editing in ReconnectPrompt mode)
    let updated_task = "updated task for children";
    if let Some(ref mut conflict) = app.worktree_conflict {
        conflict.prompt = Some(updated_task.to_string());
    }

    // Now reconnect
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.reconnect_to_worktree(&mut app)?;

    // Should have created root + 2 children = 3 agents
    assert_eq!(app.storage.len(), 3, "Should have root + 2 children");

    // Find the root and children
    let root = app
        .storage
        .iter()
        .find(|a| a.is_root())
        .expect("Should have a root agent");
    let children: Vec<_> = app.storage.iter().filter(|a| !a.is_root()).collect();

    assert_eq!(children.len(), 2, "Should have 2 children");

    // Root should NOT have the prompt (root doesn't get the planning preamble)
    assert!(
        root.initial_prompt.is_none(),
        "Root should not have initial_prompt, got {:?}",
        root.initial_prompt
    );

    // Children SHOULD have the updated prompt (wrapped in planning preamble)
    for child in &children {
        let prompt = child
            .initial_prompt
            .as_ref()
            .expect("Child should have initial_prompt");
        assert!(
            prompt.contains(updated_task),
            "Child prompt should contain the updated task '{updated_task}'. Got: {prompt}"
        );
    }

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test recreating worktree for swarm
#[test]
fn test_worktree_conflict_recreate_swarm() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_recreate_swarm")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Use sleep command to keep sessions alive for swarm tests
    let mut config = fixture.config();
    config.default_program = "sleep 60".to_string();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // Create a worktree manually
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/swarm-recreate", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Add a marker file
    let marker_path = worktree_path.join("old_swarm_marker.txt");
    fs::write(&marker_path, "old swarm worktree")?;

    // Set up for swarm spawning
    app.spawning_under = None;
    app.child_count = 2;

    // Trigger conflict detection
    let handler = Actions::new();
    handler.spawn_children(&mut app, "swarm-recreate")?;

    // Verify we're in conflict mode
    assert!(matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ));

    // Now recreate
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.recreate_worktree(&mut app)?;

    // Should have created root + 2 children = 3 agents
    assert_eq!(app.storage.len(), 3, "Should have root + 2 children");

    // The old marker file should be gone
    assert!(
        !marker_path.exists(),
        "Old marker file should be gone after recreate"
    );

    // Verify we have correct structure
    let root_count = app.storage.iter().filter(|a| a.is_root()).count();
    let child_count = app.storage.iter().filter(|a| !a.is_root()).count();

    assert_eq!(root_count, 1, "Should have exactly 1 root");
    assert_eq!(child_count, 2, "Should have exactly 2 children");

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test that adding children to existing agent (A key) does NOT trigger conflict
/// (since it uses the parent's existing worktree)
#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
fn test_add_children_to_existing_no_conflict() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_add_children")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Use sleep command to keep sessions alive for child spawning tests
    let mut config = fixture.config();
    config.default_program = "sleep 60".to_string();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // First create a root agent normally
    let handler = Actions::new();
    handler.create_agent(&mut app, "parent-agent", None)?;

    assert_eq!(app.storage.len(), 1, "Should have parent agent");
    let parent_id = app.storage.iter().next().unwrap().id;

    // Now add children to the existing agent (A key flow)
    app.spawning_under = Some(parent_id);
    app.child_count = 2;

    let handler2 = Actions::new();
    handler2.spawn_children(&mut app, "child task")?;

    // Should NOT be in conflict mode - should have spawned directly
    assert!(
        !matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Adding children to existing agent should not trigger conflict"
    );

    // Should have parent + 2 children
    assert_eq!(app.storage.len(), 3, "Should have parent + 2 children");

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}
