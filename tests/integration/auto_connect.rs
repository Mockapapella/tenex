#![cfg(not(windows))]

//! Auto-connect to existing worktrees tests

use crate::common::{DirGuard, TestFixture, assert_paths_eq, skip_if_no_tmux};
use tenex::app::{Actions, App};

/// Test that `auto_connect_worktrees` picks up an existing worktree and creates an agent
#[test]
fn test_auto_connect_existing_worktree() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let _dir_guard = DirGuard::new()?;
    let fixture = TestFixture::new("auto_connect")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = Actions::new();

    // Verify no agents exist initially
    assert_eq!(app.storage.len(), 0, "Storage should be empty initially");

    // Create a worktree manually to simulate an existing worktree from a previous session
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/my-feature", fixture.session_prefix);
    let worktree_path = fixture.worktree_path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Verify the worktree exists
    assert!(worktree_mgr.exists(&branch_name), "Worktree should exist");

    // Call auto_connect_worktrees - this should detect the worktree and create an agent
    handler.auto_connect_worktrees(&mut app)?;

    // Verify an agent was created
    assert_eq!(
        app.storage.len(),
        1,
        "Should have created one agent for the existing worktree"
    );

    // Verify the agent has the correct properties
    let agent = app.storage.iter().next().ok_or("Should have an agent")?;
    assert_eq!(
        agent.branch, branch_name,
        "Agent branch should match the worktree branch"
    );
    assert_eq!(
        agent.title, branch_name,
        "Agent title should be the branch name"
    );
    assert_paths_eq(
        &agent.worktree_path,
        &worktree_path,
        "Agent worktree path should match",
    );

    // Cleanup (DirGuard will restore directory on drop)
    fixture.cleanup_sessions();
    fixture.cleanup_branches();

    Ok(())
}

/// Test that `auto_connect_worktrees` skips worktrees that already have agents
#[test]
fn test_auto_connect_skips_existing_agents() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let _dir_guard = DirGuard::new()?;
    let fixture = TestFixture::new("auto_connect_skip")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = Actions::new();

    // Create a worktree manually
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/existing-agent", fixture.session_prefix);
    let worktree_path = fixture.worktree_path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Create an agent for this worktree first (simulating it already being tracked)
    handler.create_agent(&mut app, "existing-agent", None)?;

    // Handle potential worktree conflict by reconnecting
    if matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ) {
        handler.reconnect_to_worktree(&mut app)?;
    }

    let initial_count = app.storage.len();
    assert!(initial_count > 0, "Should have at least one agent");

    // Call auto_connect_worktrees - should not create duplicates
    handler.auto_connect_worktrees(&mut app)?;

    // Verify no new agents were created
    assert_eq!(
        app.storage.len(),
        initial_count,
        "Should not create duplicate agents"
    );

    // Cleanup (DirGuard will restore directory on drop)
    fixture.cleanup_sessions();
    fixture.cleanup_branches();

    Ok(())
}

/// Test that `auto_connect_worktrees` skips worktrees with different branch prefix
#[test]
fn test_auto_connect_skips_different_prefix() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let _dir_guard = DirGuard::new()?;
    let fixture = TestFixture::new("auto_connect_prefix")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = Actions::new();

    // Create a worktree with a different prefix (not matching our config)
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "other-prefix/some-feature";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    // Call auto_connect_worktrees
    handler.auto_connect_worktrees(&mut app)?;

    // Verify no agents were created (wrong prefix)
    assert_eq!(
        app.storage.len(),
        0,
        "Should not create agents for worktrees with different prefix"
    );

    // Cleanup (DirGuard will restore directory on drop)
    fixture.cleanup_sessions();
    fixture.cleanup_branches();

    Ok(())
}

