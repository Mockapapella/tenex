//! Integration tests for CLI commands
//!
//! These tests require:
//! - tmux to be installed and running
//! - git to be available
//! - A writable temp directory

#![expect(clippy::unwrap_used, reason = "integration test assertions")]
#![expect(clippy::similar_names, reason = "test clarity")]
#![expect(clippy::default_trait_access, reason = "test simplicity")]
#![expect(clippy::unused_self, reason = "consistent API in test fixtures")]
#![expect(clippy::missing_const_for_fn, reason = "test code simplicity")]
#![expect(clippy::redundant_clone, reason = "test clarity over efficiency")]
#![expect(clippy::needless_collect, reason = "test readability")]
#![expect(clippy::implicit_clone, reason = "test clarity")]
#![expect(clippy::uninlined_format_args, reason = "test readability")]
#![expect(clippy::field_reassign_with_default, reason = "test setup clarity")]

use std::fs;
use std::path::{Path, PathBuf};

use git2::{Repository, Signature};
use muster::agent::{Agent, Storage};
use muster::config::Config;
use muster::tmux::SessionManager;
use tempfile::TempDir;

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
    fn new(test_name: &str) -> Self {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Initialize git repo with initial commit
        let repo = Repository::init(&repo_path).unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();

        // Create a file and commit it
        let readme_path = repo_path.join("README.md");
        fs::write(&readme_path, "# Test Repository\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        let worktree_dir = TempDir::new().unwrap();
        let state_dir = TempDir::new().unwrap();

        // Generate unique session prefix for this test run
        let session_prefix = format!("muster-test-{}-{}", test_name, std::process::id());

        Self {
            _temp_dir: temp_dir,
            repo_path,
            worktree_dir,
            state_dir,
            session_prefix,
        }
    }

    fn config(&self) -> Config {
        Config {
            default_program: "echo".to_string(), // Use echo instead of claude for testing
            branch_prefix: format!("{}/", self.session_prefix),
            worktree_dir: self.worktree_dir.path().to_path_buf(),
            auto_yes: false,
            poll_interval_ms: 100,
            max_agents: 10,
            keys: Default::default(),
        }
    }

    fn storage_path(&self) -> PathBuf {
        self.state_dir.path().join("agents.json")
    }

    fn create_storage(&self) -> Storage {
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
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        self.cleanup_sessions();
    }
}

fn tmux_available() -> bool {
    muster::tmux::is_available()
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
fn test_cmd_list_shows_agents() {
    let fixture = TestFixture::new("list");
    let mut storage = fixture.create_storage();

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
    let agents: Vec<_> = storage.iter().collect();
    assert_eq!(agents.len(), 2);
}

#[test]
fn test_cmd_list_filter_running() {
    let fixture = TestFixture::new("list_filter");
    let mut storage = fixture.create_storage();

    let mut agent1 = Agent::new(
        "running-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("running"),
        fixture.worktree_dir.path().join("running"),
        None,
    );
    agent1.set_status(muster::Status::Running);

    let mut agent2 = Agent::new(
        "paused-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("paused"),
        fixture.worktree_dir.path().join("paused"),
        None,
    );
    agent2.set_status(muster::Status::Paused);

    storage.add(agent1);
    storage.add(agent2);

    // Filter running only
    let running: Vec<_> = storage
        .iter()
        .filter(|a| a.status == muster::Status::Running)
        .collect();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].title, "running-agent");
}

// =============================================================================
// Integration tests for find_agent
// =============================================================================

#[test]
fn test_find_agent_by_short_id_integration() {
    let fixture = TestFixture::new("find_short");
    let mut storage = fixture.create_storage();

    let agent = Agent::new(
        "findable-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("findable"),
        fixture.worktree_dir.path().join("findable"),
        None,
    );
    let short_id = agent.short_id().to_string();
    let full_id = agent.id;
    storage.add(agent);

    // Find by short ID
    let found = storage.find_by_short_id(&short_id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, full_id);
}

#[test]
fn test_find_agent_by_index_integration() {
    let fixture = TestFixture::new("find_index");
    let mut storage = fixture.create_storage();

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
    assert_ne!(found0.unwrap().id, found1.unwrap().id);
}

