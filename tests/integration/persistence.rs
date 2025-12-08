//! Tests for storage and config persistence

use crate::common::TestFixture;
use tenex::agent::{Agent, Storage};
use tenex::config::Config;

#[test]
fn test_storage_save_and_load() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("storage_persist")?;
    let storage_path = fixture.storage_path();

    // Create storage with agents
    let mut storage = TestFixture::create_storage();
    storage.add(Agent::new(
        "persistent-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("persist"),
        fixture.worktree_dir.path().join("persist"),
        None,
    ));

    // Save to file
    storage.save_to(&storage_path)?;

    // Verify file exists
    assert!(storage_path.exists());

    // Load from file
    let loaded = Storage::load_from(&storage_path)?;
    assert_eq!(loaded.len(), 1);

    let agent = loaded.iter().next().ok_or("No agent found in storage")?;
    assert_eq!(agent.title, "persistent-agent");

    Ok(())
}

#[test]
fn test_config_save_and_load() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("config_persist")?;
    let config_path = fixture.state_dir.path().join("config.json");

    let config = Config {
        default_program: "custom-program".to_string(),
        max_agents: 20,
        ..Config::default()
    };

    // Save config
    config.save_to(&config_path)?;

    // Load config
    let loaded = Config::load_from(&config_path)?;
    assert_eq!(loaded.default_program, "custom-program");
    assert_eq!(loaded.max_agents, 20);

    Ok(())
}