/// Test that `auto_connect_worktrees` handles multiple existing worktrees
#[test]
fn test_auto_connect_multiple_worktrees() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let _dir_guard = DirGuard::new()?;
    let fixture = TestFixture::new("auto_connect_multi")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = Actions::new();

    // Create multiple worktrees manually
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);

    let branch1 = format!("{}/feature-one", fixture.session_prefix);
    let path1 = fixture.worktree_path().join(&branch1);
    worktree_mgr.create_with_new_branch(&path1, &branch1)?;

    let branch2 = format!("{}/feature-two", fixture.session_prefix);
    let path2 = fixture.worktree_path().join(&branch2);
    worktree_mgr.create_with_new_branch(&path2, &branch2)?;

    let branch3 = format!("{}/feature-three", fixture.session_prefix);
    let path3 = fixture.worktree_path().join(&branch3);
    worktree_mgr.create_with_new_branch(&path3, &branch3)?;

    // Call auto_connect_worktrees
    handler.auto_connect_worktrees(&mut app)?;

    // Verify all three agents were created
    assert_eq!(
        app.storage.len(),
        3,
        "Should have created agents for all three worktrees"
    );

    // Verify each branch has a corresponding agent
    let branches: Vec<_> = app.storage.iter().map(|a| a.branch.clone()).collect();
    assert!(branches.contains(&branch1), "Should have agent for branch1");
    assert!(branches.contains(&branch2), "Should have agent for branch2");
    assert!(branches.contains(&branch3), "Should have agent for branch3");

    // Cleanup (DirGuard will restore directory on drop)
    fixture.cleanup_sessions();
    fixture.cleanup_branches();

    Ok(())
}

/// Regression test: Deleted agents should not reappear after restart
///
/// This test verifies the fix for a bug where:
/// 1. User creates an agent (creates worktree + adds to storage)
/// 2. User deletes the agent (removes from storage, should remove worktree)
/// 3. User restarts tenex (`auto_connect_worktrees` runs)
/// 4. BUG: Agent would reappear because worktree removal failed silently
///
/// The fix ensures worktree removal errors are handled properly with retries,
/// and errors are reported instead of silently ignored.
///
/// See: `src/git/worktree.rs:remove()` and `src/app/handlers/agent_lifecycle.rs:kill_agent()`
#[test]
fn test_deleted_agent_does_not_reappear_after_restart() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    // Guard that restores the current directory when dropped (even on panic)
    let _dir_guard = crate::common::DirGuard::new()?;

    let fixture = TestFixture::new("deleted_agent_restart")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(
        config.clone(),
        storage,
        tenex::app::Settings::default(),
        false,
    );
    let handler = Actions::new();

    // Step 1: Create an agent
    handler.create_agent(&mut app, "will-be-deleted", None)?;

    // Handle potential worktree conflict
    if matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ) {
        handler.reconnect_to_worktree(&mut app)?;
    }

    assert_eq!(app.storage.len(), 1, "Should have one agent after creation");

    let branch_name = app
        .storage
        .iter()
        .next()
        .map(|a| a.branch.clone())
        .ok_or("No agent found after creation")?;

    // Verify the worktree exists
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    assert!(
        worktree_mgr.exists(&branch_name),
        "Worktree should exist after agent creation"
    );

    // Step 2: Delete the agent (simulating kill_agent)
    // Select the agent first
    app.select_next();

    // Enter confirming mode and confirm the kill
    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Kill,
    ));
    handler.handle_action(&mut app, tenex::config::Action::Confirm)?;

    assert_eq!(app.storage.len(), 0, "Should have no agents after deletion");

    // Verify the worktree was removed
    // Re-open the repo to get fresh state
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    assert!(
        !worktree_mgr.exists(&branch_name),
        "Worktree should be removed after agent deletion"
    );

    // Step 3: Simulate restart by calling auto_connect_worktrees
    // This is what happens on tenex startup
    let storage2 = TestFixture::create_storage(); // Fresh empty storage (simulating restart)
    let mut app2 = App::new(config, storage2, tenex::app::Settings::default(), false);

    handler.auto_connect_worktrees(&mut app2)?;

    // Step 4: Verify the deleted agent does NOT reappear
    assert_eq!(
        app2.storage.len(),
        0,
        "Deleted agent should NOT reappear after restart - worktree should have been cleaned up"
    );

    // Cleanup (DirGuard will restore directory on drop)
    fixture.cleanup_sessions();
    fixture.cleanup_branches();

    Ok(())
}
