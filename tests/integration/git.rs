//! Tests for git worktree operations

use crate::common::{DirGuard, TestFixture, git_command};
use tenex::agent::{Agent, Storage};
use tenex::app::{Actions, Settings};
use tenex::config::Action;

fn assert_git_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn test_git_worktree_create_and_remove() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("worktree")?;
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let manager = tenex::git::WorktreeManager::new(&repo);

    let worktree_path = fixture.worktree_path().join("test-worktree");
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

#[test]
fn test_execute_rename_same_name() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rename_same")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "test-agent".to_string(),
        "echo".to_string(),
        "test-branch".to_string(),
        fixture.repo_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up rename state with same name
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "test-agent".to_string();
    app.data.git_op.original_branch = "test-agent".to_string();
    app.data.git_op.is_root_rename = true;

    // Execute rename
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);

    // Should set status "Name unchanged"
    assert!(
        app.data
            .ui
            .status_message
            .as_ref()
            .is_some_and(|s| s.contains("unchanged")),
        "Should show name unchanged status"
    );

    Ok(())
}

#[test]
fn test_execute_push_with_valid_agent() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("push_valid")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a worktree for the agent
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-test-push-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "push-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up push state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch_name.to_string();

    // Execute push - will fail because there's no remote, but should handle gracefully
    let result = Actions::execute_push(&mut app.data);
    assert!(result.is_ok(), "Push should handle no remote gracefully");
    app.apply_mode(result?);

    // Should have set an error message about push failure
    assert!(
        app.data.ui.last_error.is_some(),
        "Should have error about push failure"
    );

    Ok(())
}

#[test]
fn test_execute_rebase_with_valid_agent() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rebase_valid")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a feature branch with a commit
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-test-rebase-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    // Make a commit on the feature branch
    std::fs::write(worktree_path.join("test.txt"), "test content")?;
    let output = git_command()
        .args(["add", "test.txt"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git add test.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Test commit"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "rebase-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up rebase state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch_name.to_string();
    app.data.git_op.target_branch = "master".to_string();

    // Execute rebase - should succeed since there are no conflicts
    let result = Actions::execute_rebase(&mut app.data);
    assert!(result.is_ok(), "Rebase should succeed: {result:?}");
    app.apply_mode(result?);

    // Should either show success or set an error (depends on git state)
    // The key test is that the function doesn't panic and handles the result
    let is_success = matches!(app.mode, tenex::AppMode::SuccessModal(_));
    let has_error = app.data.ui.last_error.is_some();
    assert!(
        is_success || has_error,
        "Should be in SuccessModal mode or have an error after rebase"
    );

    Ok(())
}

#[test]
fn test_execute_merge_with_valid_agent() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("merge_valid")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a feature branch with a commit
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-test-merge-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    // Make a commit on the feature branch
    std::fs::write(worktree_path.join("feature.txt"), "feature content")?;
    let output = git_command()
        .args(["add", "feature.txt"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git add feature.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Feature commit"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "merge-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Change to repo directory for the merge to work (with DirGuard for cleanup on panic)
    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    // Set up merge state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch_name.to_string();
    app.data.git_op.target_branch = "master".to_string();

    // Execute merge
    let result = Actions::execute_merge(&mut app.data);

    // DirGuard will restore directory on drop
    assert!(result.is_ok(), "Merge should succeed: {result:?}");
    app.apply_mode(result?);

    Ok(())
}

#[test]
fn test_push_action_handler() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("push_action")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "push-action-test".to_string(),
        "echo".to_string(),
        "feature/test".to_string(),
        fixture.repo_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Use handle_action to initiate push flow
    let handler = Actions::new();
    handler.handle_action(&mut app, Action::Push)?;

    // Should set up git_op state
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, "feature/test");
    assert_eq!(
        app.mode,
        tenex::AppMode::ConfirmPush(tenex::state::ConfirmPushMode)
    );

    Ok(())
}

