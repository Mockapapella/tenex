//! Tests for agent operations: listing, finding, status transitions

use crate::common::TestFixture;
use tenex::Status;
use tenex::agent::Agent;

#[test]
fn test_cmd_list_shows_agents() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("list")?;
    let mut storage = TestFixture::create_storage();

    // Add some test agents
    let agent1 = Agent::new(
        "test-agent-1".to_string(),
        "echo".to_string(),
        fixture.session_name("agent1"),
        fixture.worktree_path().join("agent1"),
    );
    let agent2 = Agent::new(
        "test-agent-2".to_string(),
        "echo".to_string(),
        fixture.session_name("agent2"),
        fixture.worktree_path().join("agent2"),
    );

    storage.add(agent1);
    storage.add(agent2);

    // Verify agents are in storage
    assert_eq!(storage.len(), 2);

    // The cmd_list function just prints, so we verify storage state
    assert_eq!(storage.iter().count(), 2);

    Ok(())
}

#[test]
fn test_cmd_list_filter_running() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("list_filter")?;
    let mut storage = TestFixture::create_storage();

    let mut agent1 = Agent::new(
        "running-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("running"),
        fixture.worktree_path().join("running"),
    );
    agent1.set_status(Status::Running);

    let mut agent2 = Agent::new(
        "starting-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("starting"),
        fixture.worktree_path().join("starting"),
    );
    agent2.set_status(Status::Starting);

    storage.add(agent1);
    storage.add(agent2);

    // Filter running only
    let running: Vec<_> = storage
        .iter()
        .filter(|a| a.status == Status::Running)
        .collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].title, "running-agent");

    Ok(())
}

#[test]
fn test_find_agent_by_short_id_integration() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("find_short")?;
    let mut storage = TestFixture::create_storage();

    let agent = Agent::new(
        "findable-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("findable"),
        fixture.worktree_path().join("findable"),
    );
    let short_id = agent.short_id();
    let full_id = agent.id;
    storage.add(agent);

    // Find by short ID
    let found = storage.find_by_short_id(&short_id);
    assert!(found.is_some());
    assert_eq!(found.ok_or("Agent not found")?.id, full_id);

    Ok(())
}

#[test]
fn test_find_agent_by_index_integration() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("find_index")?;
    let mut storage = TestFixture::create_storage();

    storage.add(Agent::new(
        "agent-0".to_string(),
        "echo".to_string(),
        fixture.session_name("idx0"),
        fixture.worktree_path().join("idx0"),
    ));
    storage.add(Agent::new(
        "agent-1".to_string(),
        "echo".to_string(),
        fixture.session_name("idx1"),
        fixture.worktree_path().join("idx1"),
    ));

    // Find by index
    let found0 = storage.get_by_index(0);
    let found1 = storage.get_by_index(1);

    assert!(found0.is_some());
    assert!(found1.is_some());
    assert_ne!(
        found0.ok_or("Agent 0 not found")?.id,
        found1.ok_or("Agent 1 not found")?.id
    );

    Ok(())
}

#[test]
fn test_agent_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("status_trans")?;
    let mut storage = TestFixture::create_storage();

    let mut agent = Agent::new(
        "status-test".to_string(),
        "echo".to_string(),
        fixture.session_name("status"),
        fixture.worktree_path().join("status"),
    );

    // Initial status should be Starting
    assert_eq!(agent.status, Status::Starting);

    // Transition to Running
    agent.set_status(Status::Running);
    assert_eq!(agent.status, Status::Running);

    storage.add(agent);
    assert_eq!(storage.len(), 1);

    Ok(())
}