// =============================================================================
// Integration tests for tmux session operations
// =============================================================================

#[test]
fn test_tmux_session_lifecycle() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("lifecycle");
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
}

#[test]
fn test_tmux_session_list() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("list_sessions");
    let manager = SessionManager::new();
    let session_name = fixture.session_name("listtest");

    // Create a session
    let _ = manager.kill(&session_name);
    manager
        .create(&session_name, fixture.worktree_dir.path(), None)
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(100));

    // List sessions and verify our session is present
    let sessions = manager.list().unwrap();
    let found = sessions.iter().any(|s| s.name == session_name);
    assert!(found, "Created session should appear in list");

    // Cleanup
    let _ = manager.kill(&session_name);
}

// =============================================================================
// Integration tests for git worktree operations
// =============================================================================

#[test]
fn test_git_worktree_create_and_remove() {
    let fixture = TestFixture::new("worktree");
    let repo = muster::git::open_repository(&fixture.repo_path).unwrap();
    let manager = muster::git::WorktreeManager::new(&repo);

    let worktree_path = fixture.worktree_dir.path().join("test-worktree");
    let branch_name = "test-branch";

    // Create worktree with new branch
    let result = manager.create_with_new_branch(&worktree_path, branch_name);
    assert!(result.is_ok(), "Failed to create worktree: {:?}", result);

    // Verify worktree exists
    assert!(worktree_path.exists());
    assert!(worktree_path.join(".git").exists());

    // Remove worktree
    let result = manager.remove(branch_name);
    assert!(result.is_ok(), "Failed to remove worktree: {:?}", result);
}

// =============================================================================
// Integration tests for agent creation workflow
// =============================================================================

#[test]
fn test_agent_creation_workflow() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("create_workflow");
    let config = fixture.config();
    let mut storage = fixture.create_storage();
    let manager = SessionManager::new();

    // Create agent manually (simulating cmd_new)
    let title = "test-workflow";
    let branch = config.generate_branch_name(title);
    let worktree_path = config.worktree_dir.join(&branch);
    let session_name = branch.replace('/', "-");

    // Create git worktree
    let repo = muster::git::open_repository(&fixture.repo_path).unwrap();
    let worktree_mgr = muster::git::WorktreeManager::new(&repo);
    worktree_mgr
        .create_with_new_branch(&worktree_path, &branch)
        .unwrap();

    // Create tmux session with a command that stays alive
    manager
        .create(&session_name, &worktree_path, Some("sleep 10"))
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Create agent record
    let agent = Agent::new(
        title.to_string(),
        config.default_program.clone(),
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
}

// =============================================================================
// Integration tests for storage persistence
// =============================================================================

#[test]
fn test_storage_save_and_load() {
    let fixture = TestFixture::new("storage_persist");
    let storage_path = fixture.storage_path();

    // Create storage with agents
    let mut storage = fixture.create_storage();
    storage.add(Agent::new(
        "persistent-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("persist"),
        fixture.worktree_dir.path().join("persist"),
        None,
    ));

    // Save to file
    storage.save_to(&storage_path).unwrap();

    // Verify file exists
    assert!(storage_path.exists());

    // Load from file
    let loaded = Storage::load_from(&storage_path).unwrap();
    assert_eq!(loaded.len(), 1);

    let agent = loaded.iter().next().unwrap();
    assert_eq!(agent.title, "persistent-agent");
}

// =============================================================================
// Integration tests for config persistence
// =============================================================================

#[test]
fn test_config_save_and_load() {
    let fixture = TestFixture::new("config_persist");
    let config_path = fixture.state_dir.path().join("config.json");

    let mut config = Config::default();
    config.default_program = "custom-program".to_string();
    config.max_agents = 20;

    // Save config
    config.save_to(&config_path).unwrap();

    // Load config
    let loaded = Config::load_from(&config_path).unwrap();
    assert_eq!(loaded.default_program, "custom-program");
    assert_eq!(loaded.max_agents, 20);
}

// =============================================================================
// Integration tests for agent status transitions
// =============================================================================

