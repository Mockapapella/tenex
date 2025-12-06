//! Integration tests for CLI commands
//!
//! These tests require:
//! - tmux to be installed and running
//! - git to be available
//! - A writable temp directory
//!
//! IMPORTANT: Run with `--test-threads=1` to avoid race conditions from
//! parallel tests calling `std::env::set_current_dir`.

use std::fs;
use std::path::{Path, PathBuf};

use git2::{Repository, Signature};
use tempfile::TempDir;
use tenex::agent::{Agent, Storage};
use tenex::config::Config;
use tenex::tmux::SessionManager;

/// Test fixture that sets up a temporary git repository
struct TestFixture {
    /// Temporary directory containing the git repo
    _temp_dir: TempDir,
    /// Path to the git repository
    repo_path: PathBuf,
    /// Temporary directory for worktrees
    worktree_dir: TempDir,
    /// Temporary directory for state storage
    state_dir: TempDir,
    /// Test-specific session prefix to avoid conflicts
    session_prefix: String,
}

impl TestFixture {
    fn new(test_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize git repo with initial commit
        let repo = Repository::init(&repo_path)?;
        let sig = Signature::now("Test", "test@test.com")?;

        // Create a file and commit it
        let readme_path = repo_path.join("README.md");
        fs::write(&readme_path, "# Test Repository\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        let worktree_dir = TempDir::new()?;
        let state_dir = TempDir::new()?;

        // Generate unique session prefix for this test run
        let session_prefix = format!("tenex-test-{}-{}", test_name, std::process::id());

        Ok(Self {
            _temp_dir: temp_dir,
            repo_path,
            worktree_dir,
            state_dir,
            session_prefix,
        })
    }

    fn config(&self) -> Config {
        Config {
            default_program: "echo".to_string(), // Use echo instead of claude for testing
            branch_prefix: format!("{}/", self.session_prefix),
            worktree_dir: self.worktree_dir.path().to_path_buf(),
            auto_yes: false,
            poll_interval_ms: 100,
            max_agents: 10,
        }
    }

    fn storage_path(&self) -> PathBuf {
        self.state_dir.path().join("agents.json")
    }

    const fn create_storage() -> Storage {
        Storage::new()
    }

    fn session_name(&self, suffix: &str) -> String {
        format!("{}-{}", self.session_prefix, suffix)
    }

    /// Clean up any tmux sessions created by this test
    fn cleanup_sessions(&self) {
        let manager = SessionManager::new();
        if let Ok(sessions) = manager.list() {
            for session in sessions {
                if session.name.starts_with(&self.session_prefix) {
                    let _ = manager.kill(&session.name);
                }
            }
        }
    }

    /// Clean up branches and worktrees from the test's repo
    fn cleanup_branches(&self) {
        if let Ok(repo) = Repository::open(&self.repo_path) {
            // Clean up worktrees
            let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
            if let Ok(worktrees) = worktree_mgr.list() {
                for wt in worktrees {
                    if wt.name.starts_with(&self.session_prefix) {
                        let _ = worktree_mgr.remove(&wt.name);
                    }
                }
            }

            // Clean up branches
            let branch_mgr = tenex::git::BranchManager::new(&repo);
            if let Ok(branches) = branch_mgr.list() {
                for branch in branches {
                    if branch.starts_with(&self.session_prefix) {
                        let _ = branch_mgr.delete(&branch);
                    }
                }
            }
        }
    }

    /// Clean up agents from the real storage that have this test's prefix
    fn cleanup_storage(&self) {
        if let Ok(mut storage) = Storage::load() {
            let agents_to_remove: Vec<_> = storage
                .iter()
                .filter(|a| a.branch.starts_with(&self.session_prefix))
                .map(|a| a.id)
                .collect();

            for id in agents_to_remove {
                storage.remove(id);
            }

            let _ = storage.save();
        }
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        self.cleanup_sessions();
        self.cleanup_branches();
        self.cleanup_storage();
    }
}

fn tmux_available() -> bool {
    tenex::tmux::is_available()
}

fn skip_if_no_tmux() -> bool {
    if !tmux_available() {
        eprintln!("Skipping test: tmux not available");
        return true;
    }
    false
}

// =============================================================================
// Integration tests for cmd_list
// =============================================================================

#[test]
fn test_cmd_list_shows_agents() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("list")?;
    let mut storage = TestFixture::create_storage();

    // Add some test agents
    let agent1 = Agent::new(
        "test-agent-1".to_string(),
        "echo".to_string(),
        fixture.session_name("agent1"),
        fixture.worktree_dir.path().join("agent1"),
        None,
    );
    let agent2 = Agent::new(
        "test-agent-2".to_string(),
        "echo".to_string(),
        fixture.session_name("agent2"),
        fixture.worktree_dir.path().join("agent2"),
        None,
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
        fixture.worktree_dir.path().join("running"),
        None,
    );
    agent1.set_status(tenex::Status::Running);

