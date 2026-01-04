//! Tests for synthesis functionality

use crate::common::{DirGuard, TestFixture, skip_if_no_mux};
use tenex::mux::SessionManager;

#[test]
fn test_synthesize_requires_children() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_no_children")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a single agent (no children)
    let Ok(next) = handler.create_agent(&mut app.data, "solo-agent", None) else {
        return Ok(());
    };
    app.apply_mode(next);

    app.select_next();

    // Try to synthesize - should show error since no children
    let result = handler.handle_action(&mut app, tenex::config::Action::Synthesize);
    assert!(result.is_ok());

    // Should be in error modal mode
    assert!(matches!(app.mode, tenex::AppMode::ErrorModal(_)));

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_synthesize_enters_confirmation_mode() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_confirm")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a swarm with children
    app.data.spawn.child_count = 2;
    app.data.spawn.spawning_under = None;
    let result = handler.spawn_children(&mut app.data, Some("synth-confirm-test"));
    if result.is_err() {
        return Ok(());
    }

    // Select root agent
    app.data.selected = 0;

    // Synthesize action should enter confirmation mode
    let result = handler.handle_action(&mut app, tenex::config::Action::Synthesize);
    assert!(result.is_ok());
    assert_eq!(
        app.mode,
        tenex::AppMode::Confirming(tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Synthesize,
        })
    );

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_synthesize_removes_all_descendants() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_descendants")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a swarm with 3 children
    app.data.spawn.child_count = 3;
    app.data.spawn.spawning_under = None;
    let result = handler.spawn_children(&mut app.data, Some("synth-desc-test"));
    if result.is_err() {
        return Ok(());
    }

    // Should have 4 agents (root + 3 children)
    assert_eq!(app.data.storage.len(), 4);

    // Find root and Child 2
    let root = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root")?;
    let root_id = root.id;

    // Expand root to show children
    if let Some(root) = app.data.storage.get_mut(root_id) {
        root.collapsed = false;
    }

    let child2 = app
        .data
        .storage
        .children(root_id)
        .into_iter()
        .find(|a| a.title.starts_with("Agent 2"))
        .ok_or("No Agent 2")?;
    let child2_id = child2.id;

    // Add 2 grandchildren under Child 2
    app.data.spawn.child_count = 2;
    app.data.spawn.spawning_under = Some(child2_id);

    // Expand Child 2
    if let Some(c2) = app.data.storage.get_mut(child2_id) {
        c2.collapsed = false;
    }

    let handler = tenex::app::Actions::new();
    let result = handler.spawn_children(&mut app.data, Some("grandchild-task"));
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.data.storage.iter() {
            let _ = manager.kill(&agent.mux_session);
        }
        return Ok(());
    }

    // Should now have 6 agents (root + 3 children + 2 grandchildren)
    assert_eq!(app.data.storage.len(), 6);

    // Select root and synthesize
    app.data.selected = 0;

    // Enter confirmation mode and confirm
    app.enter_mode(
        tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Synthesize,
        }
        .into(),
    );
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should only have root remaining (all 5 descendants removed)
    assert_eq!(app.data.storage.len(), 1);

    // Verify synthesis file was created
    let root = app.data.storage.iter().next().ok_or("Root gone")?;
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
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_synthesize_ignores_terminal_children() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_ignore_term")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a swarm with 2 children (non-terminal agents)
    app.data.spawn.child_count = 2;
    app.data.spawn.spawning_under = None;
    let result = handler.spawn_children(&mut app.data, Some("synth-term-test"));
    if result.is_err() {
        return Ok(());
    }

    // Should have 3 agents (root + 2 children)
    assert_eq!(app.data.storage.len(), 3);

    // Select root and spawn a terminal
    app.data.selected = 0;
    let handler = tenex::app::Actions::new();
    let result = handler.spawn_terminal(&mut app.data, None);
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.data.storage.iter() {
            let _ = manager.kill(&agent.mux_session);
        }
        return Ok(());
    }

    // Should now have 4 agents (root + 2 children + 1 terminal)
    assert_eq!(app.data.storage.len(), 4);

    // Verify we have exactly 1 terminal child
    let root_id = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root")?
        .id;
    let terminal_count = app
        .data
        .storage
        .children(root_id)
        .into_iter()
        .filter(|a| a.is_terminal)
        .count();
    assert_eq!(terminal_count, 1, "Should have exactly 1 terminal");

    // Select root and synthesize
    app.data.selected = 0;

    // Enter confirmation mode and confirm
    app.enter_mode(
        tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Synthesize,
        }
        .into(),
    );
    let handler = tenex::app::Actions::new();
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should have 2 agents remaining (root + terminal)
    // The 2 non-terminal children should be removed, terminal preserved
    assert_eq!(app.data.storage.len(), 2);

    // Verify the terminal is still there
    let remaining_children = app.data.storage.children(root_id);
    assert_eq!(remaining_children.len(), 1);
    assert!(
        remaining_children[0].is_terminal,
        "Terminal should be preserved"
    );

    // Verify synthesis file was created
    let root = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("Root gone")?;
    let tenex_dir = root.worktree_path.join(".tenex");
    assert!(tenex_dir.exists(), ".tenex directory should exist");

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_synthesize_only_terminals_shows_error() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_only_term")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a single agent (root)
    let Ok(next) = handler.create_agent(&mut app.data, "term-only-root", None) else {
        return Ok(());
    };
    app.apply_mode(next);

    app.select_next();

    // Spawn two terminals as children
    let handler = tenex::app::Actions::new();
    let result = handler.spawn_terminal(&mut app.data, None);
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.data.storage.iter() {
            let _ = manager.kill(&agent.mux_session);
        }
        return Ok(());
    }

    let handler = tenex::app::Actions::new();
    let result = handler.spawn_terminal(&mut app.data, None);
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.data.storage.iter() {
            let _ = manager.kill(&agent.mux_session);
        }
        return Ok(());
    }

    // Should have 3 agents (root + 2 terminals)
    assert_eq!(app.data.storage.len(), 3);

    // Verify all children are terminals
    let root_id = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root")?
        .id;
    let children = app.data.storage.children(root_id);
    assert_eq!(children.len(), 2);
    assert!(
        children.iter().all(|c| c.is_terminal),
        "All children should be terminals"
    );

    // Select root
    app.data.selected = 0;

    // has_children check passes, so we should enter confirmation mode
    let handler = tenex::app::Actions::new();
    let result = handler.handle_action(&mut app, tenex::config::Action::Synthesize);
    assert!(result.is_ok());
    assert_eq!(
        app.mode,
        tenex::AppMode::Confirming(tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Synthesize,
        })
    );

    // Now confirm - this should fail because all children are terminals
    let handler = tenex::app::Actions::new();
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should show error modal
    assert!(matches!(app.mode, tenex::AppMode::ErrorModal(_)));

    // All agents should still exist (nothing was removed)
    assert_eq!(app.data.storage.len(), 3);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_synthesize_child_with_grandchildren() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("synth_grandchild")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create a swarm with 2 children
    app.data.spawn.child_count = 2;
    app.data.spawn.spawning_under = None;
    let result = handler.spawn_children(&mut app.data, Some("synth-gc-test"));
    if result.is_err() {
        return Ok(());
    }

    let root = app
        .data
        .storage
        .iter()
        .find(|a| a.is_root())
        .ok_or("No root")?;
    let root_id = root.id;

    // Expand root
    if let Some(root) = app.data.storage.get_mut(root_id) {
        root.collapsed = false;
    }

    let child1 = app
        .data
        .storage
        .children(root_id)
        .into_iter()
        .find(|a| a.title.starts_with("Agent 1"))
        .ok_or("No Agent 1")?;
    let child1_id = child1.id;

    // Add 2 grandchildren under Agent 1
    app.data.spawn.child_count = 2;
    app.data.spawn.spawning_under = Some(child1_id);

    if let Some(c1) = app.data.storage.get_mut(child1_id) {
        c1.collapsed = false;
    }

    let handler = tenex::app::Actions::new();
    let result = handler.spawn_children(&mut app.data, Some("gc-task"));
    if result.is_err() {
        let manager = SessionManager::new();
        for agent in app.data.storage.iter() {
            let _ = manager.kill(&agent.mux_session);
        }
        return Ok(());
    }

    // Should have 5 agents (root + 2 children + 2 grandchildren)
    assert_eq!(app.data.storage.len(), 5);

    // Select Agent 1 (which has grandchildren) and synthesize just its children
    if let Some(idx) = app.data.storage.visible_index_of(child1_id) {
        app.data.selected = idx;
    }

    // Enter confirmation mode and confirm
    app.enter_mode(
        tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Synthesize,
        }
        .into(),
    );
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());

    // Should have 3 agents remaining (root + Agent 1 + Agent 2)
    // The 2 grandchildren under Agent 1 should be removed
    assert_eq!(app.data.storage.len(), 3);

    // Root should still have 2 children
    assert_eq!(app.data.storage.children(root_id).len(), 2);

    // Agent 1 should have no children now
    assert_eq!(app.data.storage.children(child1_id).len(), 0);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}