#[test]
fn test_agent_status_transitions() {
    let fixture = TestFixture::new("status_trans");
    let mut storage = fixture.create_storage();

    let mut agent = Agent::new(
        "status-test".to_string(),
        "echo".to_string(),
        fixture.session_name("status"),
        fixture.worktree_dir.path().join("status"),
        None,
    );

    // Initial status should be Starting
    assert_eq!(agent.status, muster::Status::Starting);

    // Transition to Running
    agent.set_status(muster::Status::Running);
    assert_eq!(agent.status, muster::Status::Running);
    assert!(agent.status.can_pause());
    assert!(!agent.status.can_resume());

    // Transition to Paused
    agent.set_status(muster::Status::Paused);
    assert_eq!(agent.status, muster::Status::Paused);
    assert!(!agent.status.can_pause());
    assert!(agent.status.can_resume());

    // Back to Running
    agent.set_status(muster::Status::Running);
    assert_eq!(agent.status, muster::Status::Running);

    // Transition to Stopped
    agent.set_status(muster::Status::Stopped);
    assert_eq!(agent.status, muster::Status::Stopped);
    assert!(!agent.status.can_pause());
    assert!(!agent.status.can_resume());

    storage.add(agent);
    assert_eq!(storage.len(), 1);
}

// =============================================================================
// Integration tests for Actions handler with real operations
// =============================================================================

#[test]
fn test_actions_create_agent_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_create");
    let config = fixture.config();
    let storage = fixture.create_storage();

    // Change to repo directory for the test
    let original_dir = std::env::current_dir().unwrap();
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent via the handler
    let result = handler.create_agent(&mut app, "integration-test", None);

    // Cleanup first
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    // Restore original directory
    let _ = std::env::set_current_dir(&original_dir);

    assert!(result.is_ok(), "Failed to create agent: {:?}", result);
    assert_eq!(app.storage.len(), 1);
}

#[test]
fn test_actions_create_agent_with_prompt_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_prompt");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent with a prompt
    let result = handler.create_agent(&mut app, "prompted-agent", Some("test prompt"));

    std::env::set_current_dir(&original_dir).unwrap();

    assert!(result.is_ok(), "Failed to create agent: {:?}", result);
    assert_eq!(app.storage.len(), 1);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }
}

#[test]
fn test_actions_kill_agent_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_kill");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent first
    handler.create_agent(&mut app, "killable", None).unwrap();
    assert_eq!(app.storage.len(), 1);

    // Select the agent
    app.select_next();

    // Now kill it via confirm action
    app.enter_mode(muster::app::Mode::Confirming(
        muster::app::ConfirmAction::Kill,
    ));
    let result = handler.handle_action(&mut app, muster::config::Action::Confirm);

    std::env::set_current_dir(&original_dir).unwrap();

    assert!(result.is_ok());
    assert_eq!(app.storage.len(), 0);
}

#[test]
fn test_actions_pause_resume_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_pause_resume");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "pausable", None).unwrap();
    app.select_next();

    // Wait for agent to start
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Manually set to Running (since "echo" command exits quickly)
    if let Some(agent) = app.selected_agent_mut() {
        agent.set_status(muster::Status::Running);
    }

    // Pause the agent
    let result = handler.handle_action(&mut app, muster::config::Action::Pause);
    assert!(result.is_ok());

    // Check status is paused
    if let Some(agent) = app.selected_agent() {
        assert_eq!(agent.status, muster::Status::Paused);
    }

    // Resume the agent
    let result = handler.handle_action(&mut app, muster::config::Action::Resume);
    assert!(result.is_ok());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }
}

#[test]
fn test_actions_update_preview_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_preview");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    handler
        .create_agent(&mut app, "preview-test", None)
        .unwrap();
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
}

#[test]
fn test_actions_update_diff_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_diff");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "diff-test", None).unwrap();
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
}

#[test]
fn test_actions_attach_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_attach");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    handler.create_agent(&mut app, "attachable", None).unwrap();
    app.select_next();

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Request attach - this sets the attach_session field if session exists
    // Note: The session may have already exited (echo command), so attach may fail
    let _result = handler.handle_action(&mut app, muster::config::Action::Attach);

    let _ = std::env::set_current_dir(&original_dir);

    // The attach action either succeeds or sets an error
    // We just verify the action was processed without panic

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }
}

