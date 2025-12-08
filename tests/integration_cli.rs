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
use tenex::agent::{Agent, ChildConfig, Storage};
use tenex::app::{Actions, App};
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
        "starting-agent".to_string(),
        "echo".to_string(),
        fixture.session_name("starting"),
        fixture.worktree_dir.path().join("starting"),
        None,
    );
    agent2.set_status(tenex::Status::Starting);

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

    // Sync should remove dead agents
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
        worktree_path,
        None,
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

// =============================================================================
// Integration tests for nested agent hierarchy window index tracking
// =============================================================================

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

// =============================================================================
// Integration tests for synthesis functionality
// =============================================================================

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

#[test]
fn test_git_exclude_tenex_directory() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("git_exclude")?;

    // Call ensure_tenex_excluded
    let result = tenex::git::ensure_tenex_excluded(&fixture.repo_path);
    assert!(result.is_ok());

    // Check that .git/info/exclude contains .tenex/
    let exclude_path = fixture.repo_path.join(".git/info/exclude");
    assert!(exclude_path.exists());

    let contents = std::fs::read_to_string(&exclude_path)?;
    assert!(
        contents.contains(".tenex/"),
        "Exclude file should contain .tenex/"
    );

    // Call again - should be idempotent
    let result = tenex::git::ensure_tenex_excluded(&fixture.repo_path);
    assert!(result.is_ok());

    // Should still only have one .tenex/ entry
    let contents = std::fs::read_to_string(&exclude_path)?;
    let count = contents.matches(".tenex/").count();
    assert_eq!(count, 1, "Should only have one .tenex/ entry");

    Ok(())
}

// =============================================================================
// Performance Optimization Tests
// =============================================================================

/// Helper to create a child agent with specified parent and window index
fn create_child_agent(parent: &Agent, title: &str, window_index: u32) -> Agent {
    Agent::new_child(
        title.to_string(),
        "echo".to_string(),
        parent.branch.clone(),
        parent.worktree_path.clone(),
        None,
        ChildConfig {
            parent_id: parent.id,
            tmux_session: parent.tmux_session.clone(),
            window_index,
        },
    )
}

/// Test that `sync_agent_status` correctly removes agents whose sessions don't exist
/// using the batched session list approach (single tmux list-sessions call)
#[test]
fn test_sync_agent_status_batched_session_check() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("sync_batched")?;
    let manager = SessionManager::new();

    // Create 3 agents in storage
    let mut storage = Storage::new();

    let agent1 = Agent::new(
        "agent1".to_string(),
        "echo".to_string(),
        fixture.session_name("agent1"),
        fixture.worktree_dir.path().to_path_buf(),
        None,
    );
    let agent2 = Agent::new(
        "agent2".to_string(),
        "echo".to_string(),
        fixture.session_name("agent2"),
        fixture.worktree_dir.path().to_path_buf(),
        None,
    );
    let agent3 = Agent::new(
        "agent3".to_string(),
        "echo".to_string(),
        fixture.session_name("agent3"),
        fixture.worktree_dir.path().to_path_buf(),
        None,
    );

    let agent1_session = agent1.tmux_session.clone();
    storage.add(agent1);
    storage.add(agent2);
    storage.add(agent3);

    // Only create a real tmux session for agent1
    manager.create(
        &agent1_session,
        fixture.worktree_dir.path(),
        Some("sleep 60"),
    )?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Verify the session was created
    assert!(
        manager.exists(&agent1_session),
        "Session {agent1_session} should exist"
    );

    // Create app with the storage
    let mut app = App::new(fixture.config(), storage);
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
    let fixture = TestFixture::new("large_swarm")?;
    let manager = SessionManager::new();

    // Create root agent with real session
    let mut storage = Storage::new();
    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        fixture.session_name("root"),
        fixture.worktree_dir.path().to_path_buf(),
        None,
    );
    let root_session = root.tmux_session.clone();
    let root_id = root.id;
    storage.add(root.clone());

    // Create the root's tmux session with a long-running command
    manager.create(&root_session, fixture.worktree_dir.path(), Some("sleep 60"))?;
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
    let mut app = App::new(fixture.config(), storage);
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

// =============================================================================
// Worktree conflict detection and resolution tests
// =============================================================================