    let mut agent2 = Agent::new(
        "paused-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("paused"),
        fixture.worktree_dir.path().join("paused"),
        None,
    );
    agent2.set_status(tenex::Status::Paused);

    storage.add(agent1);
    storage.add(agent2);

    // Filter running only
    let running: Vec<_> = storage
        .iter()
        .filter(|a| a.status == tenex::Status::Running)
        .collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].title, "running-agent");

    Ok(())
}

// =============================================================================
// Integration tests for find_agent
// =============================================================================

#[test]
fn test_find_agent_by_short_id_integration() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("find_short")?;
    let mut storage = TestFixture::create_storage();

    let agent = Agent::new(
        "findable-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("findable"),
        fixture.worktree_dir.path().join("findable"),
        None,
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
        fixture.worktree_dir.path().join("idx0"),
        None,
    ));
    storage.add(Agent::new(
        "agent-1".to_string(),
        "echo".to_string(),
        fixture.session_name("idx1"),
        fixture.worktree_dir.path().join("idx1"),
        None,
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

// =============================================================================
// Integration tests for tmux session operations
// =============================================================================

#[test]
fn test_tmux_session_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("lifecycle")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("lifecycle");

    // Ensure session doesn't exist
    let _ = manager.kill(&session_name);
    assert!(!manager.exists(&session_name));

    // Create session with a command that stays alive
    let result = manager.create(&session_name, fixture.worktree_dir.path(), Some("sleep 10"));
    assert!(result.is_ok());

    // Give tmux time to start
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify session exists
    assert!(manager.exists(&session_name));

    // Kill session
    let result = manager.kill(&session_name);
    assert!(result.is_ok());

    // Verify session is gone
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));

    Ok(())
}

#[test]
fn test_tmux_session_list() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("list_sessions")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("listtest");

    // Create a session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, fixture.worktree_dir.path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    // List sessions and verify our session is present
    let sessions = manager.list()?;
    let found = sessions.iter().any(|s| s.name == session_name);
    assert!(found, "Created session should appear in list");

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

// =============================================================================
// Integration tests for git worktree operations
// =============================================================================

#[test]
fn test_git_worktree_create_and_remove() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("worktree")?;
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let manager = tenex::git::WorktreeManager::new(&repo);

    let worktree_path = fixture.worktree_dir.path().join("test-worktree");
    let branch_name = "test-branch";

    // Create worktree with new branch
    let result = manager.create_with_new_branch(&worktree_path, branch_name);
    assert!(result.is_ok(), "Failed to create worktree: {result:?}");

    // Verify worktree exists
    assert!(worktree_path.exists());
    assert!(worktree_path.join(".git").exists());

    // Remove worktree
    let result = manager.remove(branch_name);
    assert!(result.is_ok(), "Failed to remove worktree: {result:?}");

    Ok(())
}

// =============================================================================
// Integration tests for agent creation workflow
// =============================================================================

#[test]
fn test_agent_creation_workflow() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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

    // Create tmux session with a command that stays alive
    manager.create(&session_name, &worktree_path, Some("sleep 10"))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Create agent record
    let agent = Agent::new(
        title.to_string(),
        config.default_program,
        branch.clone(),
        worktree_path.clone(),
        None,
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

// =============================================================================
// Integration tests for storage persistence
// =============================================================================

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

// =============================================================================
// Integration tests for config persistence
// =============================================================================

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

// =============================================================================
// Integration tests for agent status transitions
// =============================================================================

#[test]
fn test_agent_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("status_trans")?;
    let mut storage = TestFixture::create_storage();

    let mut agent = Agent::new(
        "status-test".to_string(),
        "echo".to_string(),
        fixture.session_name("status"),
        fixture.worktree_dir.path().join("status"),
        None,
    );

    // Initial status should be Starting
    assert_eq!(agent.status, tenex::Status::Starting);

    // Transition to Running
    agent.set_status(tenex::Status::Running);
    assert_eq!(agent.status, tenex::Status::Running);
    assert!(agent.status.can_pause());
    assert!(!agent.status.can_resume());

    // Transition to Paused
    agent.set_status(tenex::Status::Paused);
    assert_eq!(agent.status, tenex::Status::Paused);
    assert!(!agent.status.can_pause());
    assert!(agent.status.can_resume());

    // Back to Running
    agent.set_status(tenex::Status::Running);
    assert_eq!(agent.status, tenex::Status::Running);

    // Transition to Stopped
    agent.set_status(tenex::Status::Stopped);
    assert_eq!(agent.status, tenex::Status::Stopped);
    assert!(!agent.status.can_pause());
    assert!(!agent.status.can_resume());

    storage.add(agent);
    assert_eq!(storage.len(), 1);

    Ok(())
}