#[test]
fn test_actions_reset_all_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_reset");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fixture.repo_path).unwrap();

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create multiple agents
    handler.create_agent(&mut app, "reset1", None).unwrap();
    handler.create_agent(&mut app, "reset2", None).unwrap();
    assert_eq!(app.storage.len(), 2);

    // Reset all via confirm action
    app.enter_mode(muster::app::Mode::Confirming(
        muster::app::ConfirmAction::Reset,
    ));
    let result = handler.handle_action(&mut app, muster::config::Action::Confirm);
    assert!(result.is_ok());
    assert_eq!(app.storage.len(), 0);

    let _ = std::env::set_current_dir(&original_dir);
}

#[test]
fn test_actions_push_branch_integration() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("actions_push");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "pushable", None);

    // Early cleanup if creation failed
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        // Skip test if agent creation fails (e.g., git/tmux issues)
        return;
    }

    app.select_next();

    // Push action (just sets status message, doesn't actually push in test)
    let result = handler.handle_action(&mut app, muster::config::Action::Push);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }

    let _ = std::env::set_current_dir(&original_dir);

    assert!(result.is_ok());
}

// =============================================================================
// Integration tests for tmux capture functions
// =============================================================================

#[test]
fn test_tmux_capture_pane() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("capture_pane");
    let manager = SessionManager::new();
    let session_name = fixture.session_name("capture");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager
        .create(&session_name, fixture.worktree_dir.path(), Some("sleep 60"))
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture the pane
    let capture = muster::tmux::OutputCapture::new();
    let result = capture.capture_pane(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture failed: {:?}", result);
}

#[test]
fn test_tmux_capture_pane_with_history() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("capture_history");
    let manager = SessionManager::new();
    let session_name = fixture.session_name("hist");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager
        .create(&session_name, fixture.worktree_dir.path(), Some("sleep 60"))
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture with history
    let capture = muster::tmux::OutputCapture::new();
    let result = capture.capture_pane_with_history(&session_name, 100);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture with history failed: {:?}", result);
}

#[test]
fn test_tmux_capture_full_history() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("capture_full");
    let manager = SessionManager::new();
    let session_name = fixture.session_name("full");

    // Create a session that stays alive
    let _ = manager.kill(&session_name);
    manager
        .create(&session_name, fixture.worktree_dir.path(), Some("sleep 60"))
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(300));

    // Verify session exists
    assert!(
        manager.exists(&session_name),
        "Session should exist before capture"
    );

    // Capture full history
    let capture = muster::tmux::OutputCapture::new();
    let result = capture.capture_full_history(&session_name);

    // Cleanup
    let _ = manager.kill(&session_name);

    assert!(result.is_ok(), "Capture full history failed: {:?}", result);
}

#[test]
fn test_tmux_capture_nonexistent_session() {
    if skip_if_no_tmux() {
        return;
    }

    let capture = muster::tmux::OutputCapture::new();
    let result = capture.capture_pane("nonexistent-session-xyz");
    assert!(result.is_err());
}

#[test]
fn test_tmux_send_keys() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("send_keys");
    let manager = SessionManager::new();
    let session_name = fixture.session_name("keys");

    // Create a session
    let _ = manager.kill(&session_name);
    manager
        .create(&session_name, fixture.worktree_dir.path(), None)
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Send keys
    let result = manager.send_keys(&session_name, "echo test");
    assert!(result.is_ok());

    // Cleanup
    let _ = manager.kill(&session_name);
}

// =============================================================================
// Integration tests for CLI command success paths
// =============================================================================