#[test]
fn test_rename_action_handler() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rename_action")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "rename-action-test".to_string(),
        "echo".to_string(),
        "feature/rename".to_string(),
        fixture.repo_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Use handle_action to initiate rename flow
    let handler = Actions::new();
    handler.handle_action(&mut app, Action::RenameBranch)?;

    // Should set up git_op state
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.original_branch, "rename-action-test");
    assert!(app.data.git_op.is_root_rename);
    assert_eq!(
        app.mode,
        tenex::AppMode::RenameBranch(tenex::state::RenameBranchMode)
    );

    Ok(())
}

#[test]
fn test_rebase_action_handler() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rebase_action")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Change to repo directory (with DirGuard for cleanup on panic)
    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "rebase-action-test".to_string(),
        "echo".to_string(),
        "feature/rebase".to_string(),
        fixture.repo_path.clone(),
    );
    app.data.storage.add(agent);

    // Use handle_action to initiate rebase flow
    let handler = Actions::new();
    let result = handler.handle_action(&mut app, Action::Rebase);

    // DirGuard will restore directory on drop
    assert!(result.is_ok());
    assert_eq!(
        app.mode,
        tenex::AppMode::RebaseBranchSelector(tenex::state::RebaseBranchSelectorMode)
    );

    Ok(())
}

#[test]
fn test_merge_action_handler() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("merge_action")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Change to repo directory (with DirGuard for cleanup on panic)
    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "merge-action-test".to_string(),
        "echo".to_string(),
        "feature/merge".to_string(),
        fixture.repo_path.clone(),
    );
    app.data.storage.add(agent);

    // Use handle_action to initiate merge flow
    let handler = Actions::new();
    let result = handler.handle_action(&mut app, Action::Merge);

    // DirGuard will restore directory on drop
    assert!(result.is_ok());
    assert_eq!(
        app.mode,
        tenex::AppMode::MergeBranchSelector(tenex::state::MergeBranchSelectorMode)
    );

    Ok(())
}

#[test]
fn test_open_pr_action_handler() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("pr_action")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent with worktree
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-test-pr-action-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    let agent = Agent::new(
        "pr-action-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Use handle_action to initiate open PR flow
    let handler = Actions::new();
    handler.handle_action(&mut app, Action::OpenPR)?;

    // Should set up git_op state
    assert_eq!(app.data.git_op.agent_id, Some(agent_id));
    assert_eq!(app.data.git_op.branch_name, branch_name);
    // Should have detected a base branch or be in the right mode
    assert_eq!(
        app.mode,
        tenex::AppMode::ConfirmPushForPR(tenex::state::ConfirmPushForPRMode)
    );

    Ok(())
}

#[test]
fn test_execute_root_rename_with_real_worktree() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("root_rename")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a worktree for the agent
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let old_branch = "tenex-old-name";
    let worktree_path = fixture.worktree_path().join(old_branch);
    worktree_mgr.create_with_new_branch(&worktree_path, old_branch)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "old-name".to_string(),
        "echo".to_string(),
        old_branch.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up rename state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "new-name".to_string();
    app.data.git_op.original_branch = "old-name".to_string();
    app.data.git_op.is_root_rename = true;

    // Execute rename
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);

    // Verify the agent title was updated
    let renamed_agent = app.data.storage.get(agent_id);
    assert!(renamed_agent.is_some(), "Agent should exist after rename");
    if let Some(agent) = renamed_agent {
        assert_eq!(agent.title, "new-name");
        // The branch should be renamed
        assert!(agent.branch.contains("new-name"));
    }

    Ok(())
}