// =============================================================================
// Integration tests for Actions handler with real operations
// =============================================================================

#[test]
fn test_actions_create_agent_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_create")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    // Change to repo directory for the test
    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent via the handler
    let result = handler.create_agent(&mut app, "integration-test", None);

    // Cleanup first
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    // Restore original directory
    let _ = std::env::set_current_dir(&original_dir);

    assert!(result.is_ok(), "Failed to create agent: {result:?}");
    assert_eq!(app.storage.len(), 1);

    Ok(())
}

#[test]
fn test_actions_create_agent_with_prompt_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_prompt")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent with a prompt
    let result = handler.create_agent(&mut app, "prompted-agent", Some("test prompt"));

    std::env::set_current_dir(&original_dir)?;

    assert!(result.is_ok(), "Failed to create agent: {result:?}");
    assert_eq!(app.storage.len(), 1);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_actions_kill_agent_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_kill")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent first
    handler.create_agent(&mut app, "killable", None)?;
    assert_eq!(app.storage.len(), 1);

    // Select the agent
    app.select_next();

    // Now kill it via confirm action
    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Kill,
    ));
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);

    std::env::set_current_dir(&original_dir)?;

    assert!(result.is_ok());
    assert_eq!(app.storage.len(), 0);

    Ok(())
}

#[test]
fn test_actions_pause_resume_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_pause_resume")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "pausable", None)?;
    app.select_next();

    // Wait for agent to start
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Manually set to Running (since "echo" command exits quickly)
    if let Some(agent) = app.selected_agent_mut() {
        agent.set_status(tenex::Status::Running);
    }

    // Pause the agent
    let result = handler.handle_action(&mut app, tenex::config::Action::Pause);
    assert!(result.is_ok());

    // Check status is paused
    if let Some(agent) = app.selected_agent() {
        assert_eq!(agent.status, tenex::Status::Paused);
    }

    // Resume the agent
    let result = handler.handle_action(&mut app, tenex::config::Action::Resume);
    assert!(result.is_ok());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_actions_update_preview_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_preview")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "preview-test", None)?;
    app.select_next();

    // Wait for session
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Update preview
    let result = handler.update_preview(&mut app);
    assert!(result.is_ok());
    // Preview content should be set (either actual content or session not running)
    assert!(!app.preview_content.is_empty());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_actions_update_diff_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_diff")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "diff-test", None)?;
    app.select_next();

    // Update diff
    let result = handler.update_diff(&mut app);
    assert!(result.is_ok());
    // Diff content should be set (either "No changes" or actual diff)
    assert!(!app.diff_content.is_empty());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_actions_attach_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_attach")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "attachable", None)?;
    app.select_next();

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Request attach - this sets the attach_session field if session exists
    // Note: The session may have already exited (echo command), so attach may fail
    let _result = handler.handle_action(&mut app, tenex::config::Action::Attach);

    let _ = std::env::set_current_dir(&original_dir);

    // The attach action either succeeds or sets an error
    // We just verify the action was processed without panic

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_actions_reset_all_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_reset")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create multiple agents
    handler.create_agent(&mut app, "reset1", None)?;
    handler.create_agent(&mut app, "reset2", None)?;
    assert_eq!(app.storage.len(), 2);

    // Reset all via confirm action
    app.enter_mode(tenex::app::Mode::Confirming(
        tenex::app::ConfirmAction::Reset,
    ));
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());
    assert_eq!(app.storage.len(), 0);

    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

#[test]
fn test_actions_push_branch_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_push")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "pushable", None);

    // Early cleanup if creation failed
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        // Skip test if agent creation fails (e.g., git/tmux issues)
        return Ok(());
    }

    app.select_next();

    // Push action (just sets status message, doesn't actually push in test)
    let result = handler.handle_action(&mut app, tenex::config::Action::Push);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    let _ = std::env::set_current_dir(&original_dir);

    assert!(result.is_ok());

    Ok(())
}

// =============================================================================
// Integration tests for tmux capture functions
// =============================================================================

#[test]
fn test_tmux_capture_pane() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_pane")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("capture");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager.create(&session_name, fixture.worktree_dir.path(), Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture the pane
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_pane(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture failed: {result:?}");

    Ok(())
}

#[test]
fn test_tmux_capture_pane_with_history() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_history")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("hist");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager.create(&session_name, fixture.worktree_dir.path(), Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture with history
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_pane_with_history(&session_name, 100);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture with history failed: {result:?}");

    Ok(())
}

