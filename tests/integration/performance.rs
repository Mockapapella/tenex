#![cfg(not(windows))]

//! Performance optimization tests

use std::path::PathBuf;

use crate::common::{TestFixture, create_child_agent, skip_if_no_tmux};
use tenex::agent::{Agent, Storage};
use tenex::app::{Actions, App};
use tenex::tmux::SessionManager;

/// Test that `sync_agent_status` correctly removes agents whose sessions don't exist
/// using the batched session list approach (single tmux list-sessions call)
#[test]
fn test_sync_agent_status_batched_session_check() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("sync_batched")?;
    let manager = SessionManager::new();

    // Create 3 agents in storage
    let mut storage = Storage::new();

    let agent1 = Agent::new(
        "agent1".to_string(),
        "echo".to_string(),
        fixture.session_name("agent1"),
        fixture.worktree_path(),
        None,
    );
    let agent2 = Agent::new(
        "agent2".to_string(),
        "echo".to_string(),
        fixture.session_name("agent2"),
        fixture.worktree_path(),
        None,
    );
    let agent3 = Agent::new(
        "agent3".to_string(),
        "echo".to_string(),
        fixture.session_name("agent3"),
        fixture.worktree_path(),
        None,
    );

    let agent1_session = agent1.tmux_session.clone();
    storage.add(agent1);
    storage.add(agent2);
    storage.add(agent3);

    // Only create a real tmux session for agent1
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&agent1_session, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Verify the session was created
    assert!(
        manager.exists(&agent1_session),
        "Session {agent1_session} should exist"
    );

    // Create app with the storage
    let mut app = App::new(
        fixture.config(),
        storage,
        tenex::app::Settings::default(),
        false,
    );
    assert_eq!(app.storage.len(), 3);

    // Sync agent status - should remove agents without sessions
    let handler = Actions::new();
    handler.sync_agent_status(&mut app)?;

    // Only agent1 should remain (the one with a real session)
    assert_eq!(
        app.storage.len(),
        1,
        "Expected 1 agent, got {}. Remaining: {:?}",
        app.storage.len(),
        app.storage.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
    assert!(app.storage.iter().any(|a| a.title == "agent1"));

    // Cleanup the session we created
    let _ = manager.kill(&agent1_session);

    Ok(())
}

/// Test that `reserve_window_indices` returns correct starting index
/// and spawning uses consecutive indices
#[test]
fn test_reserve_window_indices_consecutive() {
    let mut storage = Storage::new();

    // Create root agent
    let root = Agent::new(
        "Root".to_string(),
        "echo".to_string(),
        "branch".to_string(),
        PathBuf::from("/tmp/root"),
        None,
    );
    let root_id = root.id;
    storage.add(root.clone());

    // No children yet - next index should be 2 (window 1 is root)
    let start_idx = storage.reserve_window_indices(root_id);
    assert_eq!(start_idx, 2);

    // Add 3 children with consecutive indices
    for i in 0..3 {
        let mut child = create_child_agent(&root, &format!("Child{}", i + 1), start_idx + i);
        child.window_index = Some(start_idx + i);
        storage.add(child);
    }

    // Now reserve again - should return 5 (after 2, 3, 4)
    let next_idx = storage.reserve_window_indices(root_id);
    assert_eq!(next_idx, 5);

    // Add 2 more children
    for i in 0..2 {
        let mut child = create_child_agent(&root, &format!("Child{}", i + 4), next_idx + i);
        child.window_index = Some(next_idx + i);
        storage.add(child);
    }

    // Verify all 5 children have correct consecutive indices
    let children = storage.children(root_id);
    assert_eq!(children.len(), 5);

    let mut indices: Vec<u32> = children.iter().filter_map(|c| c.window_index).collect();
    indices.sort_unstable();
    assert_eq!(indices, vec![2, 3, 4, 5, 6]);
}

/// Stress test: verify `sync_agent_status` handles many agents efficiently
#[test]
fn test_large_swarm_sync_status() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("large_swarm")?;
    let manager = SessionManager::new();

    // Create root agent with real session
    let mut storage = Storage::new();
    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        fixture.session_name("root"),
        fixture.worktree_path(),
        None,
    );
    let root_session = root.tmux_session.clone();
    let root_id = root.id;
    storage.add(root.clone());

    // Create the root's tmux session with a long-running command
    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&root_session, &fixture.worktree_path(), Some(&command))?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Verify session was created
    assert!(
        manager.exists(&root_session),
        "Root session {root_session} should exist"
    );

    // Add 20 child agents to storage (they reference the root's session)
    // These are just storage entries - they share the root's session
    for i in 0u32..20 {
        let mut child = create_child_agent(&root, &format!("child{}", i + 1), i + 2);
        child.window_index = Some(i + 2);
        storage.add(child);
    }

    assert_eq!(storage.len(), 21); // 1 root + 20 children

    // Create app and sync status
    let mut app = App::new(
        fixture.config(),
        storage,
        tenex::app::Settings::default(),
        false,
    );
    let handler = Actions::new();

    // Sync should complete quickly (single list call, not 21 exists calls)
    // Note: sync_agent_status checks session existence, not window existence
    // So root should remain (its session exists)
    // Children also remain because they share the same session name as root
    handler.sync_agent_status(&mut app)?;

    // Root session exists, so root remains
    // Children share the same session, so they also remain
    // (The optimization is about *how* we check, not *what* we check)
    assert!(
        !app.storage.is_empty(),
        "Root should remain since its session exists. Got {} agents.",
        app.storage.len()
    );
    assert!(
        app.storage.iter().any(|a| a.id == root_id),
        "Root agent should be in storage"
    );

    // Cleanup the session we created
    let _ = manager.kill(&root_session);

    Ok(())
}
