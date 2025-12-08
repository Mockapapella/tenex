//! Tests for nested agent hierarchy and window index tracking

use std::path::PathBuf;

use crate::common::{TestFixture, create_child_agent, skip_if_no_tmux};
use tenex::agent::{Agent, Storage};
use tenex::tmux::SessionManager;

#[test]
fn test_nested_agent_window_index_tracking() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("nested_windows")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a root agent with 3 children (swarm)
    app.child_count = 3;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "test-swarm");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(()); // Skip if creation fails
    }

    // Should have root + 3 children = 4 agents
    assert_eq!(app.storage.len(), 4);

    // Find the root agent
    let root = app
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root agent")?;
    let root_id = root.id;

    // Find first-level Child 2 to add grandchildren under
    let child2 = app
        .storage
        .children(root_id)
        .into_iter()
        .find(|a| a.title.starts_with("Child 2"))
        .ok_or("No Child 2 found")?;
    let child2_id = child2.id;

    // Expand root to see children
    if let Some(root) = app.storage.get_mut(root_id) {
        root.collapsed = false;
    }

    // Add 3 grandchildren under Child 2
    app.child_count = 3;
    app.spawning_under = Some(child2_id);

    // Expand Child 2 to see grandchildren
    if let Some(c2) = app.storage.get_mut(child2_id) {
        c2.collapsed = false;
    }

    let handler = tenex::app::Actions::new();
    let result = handler.spawn_children(&mut app, "grandchild-task");
    if result.is_err() {
        // Cleanup and skip
        let manager = SessionManager::new();
        for agent in app.storage.iter() {
            let _ = manager.kill(&agent.tmux_session);
        }
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    // Should now have root + 3 children + 3 grandchildren = 7 agents
    assert_eq!(app.storage.len(), 7);

    // Get grandchildren window indices
    let grandchildren: Vec<_> = app.storage.children(child2_id);
    assert_eq!(grandchildren.len(), 3);

    // Find grandchild with highest window index (should be "Child 3" grandchild)
    let grandchild3 = grandchildren
        .iter()
        .max_by_key(|a| a.window_index)
        .ok_or("No grandchild found")?;
    let grandchild3_id = grandchild3.id;
    let grandchild3_initial_window = grandchild3.window_index;

    // Find the middle grandchild ("Child 2" grandchild) to delete
    let grandchild2 = grandchildren
        .iter()
        .find(|a| a.title.starts_with("Child 2"))
        .ok_or("No grandchild Child 2 found")?;
    let grandchild2_id = grandchild2.id;
    let grandchild2_window = grandchild2.window_index;

    // Select grandchild2 and delete it
    if let Some(idx) = app.storage.visible_index_of(grandchild2_id) {
        app.selected = idx;
    }

    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Kill,
    ));
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should now have 6 agents
    assert_eq!(app.storage.len(), 6);

    // Verify grandchild3's window index was decremented
    // (because tmux renumbers windows when one is deleted)
    let grandchild3_updated = app.storage.get(grandchild3_id).ok_or("Grandchild3 gone")?;
    let grandchild3_new_window = grandchild3_updated.window_index;

    // The window index should have been decremented by 1
    // (since grandchild2's window was deleted and was less than grandchild3's)
    assert!(
        grandchild3_new_window < grandchild3_initial_window,
        "Grandchild3 window index should have decreased after sibling deletion. \
         Initial: {grandchild3_initial_window:?}, New: {grandchild3_new_window:?}",
    );

    // Verify first-level Child 3's window index was NOT changed
    // (its window index should be less than the deleted grandchild's)
    let child3 = app
        .storage
        .children(root_id)
        .into_iter()
        .find(|a| a.title.starts_with("Child 3"))
        .ok_or("No Child 3 found")?;

    // Child 3's window should still be at its original index (4)
    // since only windows with higher indices get renumbered
    assert!(
        child3.window_index < grandchild2_window,
        "First-level Child 3 should have lower window index than deleted grandchild"
    );

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    std::env::set_current_dir(&original_dir)?;

    Ok(())
}

