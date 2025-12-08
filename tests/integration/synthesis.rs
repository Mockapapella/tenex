//! Tests for synthesis functionality

use crate::common::{TestFixture, skip_if_no_tmux};
use tenex::tmux::SessionManager;

#[test]
fn test_synthesize_requires_children() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_no_children")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a single agent (no children)
    let result = handler.create_agent(&mut app, "solo-agent", None);
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    app.select_next();

    // Try to synthesize - should show error since no children
    let result = handler.handle_action(&mut app, tenex::config::Action::Synthesize);
    assert!(result.is_ok());

    // Should be in error modal mode
    assert!(matches!(app.mode, tenex::app::Mode::ErrorModal(_)));

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    std::env::set_current_dir(&original_dir)?;

    Ok(())
}

#[test]
fn test_synthesize_enters_confirmation_mode() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_confirm")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a swarm with children
    app.child_count = 2;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "synth-confirm-test");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    // Select root agent
    app.selected = 0;

    // Synthesize action should enter confirmation mode
    let result = handler.handle_action(&mut app, tenex::config::Action::Synthesize);
    assert!(result.is_ok());
    assert_eq!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::Synthesize)
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
fn test_synthesize_removes_all_descendants() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_descendants")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a swarm with 3 children
    app.child_count = 3;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "synth-desc-test");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    // Should have 4 agents (root + 3 children)
    assert_eq!(app.storage.len(), 4);

    // Find root and Child 2
    let root = app.storage.iter().find(|a| a.is_root()).ok_or("No root")?;
    let root_id = root.id;

    // Expand root to show children
    if let Some(root) = app.storage.get_mut(root_id) {
        root.collapsed = false;
    }

    let child2 = app
        .storage
        .children(root_id)
        .into_iter()
        .find(|a| a.title.starts_with("Child 2"))
        .ok_or("No Child 2")?;
    let child2_id = child2.id;

    // Add 2 grandchildren under Child 2
    app.child_count = 2;
    app.spawning_under = Some(child2_id);

    // Expand Child 2
    if let Some(c2) = app.storage.get_mut(child2_id) {
        c2.collapsed = false;
    }

    let handler = tenex::app::Actions::new();
    let result = handler.spawn_children(&mut app, "grandchild-task");
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.storage.iter() {
            let _ = manager.kill(&agent.tmux_session);
        }
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    // Should now have 6 agents (root + 3 children + 2 grandchildren)
    assert_eq!(app.storage.len(), 6);

    // Select root and synthesize
    app.selected = 0;

    // Enter confirmation mode and confirm
    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Synthesize,
    ));
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should only have root remaining (all 5 descendants removed)
    assert_eq!(app.storage.len(), 1);

    // Verify synthesis file was created
    let root = app.storage.iter().next().ok_or("Root gone")?;
    let tenex_dir = root.worktree_path.join(".tenex");
    assert!(tenex_dir.exists(), ".tenex directory should exist");

    // There should be a .md file in the directory
    let entries: Vec<_> = std::fs::read_dir(&tenex_dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert_eq!(entries.len(), 1, "Should have exactly one synthesis file");

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    std::env::set_current_dir(&original_dir)?;

    Ok(())
}

#[test]
fn test_synthesize_child_with_grandchildren() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_grandchild")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create a swarm with 2 children
    app.child_count = 2;
    app.spawning_under = None;
    let result = handler.spawn_children(&mut app, "synth-gc-test");
    if result.is_err() {
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    let root = app.storage.iter().find(|a| a.is_root()).ok_or("No root")?;
    let root_id = root.id;

    // Expand root
    if let Some(root) = app.storage.get_mut(root_id) {
        root.collapsed = false;
    }

    let child1 = app
        .storage
        .children(root_id)
        .into_iter()
        .find(|a| a.title.starts_with("Child 1"))
        .ok_or("No Child 1")?;
    let child1_id = child1.id;

    // Add 2 grandchildren under Child 1
    app.child_count = 2;
    app.spawning_under = Some(child1_id);

    if let Some(c1) = app.storage.get_mut(child1_id) {
        c1.collapsed = false;
    }

    let handler = tenex::app::Actions::new();
    let result = handler.spawn_children(&mut app, "gc-task");
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.storage.iter() {
            let _ = manager.kill(&agent.tmux_session);
        }
        std::env::set_current_dir(&original_dir)?;
        return Ok(());
    }

    // Should have 5 agents (root + 2 children + 2 grandchildren)
    assert_eq!(app.storage.len(), 5);

    // Select Child 1 (which has grandchildren) and synthesize just its children
    if let Some(idx) = app.storage.visible_index_of(child1_id) {
        app.selected = idx;
    }

    // Enter confirmation mode and confirm
    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Synthesize,
    ));
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should have 3 agents remaining (root + Child 1 + Child 2)
    // The 2 grandchildren under Child 1 should be removed
    assert_eq!(app.storage.len(), 3);

    // Root should still have 2 children
    assert_eq!(app.storage.children(root_id).len(), 2);

    // Child 1 should have no children now
    assert_eq!(app.storage.children(child1_id).len(), 0);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    std::env::set_current_dir(&original_dir)?;

    Ok(())
}