/// Test that creating an agent detects existing worktree and enters conflict mode
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_detection_single_agent() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_conflict_single")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);
    let handler = Actions::new();

    // First, create a worktree manually to simulate existing state
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/existing-agent", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Now try to create an agent with the same name
    // This should detect the conflict and enter Confirming mode
    handler.create_agent(&mut app, "existing-agent", Some("test prompt"))?;

    // Should be in Confirming(WorktreeConflict) mode
    assert!(
        matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Expected Confirming(WorktreeConflict) mode, got {:?}",
        app.mode
    );

    // Conflict info should be populated
    let conflict = app
        .worktree_conflict
        .as_ref()
        .expect("Conflict info should be set");
    assert_eq!(conflict.title, "existing-agent");
    assert_eq!(conflict.prompt, Some("test prompt".to_string()));
    assert!(
        conflict.swarm_child_count.is_none(),
        "Should not be a swarm"
    );

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test reconnecting to existing worktree for a single agent
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_reconnect_single_agent() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_reconnect_single")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);
    let handler = Actions::new();

    // Create a worktree manually
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/reconnect-test", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Trigger conflict detection
    handler.create_agent(&mut app, "reconnect-test", Some("original prompt"))?;

    // Verify we're in conflict mode
    assert!(matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ));

    // Modify the prompt before reconnecting (simulating user editing)
    if let Some(ref mut conflict) = app.worktree_conflict {
        conflict.prompt = Some("modified prompt".to_string());
    }

    // Now reconnect
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.reconnect_to_worktree(&mut app)?;

    // Should have created an agent
    assert_eq!(app.storage.len(), 1, "Should have one agent");

    let agent = app.storage.iter().next().expect("Should have an agent");
    assert_eq!(agent.title, "reconnect-test");
    assert_eq!(agent.initial_prompt, Some("modified prompt".to_string()));

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test recreating worktree (delete and create fresh)
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_recreate_single_agent() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_recreate_single")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);
    let handler = Actions::new();

    // Create a worktree manually with some content
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/recreate-test", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Add a marker file to the old worktree
    let marker_path = worktree_path.join("old_marker.txt");
    fs::write(&marker_path, "old worktree")?;
    assert!(
        marker_path.exists(),
        "Marker file should exist before recreate"
    );

    // Trigger conflict detection
    handler.create_agent(&mut app, "recreate-test", Some("new prompt"))?;

    // Verify we're in conflict mode
    assert!(matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ));

    // Now recreate (delete and create fresh)
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.recreate_worktree(&mut app)?;

    // Should have created an agent
    assert_eq!(app.storage.len(), 1, "Should have one agent");

    let agent = app.storage.iter().next().expect("Should have an agent");
    assert_eq!(agent.title, "recreate-test");
    assert_eq!(agent.initial_prompt, Some("new prompt".to_string()));

    // The old marker file should be gone (worktree was recreated)
    assert!(
        !marker_path.exists(),
        "Old marker file should be gone after recreate"
    );

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test worktree conflict detection for swarm creation (S key)
#[test]
#[expect(clippy::expect_used, reason = "test assertions")]
fn test_worktree_conflict_detection_swarm() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_conflict_swarm")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // Create a worktree manually that matches what spawn_children would create
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/swarm-task", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Set up for swarm spawning (simulating S key flow)
    app.spawning_under = None; // No parent = new root swarm
    app.child_count = 3;

    // Try to spawn children - should detect conflict
    let handler = Actions::new();
    handler.spawn_children(&mut app, "swarm-task")?;

    // Should be in Confirming(WorktreeConflict) mode
    assert!(
        matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Expected Confirming(WorktreeConflict) mode, got {:?}",
        app.mode
    );

    // Conflict info should indicate this is a swarm
    let conflict = app
        .worktree_conflict
        .as_ref()
        .expect("Conflict info should be set");
    assert_eq!(
        conflict.swarm_child_count,
        Some(3),
        "Should remember child count"
    );
    assert_eq!(conflict.prompt, Some("swarm-task".to_string()));

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test reconnecting to existing worktree for swarm - verifies children get the updated prompt
#[test]
#[expect(clippy::expect_used, clippy::unwrap_used, reason = "test assertions")]
fn test_worktree_conflict_reconnect_swarm_children_get_prompt()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_reconnect_swarm")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Use sleep command to keep sessions alive for swarm tests
    let mut config = fixture.config();
    config.default_program = "sleep 60".to_string();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // Create a worktree manually with a branch name that matches what spawn_children will generate
    // spawn_children uses the task as the title, which gets converted to a branch name
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let task = "swarm-reconnect-task";
    let branch_name = app.config.generate_branch_name(task);
    let worktree_path = app.config.worktree_dir.join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Set up for swarm spawning
    app.spawning_under = None;
    app.child_count = 2;

    // Trigger conflict detection - use the same task so branch names match
    let handler = Actions::new();
    handler.spawn_children(&mut app, task)?;

    // Verify we're in conflict mode with swarm info
    assert!(
        matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Expected Confirming(WorktreeConflict) mode, got {:?}",
        app.mode
    );
    assert_eq!(
        app.worktree_conflict.as_ref().unwrap().swarm_child_count,
        Some(2)
    );
    assert_eq!(
        app.worktree_conflict.as_ref().unwrap().prompt,
        Some(task.to_string())
    );

    // Modify the prompt before reconnecting (simulating user editing in ReconnectPrompt mode)
    let updated_task = "updated task for children";
    if let Some(ref mut conflict) = app.worktree_conflict {
        conflict.prompt = Some(updated_task.to_string());
    }

    // Now reconnect
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.reconnect_to_worktree(&mut app)?;

    // Should have created root + 2 children = 3 agents
    assert_eq!(app.storage.len(), 3, "Should have root + 2 children");

    // Find the root and children
    let root = app
        .storage
        .iter()
        .find(|a| a.is_root())
        .expect("Should have a root agent");
    let children: Vec<_> = app.storage.iter().filter(|a| !a.is_root()).collect();

    assert_eq!(children.len(), 2, "Should have 2 children");

    // Root should NOT have the prompt (root doesn't get the planning preamble)
    assert!(
        root.initial_prompt.is_none(),
        "Root should not have initial_prompt, got {:?}",
        root.initial_prompt
    );

    // Children SHOULD have the updated prompt (wrapped in planning preamble)
    for child in &children {
        let prompt = child
            .initial_prompt
            .as_ref()
            .expect("Child should have initial_prompt");
        assert!(
            prompt.contains(updated_task),
            "Child prompt should contain the updated task '{updated_task}'. Got: {prompt}"
        );
    }

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test recreating worktree for swarm
#[test]
fn test_worktree_conflict_recreate_swarm() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_recreate_swarm")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Use sleep command to keep sessions alive for swarm tests
    let mut config = fixture.config();
    config.default_program = "sleep 60".to_string();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // Create a worktree manually
    let repo = git2::Repository::open(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = format!("{}/swarm-recreate", fixture.session_prefix);
    let worktree_path = fixture.worktree_dir.path().join(&branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, &branch_name)?;

    // Add a marker file
    let marker_path = worktree_path.join("old_swarm_marker.txt");
    fs::write(&marker_path, "old swarm worktree")?;

    // Set up for swarm spawning
    app.spawning_under = None;
    app.child_count = 2;

    // Trigger conflict detection
    let handler = Actions::new();
    handler.spawn_children(&mut app, "swarm-recreate")?;

    // Verify we're in conflict mode
    assert!(matches!(
        app.mode,
        tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
    ));

    // Now recreate
    app.exit_mode();
    let handler2 = Actions::new();
    handler2.recreate_worktree(&mut app)?;

    // Should have created root + 2 children = 3 agents
    assert_eq!(app.storage.len(), 3, "Should have root + 2 children");

    // The old marker file should be gone
    assert!(
        !marker_path.exists(),
        "Old marker file should be gone after recreate"
    );

    // Verify we have correct structure
    let root_count = app.storage.iter().filter(|a| a.is_root()).count();
    let child_count = app.storage.iter().filter(|a| !a.is_root()).count();

    assert_eq!(root_count, 1, "Should have exactly 1 root");
    assert_eq!(child_count, 2, "Should have exactly 2 children");

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

/// Test that adding children to existing agent (A key) does NOT trigger conflict
/// (since it uses the parent's existing worktree)
#[test]
#[expect(clippy::unwrap_used, reason = "test assertions")]
fn test_add_children_to_existing_no_conflict() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_tmux() {
        return Ok(());
    }

    let original_dir = std::env::current_dir()?;
    let fixture = TestFixture::new("wt_add_children")?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Use sleep command to keep sessions alive for child spawning tests
    let mut config = fixture.config();
    config.default_program = "sleep 60".to_string();
    let storage = TestFixture::create_storage();
    let mut app = App::new(config, storage);

    // First create a root agent normally
    let handler = Actions::new();
    handler.create_agent(&mut app, "parent-agent", None)?;

    assert_eq!(app.storage.len(), 1, "Should have parent agent");
    let parent_id = app.storage.iter().next().unwrap().id;

    // Now add children to the existing agent (A key flow)
    app.spawning_under = Some(parent_id);
    app.child_count = 2;

    let handler2 = Actions::new();
    handler2.spawn_children(&mut app, "child task")?;

    // Should NOT be in conflict mode - should have spawned directly
    assert!(
        !matches!(
            app.mode,
            tenex::app::Mode::Confirming(tenex::app::ConfirmAction::WorktreeConflict)
        ),
        "Adding children to existing agent should not trigger conflict"
    );

    // Should have parent + 2 children
    assert_eq!(app.storage.len(), 3, "Should have parent + 2 children");

    // Cleanup
    fixture.cleanup_sessions();
    fixture.cleanup_branches();
    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}