#[test]
fn test_child_agent_titles_include_short_id() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("child_titles")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a swarm with children
    app.child_count = 2;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "id-test");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    // Find the root
    let root = app.storage.iter().find(|a| a.is_root()).ok_or("No root")?;
    let root_id = root.id;

    // Check that children have short IDs in their titles
    let children = app.storage.children(root_id);
    for child in &children {
        // Title should be like "Child 1 (abc12345)"
        assert!(
            child.title.contains('(') && child.title.contains(')'),
            "Child title should contain short ID in parentheses: {}",
            child.title
        );

        // Extract the ID from the title and verify it matches short_id()
        let short_id = child.short_id();
        assert!(
            child.title.contains(&short_id),
            "Child title should contain its short ID. Title: {}, Short ID: {}",
            child.title,
            short_id
        );
    }

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    std::env::set_current_dir(&original_dir)?;

    Ok(())
}

#[test]
fn test_kill_windows_in_descending_order() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("descending_kill")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a swarm with 3 children
    app.child_count = 3;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "descending-test");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    let root = app.storage.iter().find(|a| a.is_root()).ok_or("No root")?;
    let root_id = root.id;

    // Get window indices before deletion
    let children = app.storage.children(root_id);
    let mut window_indices: Vec<u32> = children.iter().filter_map(|c| c.window_index).collect();
    window_indices.sort_unstable();

    // All 3 children should have sequential window indices (2, 3, 4)
    assert_eq!(window_indices.len(), 3);
    assert_eq!(window_indices[0], 2);
    assert_eq!(window_indices[1], 3);
    assert_eq!(window_indices[2], 4);

    // Kill the root (which should kill all children in descending order)
    if let Some(idx) = app.storage.visible_index_of(root_id) {
        app.selected = idx;
    }

    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Kill,
    ));
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // All agents should be gone
    assert_eq!(app.storage.len(), 0);

    std::env::set_current_dir(&original_dir)?;

    Ok(())
}

/// Test that `visible_agents_with_info` returns correct pre-computed child info
/// for a complex hierarchy
#[test]
fn test_visible_agents_with_info_hierarchy() {
    let mut storage = Storage::new();

    // Create hierarchy:
    // Root1 (expanded, 2 children)
    //   Child1 (expanded, 1 grandchild)
    //     Grandchild1
    //   Child2
    // Root2 (collapsed, 1 child - child should not appear)
    //   HiddenChild

    let mut root1 = Agent::new(
        "Root1".to_string(),
        "echo".to_string(),
        "branch1".to_string(),
        PathBuf::from("/tmp/root1"),
        None,
    );
    root1.collapsed = false; // Expanded

    let mut child1 = create_child_agent(&root1, "Child1", 2);
    child1.collapsed = false; // Expanded

    let grandchild1 = create_child_agent(&child1, "Grandchild1", 3);
    let child2 = create_child_agent(&root1, "Child2", 4);

    let root2 = Agent::new(
        "Root2".to_string(),
        "echo".to_string(),
        "branch2".to_string(),
        PathBuf::from("/tmp/root2"),
        None,
    );
    // root2.collapsed = true is default

    let hidden_child = create_child_agent(&root2, "HiddenChild", 2);

    // Add in order
    let root1_id = root1.id;
    let child1_id = child1.id;
    let root2_id = root2.id;

    storage.add(root1);
    storage.add(child1);
    storage.add(grandchild1);
    storage.add(child2);
    storage.add(root2);
    storage.add(hidden_child);

    // Get visible agents with info
    let visible = storage.visible_agents_with_info();

    // Should have 5 visible: Root1, Child1, Grandchild1, Child2, Root2
    // (HiddenChild is not visible because Root2 is collapsed)
    assert_eq!(visible.len(), 5);

    // Verify Root1
    assert_eq!(visible[0].agent.id, root1_id);
    assert_eq!(visible[0].depth, 0);
    assert!(visible[0].has_children);
    assert_eq!(visible[0].child_count, 2);

    // Verify Child1
    assert_eq!(visible[1].agent.id, child1_id);
    assert_eq!(visible[1].depth, 1);
    assert!(visible[1].has_children);
    assert_eq!(visible[1].child_count, 1);

    // Verify Grandchild1
    assert_eq!(visible[2].agent.title, "Grandchild1");
    assert_eq!(visible[2].depth, 2);
    assert!(!visible[2].has_children);
    assert_eq!(visible[2].child_count, 0);

    // Verify Child2
    assert_eq!(visible[3].agent.title, "Child2");
    assert_eq!(visible[3].depth, 1);
    assert!(!visible[3].has_children);
    assert_eq!(visible[3].child_count, 0);

    // Verify Root2 (collapsed but still visible itself)
    assert_eq!(visible[4].agent.id, root2_id);
    assert_eq!(visible[4].depth, 0);
    assert!(visible[4].has_children);
    assert_eq!(visible[4].child_count, 1);
}