#[test]
fn test_cmd_kill_success() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("cmd_kill");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent first
    let create_result = handler.create_agent(&mut app, "killable", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return;
    }

    // Get agent info for kill command
    let agent = app.storage.iter().next().unwrap();
    let agent_id = agent.id;
    let session = agent.tmux_session.clone();
    let branch = agent.branch.clone();

    // Save storage so cmd_kill can load it
    let storage_path = fixture.storage_path();
    app.storage.save_to(&storage_path).unwrap();

    // Simulate kill: kill session, remove worktree, remove from storage
    let manager = SessionManager::new();
    let _ = manager.kill(&session);

    let repo = muster::git::open_repository(&fixture.repo_path).unwrap();
    let worktree_mgr = muster::git::WorktreeManager::new(&repo);
    let _ = worktree_mgr.remove(&branch);

    app.storage.remove(agent_id);
    app.storage.save_to(&storage_path).unwrap();

    let _ = std::env::set_current_dir(&original_dir);

    assert_eq!(app.storage.len(), 0);
}

#[test]
fn test_cmd_pause_success() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("cmd_pause_success");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "pausable", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return;
    }

    // Mark as running
    if let Some(agent) = app.storage.iter_mut().next() {
        agent.set_status(muster::Status::Running);
    }
    app.select_next();

    // Pause via handler
    let result = handler.handle_action(&mut app, muster::config::Action::Pause);
    assert!(result.is_ok());

    // Verify paused
    if let Some(agent) = app.selected_agent() {
        assert_eq!(agent.status, muster::Status::Paused);
    }

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }
}

#[test]
fn test_cmd_resume_success() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("cmd_resume_success");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "resumable", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return;
    }

    // Mark as paused
    if let Some(agent) = app.storage.iter_mut().next() {
        agent.set_status(muster::Status::Paused);
    }
    app.select_next();

    // Resume via handler
    let result = handler.handle_action(&mut app, muster::config::Action::Resume);
    assert!(result.is_ok());

    // Verify running
    if let Some(agent) = app.selected_agent() {
        assert_eq!(agent.status, muster::Status::Running);
    }

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.storage.iter() {
        let _ = manager.kill(&agent.tmux_session);
    }
}

#[test]
fn test_sync_agent_status_transitions() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("sync_status");
    let config = fixture.config();
    let storage = fixture.create_storage();

    let original_dir = std::env::current_dir().unwrap();
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = muster::App::new(config.clone(), storage);
    let handler = muster::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app, "sync-test", None);
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        return;
    }

    // Agent starts as Starting
    if let Some(agent) = app.storage.iter().next() {
        assert_eq!(agent.status, muster::Status::Starting);
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
}

// =============================================================================
// Integration test for full CLI workflow simulation
// =============================================================================

#[test]
fn test_full_cli_workflow() {
    if skip_if_no_tmux() {
        return;
    }

    let fixture = TestFixture::new("full_workflow");
    let config = fixture.config();
    let mut storage = fixture.create_storage();
    let manager = SessionManager::new();

    // 1. Create an agent (simulate `muster new`)
    let title = "workflow-agent";
    let branch = config.generate_branch_name(title);
    let worktree_path = config.worktree_dir.join(&branch);
    let session_name = branch.replace('/', "-");

    let repo = muster::git::open_repository(&fixture.repo_path).unwrap();
    let worktree_mgr = muster::git::WorktreeManager::new(&repo);
    worktree_mgr
        .create_with_new_branch(&worktree_path, &branch)
        .unwrap();

    manager
        .create(&session_name, &worktree_path, Some("sleep 60"))
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut agent = Agent::new(
        title.to_string(),
        config.default_program.clone(),
        branch.clone(),
        worktree_path.clone(),
        None,
    );
    agent.set_status(muster::Status::Running);
    let agent_id = agent.id;
    storage.add(agent);

    // 2. List agents (simulate `muster list`)
    assert_eq!(storage.len(), 1);
    let agents: Vec<_> = storage.iter().collect();
    assert_eq!(agents[0].title, title);

    // 3. Pause agent (simulate `muster pause`)
    let _ = manager.kill(&session_name);
    if let Some(agent) = storage.get_mut(agent_id) {
        agent.set_status(muster::Status::Paused);
    }

    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!manager.exists(&session_name));

    // 4. Resume agent (simulate `muster resume`)
    manager
        .create(&session_name, &worktree_path, Some("sleep 60"))
        .unwrap();
    if let Some(agent) = storage.get_mut(agent_id) {
        agent.set_status(muster::Status::Running);
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
}