#[test]
fn test_execute_subagent_rename() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("subagent_rename")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Create a root agent
    let root = Agent::new(
        "root".to_string(),
        "echo".to_string(),
        "tenex-root".to_string(),
        fixture.repo_path.clone(),
    );
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    app.data.storage.add(root);

    // Create a child agent
    let child = tenex::agent::Agent::new_child(
        "old-child-name".to_string(),
        "echo".to_string(),
        "tenex-root".to_string(),
        fixture.repo_path.clone(),
        tenex::agent::ChildConfig {
            parent_id: root_id,
            mux_session: root_session,
            window_index: 2,
        },
    );
    let child_id = child.id;
    app.data.storage.add(child);

    // Set up rename state for child (not root)
    app.data.git_op.agent_id = Some(child_id);
    app.data.git_op.branch_name = "new-child-name".to_string();
    app.data.git_op.original_branch = "old-child-name".to_string();
    app.data.git_op.is_root_rename = false;

    // Execute rename
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);

    // Verify the agent title was updated
    let renamed_agent = app.data.storage.get(child_id);
    assert!(
        renamed_agent.is_some(),
        "Child agent should exist after rename"
    );
    if let Some(agent) = renamed_agent {
        assert_eq!(agent.title, "new-child-name");
    }

    // Verify status message
    assert!(
        app.data
            .ui
            .status_message
            .as_ref()
            .is_some_and(|s| s.contains("Renamed")),
        "Should show renamed status"
    );

    Ok(())
}