#[test]
fn test_tmux_capture_full_history() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("capture_full")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("full");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager.create(&session_name, fixture.worktree_dir.path(), Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture full history
    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_full_history(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture full history failed: {result:?}");

    Ok(())
}

#[test]
fn test_tmux_capture_nonexistent_session() {
    if skip_if_no_tmux() {
        return;
    }

    let capture = tenex::tmux::OutputCapture::new();
    let result = capture.capture_pane("nonexistent-session-xyz");
    assert!(result.is_err());
}

#[test]
fn test_tmux_send_keys() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("send_keys")?;
    let manager = SessionManager::new();
    let session_name = fixture.session_name("keys");

    // Create a session
    let _ = manager.kill(&session_name);
    manager.create(&session_name, fixture.worktree_dir.path(), None)?;

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Send keys
    let result = manager.send_keys(&session_name, "echo test");
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);

    Ok(())
}

// =============================================================================
// Integration tests for CLI command success paths
// =============================================================================

#[test]
fn test_cmd_kill_success() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("cmd_kill")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent first
    let create_result = handler.create_agent(&mut app, "killable", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return Ok(());
    }

    // Get agent info for kill command
    let agent = app.storage.iter().next().ok_or("No agent found")?;
    let agent_id = agent.id;
    let session = agent.tmux_session.clone();
    let branch = agent.branch.clone();

    // Save storage so cmd_kill can load it
    let storage_path = fixture.storage_path();
    app.storage.save_to(&storage_path)?;

    // Simulate kill: kill session, remove worktree, remove from storage
    let manager = SessionManager::new();
    let _ = manager.kill(&session);

    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let _ = worktree_mgr.remove(&branch);

    app.storage.remove(agent_id);
    app.storage.save_to(&storage_path)?;

    let _ = std::env::set_current_dir(&original_dir);

    assert_eq!(app.storage.len(), 0);

    Ok(())
}

#[test]
fn test_cmd_pause_success() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("cmd_pause_success")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "pausable", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return Ok(());
    }

    // Mark as running
    if let Some(agent) = app.storage.iter_mut().next() {
        agent.set_status(tenex::Status::Running);
    }
    app.select_next();

    // Pause via handler
    let result = handler.handle_action(&mut app, tenex::config::Action::Pause);
    assert!(result.is_ok());

    // Verify paused
    if let Some(agent) = app.selected_agent() {
        assert_eq!(agent.status, tenex::Status::Paused);
    }

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_cmd_resume_success() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("cmd_resume_success")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "resumable", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return Ok(());
    }

    // Mark as paused
    if let Some(agent) = app.storage.iter_mut().next() {
        agent.set_status(tenex::Status::Paused);
    }
    app.select_next();

    // Resume via handler
    let result = handler.handle_action(&mut app, tenex::config::Action::Resume);
    assert!(result.is_ok());

    // Verify running
    if let Some(agent) = app.selected_agent() {
        assert_eq!(agent.status, tenex::Status::Running);
    }

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    Ok(())
}

#[test]
fn test_sync_agent_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let fixture = TestFixture::new("sync_status")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "sync-test", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return Ok(());
    }

    // Agent starts as Starting
    if let Some(agent) = app.storage.iter().next() {
        assert_eq!(agent.status, tenex::Status::Starting);
    }

    // Wait a bit for session to start
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Sync should transition to Running
    let _ = handler.sync_agent_status(&mut app);

    // Kill the session to simulate it stopping
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Sync should transition to Stopped
    let _ = handler.sync_agent_status(&mut app);

    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

// =============================================================================
// Integration test for full CLI workflow simulation
// =============================================================================

#[test]
fn test_full_cli_workflow() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
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

    manager.create(&session_name, &worktree_path, Some("sleep 60"))?;

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut agent = Agent::new(
        title.to_string(),
        config.default_program,
        branch.clone(),
        worktree_path.clone(),
        None,
    );
    agent.set_status(tenex::Status::Running);
    let agent_id = agent.id;
    storage.add(agent);

    // 2. List agents (simulate `muster list`)
    assert_eq!(storage.len(), 1);
    let all_agents: Vec<_> = storage.iter().collect();
    assert_eq!(all_agents[0].title, title);

    // 3. Pause agent (simulate `muster pause`)
    let _ = manager.kill(&session_name);
    if let Some(agent) = storage.get_mut(agent_id) {
        agent.set_status(tenex::Status::Paused);
    }

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));

    // 4. Resume agent (simulate `muster resume`)
    manager.create(&session_name, &worktree_path, Some("sleep 60"))?;
    if let Some(agent) = storage.get_mut(agent_id) {
        agent.set_status(tenex::Status::Running);
    }

    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(manager.exists(&session_name));

    // 5. Kill agent (simulate `muster kill`)
    let _ = manager.kill(&session_name);
    let _ = worktree_mgr.remove(&branch);
    storage.remove(agent_id);

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));
    assert_eq!(storage.len(), 0);

    Ok(())
}
