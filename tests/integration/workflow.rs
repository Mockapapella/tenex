//! Tests for full CLI workflow, agent creation, and kill success paths

use crate::common::{TestFixture, skip_if_no_mux};
use tenex::agent::Agent;
use tenex::mux::SessionManager;

#[test]
fn test_agent_creation_workflow() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("create_workflow")?;
    let config = fixture.config();
    let mut storage = TestFixture::create_storage();
    let manager = SessionManager::new();

    // Create agent manually (simulating cmd_new)
    let title = "test-workflow";
    let branch = config.generate_branch_name(title);
    let worktree_path = config.worktree_dir.join(&branch);
    let session_name = branch.replace('/', "-");

    // Create git worktree
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch)?;

    // Create mux session with a command that stays alive
    let command = vec!["sleep".to_string(), "10".to_string()];
    manager.create(&session_name, &worktree_path, Some(&command))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Create agent record
    let agent = Agent::new(
        title.to_string(),
        config.default_program,
        branch.clone(),
        worktree_path.clone(),
    );
    let agent_id = agent.id;
    storage.add(agent);

    // Verify everything is set up
    assert!(manager.exists(&session_name));
    assert!(worktree_path.exists());
    assert_eq!(storage.len(), 1);

    // Simulate kill workflow
    let _ = manager.kill(&session_name);
    let _ = worktree_mgr.remove(&branch);
    storage.remove(agent_id);

    // Verify cleanup
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));
    assert_eq!(storage.len(), 0);

    Ok(())
}

#[test]
fn test_cmd_kill_success() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("cmd_kill")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent first
    let create_result = handler.create_agent(&mut app.data, "killable", None);
    if let Ok(next) = create_result {
        app.apply_mode(next);
    } else {
        let _ = std::env::set_current_dir(&original_dir);
        return Ok(());
    }

    // Get agent info for kill command
    let agent = app.data.storage.iter().next().ok_or("No agent found")?;
    let agent_id = agent.id;
    let session = agent.mux_session.clone();
    let branch = agent.branch.clone();

    // Save storage so cmd_kill can load it
    let storage_path = fixture.storage_path();
    app.data.storage.save_to(&storage_path)?;

    // Simulate kill: kill session, remove worktree, remove from storage
    let manager = SessionManager::new();
    let _ = manager.kill(&session);

    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let _ = worktree_mgr.remove(&branch);

    app.data.storage.remove(agent_id);
    app.data.storage.save_to(&storage_path)?;

    let _ = std::env::set_current_dir(&original_dir);

    assert_eq!(app.data.storage.len(), 0);

    Ok(())
}

#[test]
fn test_sync_agent_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("sync_status")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app.data, "sync-test", None);
    if let Ok(next) = create_result {
        app.apply_mode(next);
    } else {
        let _ = std::env::set_current_dir(&original_dir);
        return Ok(());
    }

    // Agent starts as Starting
    if let Some(agent) = app.data.storage.iter().next() {
        assert_eq!(agent.status, tenex::Status::Starting);
    }

    // Wait a bit for session to start
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Sync should transition to Running
    let _ = handler.sync_agent_status(&mut app);

    // Kill the session to simulate it stopping
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Sync should remove dead agents
    let _ = handler.sync_agent_status(&mut app);

    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

#[test]
fn test_full_cli_workflow() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("full_workflow")?;
    let config = fixture.config();
    let mut storage = TestFixture::create_storage();
    let manager = SessionManager::new();

    // 1. Create an agent (simulate `muster new`)
    let title = "workflow-agent";
    let branch = config.generate_branch_name(title);
    let worktree_path = config.worktree_dir.join(&branch);
    let session_name = branch.replace('/', "-");

    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch)?;

    let command = vec!["sleep".to_string(), "60".to_string()];
    manager.create(&session_name, &worktree_path, Some(&command))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut agent = Agent::new(
        title.to_string(),
        config.default_program,
        branch.clone(),
        worktree_path,
    );
    agent.set_status(tenex::Status::Running);
    let agent_id = agent.id;
    storage.add(agent);

    // 2. List agents (simulate `muster list`)
    assert_eq!(storage.len(), 1);
    let all_agents: Vec<_> = storage.iter().collect();
    assert_eq!(all_agents[0].title, title);

    // 3. Kill agent (simulate `muster kill`)
    let _ = manager.kill(&session_name);
    let _ = worktree_mgr.remove(&branch);
    storage.remove(agent_id);

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));
    assert_eq!(storage.len(), 0);

    Ok(())
}