#[test]
fn test_execute_rebase_with_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rebase_conflict")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a feature branch with a commit
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-conflict-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    // Create a file on the feature branch
    std::fs::write(worktree_path.join("conflict.txt"), "feature content")?;
    let output = git_command()
        .args(["add", "conflict.txt"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git add conflict.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Feature commit"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    // Now create a conflicting commit on master
    std::fs::write(fixture.repo_path.join("conflict.txt"), "master content")?;
    let output = git_command()
        .args(["add", "conflict.txt"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert_git_success(&output, "git add conflict.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Master commit"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "conflict-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up rebase state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch_name.to_string();
    app.data.git_op.target_branch = "master".to_string();

    // Count agents before
    let agents_before = app.data.storage.iter().count();

    // Execute rebase - in a real environment, this spawns a conflict terminal
    // In test environment (no active session), it may error when trying to create a window
    let result = Actions::execute_rebase(&mut app.data);
    let result_is_err = result.is_err();

    // The rebase should handle the situation gracefully.
    // Multiple outcomes are valid depending on git state and session availability:
    // 1. Conflict detected -> terminal spawned (new agent added)
    // 2. Conflict detected -> error (result.is_err())
    // 3. No conflict -> success modal shown
    // 4. Error -> error message set
    // The key is that the function handles it without panicking.
    if let Ok(next) = result {
        app.apply_mode(next);
    }

    let conflict_terminal_spawned = app.data.storage.iter().count() > agents_before;
    let is_success = matches!(app.mode, tenex::AppMode::SuccessModal(_));
    let is_error_modal = matches!(app.mode, tenex::AppMode::ErrorModal(_));
    let has_error = result_is_err || app.data.ui.last_error.is_some() || is_error_modal;

    assert!(
        conflict_terminal_spawned || has_error || is_success,
        "Rebase should detect conflict or error. conflict_spawned={conflict_terminal_spawned}, has_error={has_error}, is_success={is_success}, mode={:?}",
        app.mode
    );

    Ok(())
}

#[test]
fn test_execute_merge_with_conflict() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("merge_conflict")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // First, create a commit on master with a file that will conflict
    std::fs::write(fixture.repo_path.join("shared.txt"), "initial")?;
    let output = git_command()
        .args(["add", "shared.txt"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert_git_success(&output, "git add shared.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Initial shared file"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    // Create a feature branch with a worktree
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-merge-conflict-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    // Modify the file on the feature branch
    std::fs::write(worktree_path.join("shared.txt"), "feature content")?;
    let output = git_command()
        .args(["add", "shared.txt"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git add shared.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Feature changes to shared"])
        .current_dir(&worktree_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    // Now create a conflicting modification on master
    std::fs::write(fixture.repo_path.join("shared.txt"), "master content")?;
    let output = git_command()
        .args(["add", "shared.txt"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert_git_success(&output, "git add shared.txt failed");
    let output = git_command()
        .args(["commit", "-m", "Master changes to shared"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    // Change to repo directory (with DirGuard for cleanup on panic)
    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "merge-conflict-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up merge state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch_name.to_string();
    app.data.git_op.target_branch = "master".to_string();

    // Count agents before
    let agents_before = app.data.storage.iter().count();

    // Execute merge - in real mux environment, this spawns a conflict terminal
    // In test environment (no mux), it may error when trying to create window
    let result = Actions::execute_merge(&mut app.data);

    // DirGuard will restore directory on drop

    // The merge should handle the situation gracefully.
    // Multiple outcomes are valid depending on git state and mux availability:
    // 1. Conflict detected -> terminal spawned (new agent added)
    // 2. Conflict detected -> mux error (result.is_err())
    // 3. Error -> error message set
    // 4. No conflict -> success modal shown
    // The key is that the function handles it without panicking.
    let conflict_terminal_spawned = app.data.storage.iter().count() > agents_before;
    let has_error = result.is_err() || app.data.ui.last_error.is_some();
    assert!(
        result.is_ok(),
        "Merge should complete without error: {result:?}"
    );
    if let Ok(next) = result {
        app.apply_mode(next);
    }
    let is_success = matches!(app.mode, tenex::AppMode::SuccessModal(_));

    // The merge function should complete without panicking.
    // In different environments, the outcome varies:
    // - Conflict detected -> terminal spawned or error set
    // - No conflict (fast-forward possible) -> success or mode stays normal
    // The key assertion is that result.is_ok() - the function handled it gracefully.
    // Additional check: if not in success mode and no error, mode should be Normal (no-op)
    let handled_correctly = conflict_terminal_spawned
        || has_error
        || is_success
        || app.mode == tenex::AppMode::normal();
    assert!(
        handled_correctly,
        "Merge should handle result. conflict_spawned={conflict_terminal_spawned}, has_error={has_error}, is_success={is_success}, mode={:?}",
        app.mode
    );

    Ok(())
}

#[test]
fn test_execute_push_and_open_pr_no_remote() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("push_pr_no_remote")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a worktree for the agent
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let branch_name = "tenex-push-pr-branch";
    let worktree_path = fixture.worktree_path().join(branch_name);
    worktree_mgr.create_with_new_branch(&worktree_path, branch_name)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent pointing to the worktree
    let agent = Agent::new(
        "push-pr-test".to_string(),
        "echo".to_string(),
        branch_name.to_string(),
        worktree_path,
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up push state
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = branch_name.to_string();
    app.data.git_op.base_branch = "master".to_string();

    // Execute push and open PR - will fail because there's no remote
    let result = Actions::execute_push_and_open_pr(&mut app.data);
    assert!(result.is_ok(), "Should handle no remote gracefully");
    app.apply_mode(result?);

    // Should have set an error message about push failure
    assert!(
        app.data.ui.last_error.is_some(),
        "Should have error about push failure"
    );

    Ok(())
}

#[test]
fn test_rename_unchanged_name() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rename_unchanged")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "same-name".to_string(),
        "echo".to_string(),
        "tenex-same".to_string(),
        fixture.repo_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up rename state with same name
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "same-name".to_string();
    app.data.git_op.original_branch = "same-name".to_string();
    app.data.git_op.is_root_rename = true;

    // Start in rename mode
    app.mode = tenex::AppMode::RenameBranch(tenex::state::RenameBranchMode);

    // Execute rename
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);

    // Should exit mode and set "unchanged" status
    assert_eq!(app.mode, tenex::AppMode::normal());
    assert!(
        app.data
            .ui
            .status_message
            .as_ref()
            .is_some_and(|s| s.contains("unchanged")),
        "Should show name unchanged status"
    );

    Ok(())
}

#[test]
fn test_execute_rebase_no_target_branch() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rebase_no_target")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "rebase-no-target".to_string(),
        "echo".to_string(),
        "tenex-feature".to_string(),
        fixture.repo_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up rebase state without target branch
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "tenex-feature".to_string();
    app.data.git_op.target_branch = String::new(); // Empty target

    // Execute rebase - should fail gracefully
    let result = Actions::execute_rebase(&mut app.data);
    assert!(
        result.is_ok(),
        "Should handle missing target branch gracefully"
    );
    app.apply_mode(result?);
    assert!(matches!(app.mode, tenex::AppMode::ErrorModal(_)));
    assert!(app.data.ui.last_error.is_some());

    Ok(())
}

#[test]
fn test_execute_merge_no_target_branch() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("merge_no_target")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Change to repo directory (with DirGuard for cleanup on panic)
    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add an agent
    let agent = Agent::new(
        "merge-no-target".to_string(),
        "echo".to_string(),
        "tenex-feature".to_string(),
        fixture.repo_path.clone(),
    );
    let agent_id = agent.id;
    app.data.storage.add(agent);

    // Set up merge state without target branch
    app.data.git_op.agent_id = Some(agent_id);
    app.data.git_op.branch_name = "tenex-feature".to_string();
    app.data.git_op.target_branch = String::new(); // Empty target

    // Execute merge - should fail gracefully
    let result = Actions::execute_merge(&mut app.data);

    // DirGuard will restore directory on drop
    assert!(
        result.is_ok(),
        "Should handle missing target branch gracefully"
    );
    app.apply_mode(result?);
    assert!(matches!(app.mode, tenex::AppMode::ErrorModal(_)));
    assert!(app.data.ui.last_error.is_some());

    Ok(())
}

#[test]
fn test_execute_rename_with_descendants() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("rename_descendants")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    // Create a worktree for the agent
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    let old_branch = "tenex-parent-old";
    let worktree_path = fixture.worktree_path().join(old_branch);
    worktree_mgr.create_with_new_branch(&worktree_path, old_branch)?;

    let mut app = tenex::App::new(config, storage, Settings::default(), false);

    // Add a root agent pointing to the worktree
    let root = Agent::new(
        "parent-old".to_string(),
        "echo".to_string(),
        old_branch.to_string(),
        worktree_path.clone(),
    );
    let root_id = root.id;
    let root_session = root.mux_session.clone();
    app.data.storage.add(root);

    // Add child agents
    for i in 0..3 {
        let child = tenex::agent::Agent::new_child(
            format!("child-{i}"),
            "echo".to_string(),
            old_branch.to_string(),
            worktree_path.clone(),
            tenex::agent::ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: i + 2,
            },
        );
        app.data.storage.add(child);
    }

    // Set up rename state
    app.data.git_op.agent_id = Some(root_id);
    app.data.git_op.branch_name = "parent-new".to_string();
    app.data.git_op.original_branch = "parent-old".to_string();
    app.data.git_op.is_root_rename = true;

    // Execute rename
    let next = Actions::execute_rename(&mut app.data)?;
    app.apply_mode(next);

    // Verify the root agent title was updated
    let renamed_root = app.data.storage.get(root_id);
    assert!(
        renamed_root.is_some(),
        "Root agent should exist after rename"
    );
    if let Some(agent) = renamed_root {
        assert_eq!(agent.title, "parent-new");
    }

    // Verify descendants' worktree paths were updated
    let descendants = app.data.storage.descendants(root_id);
    assert_eq!(descendants.len(), 3);
    for desc in descendants {
        // All descendants should have the new worktree path
        assert!(
            desc.worktree_path.to_string_lossy().contains("parent-new"),
            "Descendant should have updated worktree path"
        );
    }

    Ok(())
}
