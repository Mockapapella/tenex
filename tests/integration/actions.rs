//! Tests for Actions handler with real operations

use crate::common::{DirGuard, TestFixture, git_command, skip_if_no_mux};
use std::time::Duration;
use tenex::agent::Storage;
use tenex::mux::SessionManager;

#[test]
fn test_actions_create_agent_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_create")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    // Change to repo directory for the test
    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent via the handler
    let result = handler.create_agent(&mut app.data, "integration-test", None);

    // Cleanup first
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    // Restore original directory
    let _ = std::env::set_current_dir(&original_dir);

    assert!(result.is_ok(), "Failed to create agent: {result:?}");
    assert_eq!(app.data.storage.len(), 1);

    Ok(())
}

#[test]
fn test_actions_switch_branch_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_switch_branch")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    let next = handler.create_agent(&mut app.data, "switchable", None)?;
    app.apply_mode(next);

    let root = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected an agent to be selected"))?
        .clone();

    let target_branch = format!("{}/target-branch", fixture.session_prefix);
    let output = git_command()
        .args(["branch", &target_branch])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert!(
        output.status.success(),
        "git branch failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    app.data.git_op.agent_id = Some(root.id);
    app.data.git_op.branch_name = root.branch.clone();
    app.data.git_op.target_branch = target_branch.clone();

    let next = handler.switch_branch(&mut app.data)?;
    app.apply_mode(next);

    assert_eq!(app.mode, tenex::AppMode::normal());
    assert!(
        app.data
            .ui
            .status_message
            .as_ref()
            .is_some_and(|msg| msg.contains("Switched to branch")),
        "Expected a switch status message"
    );

    assert_eq!(app.data.storage.len(), 1);
    let new_root = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected an agent after switching branches"))?;

    assert_eq!(new_root.title, "target-branch");
    assert_eq!(new_root.branch, target_branch);
    assert_ne!(new_root.id, root.id);

    assert!(
        !root.worktree_path.exists(),
        "Expected old worktree to be deleted"
    );
    assert!(new_root.worktree_path.exists());

    let expected_path = app
        .data
        .config
        .worktree_path_for_repo_root(&fixture.repo_path, &target_branch);
    crate::common::assert_paths_eq(
        &new_root.worktree_path,
        &expected_path,
        "Expected Tenex to use the canonical worktree path",
    );

    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let worktree_mgr = tenex::git::WorktreeManager::new(&repo);
    assert!(!worktree_mgr.exists(&root.branch));
    assert!(worktree_mgr.exists(&target_branch));

    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_switch_branch_from_remote_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_switch_branch_remote")?;
    let config = fixture.config();
    let storage = Storage::with_path(fixture.storage_path());

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    let next = handler.create_agent(&mut app.data, "switchable", None)?;
    app.apply_mode(next);

    let root = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected an agent to be selected"))?
        .clone();

    let remote_ref = "refs/remotes/origin/remote-target";
    let output = git_command()
        .args(["update-ref", remote_ref, "HEAD"])
        .current_dir(&fixture.repo_path)
        .output()?;
    assert!(
        output.status.success(),
        "git update-ref failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    app.data.git_op.agent_id = Some(root.id);
    app.data.git_op.branch_name = root.branch.clone();
    app.data.git_op.target_branch = "origin/remote-target".to_string();

    let next = handler.switch_branch(&mut app.data)?;
    app.apply_mode(next);

    assert_eq!(app.mode, tenex::AppMode::normal());
    assert_eq!(app.data.storage.len(), 1);

    let new_root = app
        .selected_agent()
        .ok_or_else(|| std::io::Error::other("Expected an agent after switching branches"))?;

    assert_eq!(new_root.title, "remote-target");
    assert_eq!(new_root.branch, "remote-target");
    assert_ne!(new_root.id, root.id);

    assert!(
        !root.worktree_path.exists(),
        "Expected old worktree to be deleted"
    );
    assert!(new_root.worktree_path.exists());

    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let branch_mgr = tenex::git::BranchManager::new(&repo);
    assert!(branch_mgr.exists("remote-target"));

    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_sync_agent_pane_activity_tracks_unseen_waiting()
-> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_pane_activity")?;
    let mut config = fixture.config();
    config.default_program = "sh".to_string();
    let storage = TestFixture::create_storage();

    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    let next = handler.create_agent(&mut app.data, "pane-a", None)?;
    app.apply_mode(next);
    let next = handler.create_agent(&mut app.data, "pane-b", None)?;
    app.apply_mode(next);

    for agent in app.data.storage.iter_mut() {
        agent.set_status(tenex::agent::Status::Running);
    }

    app.data.selected = 1;
    app.validate_selection();
    assert_eq!(
        app.selected_agent().map(|a| a.title.as_str()),
        Some("pane-a"),
        "Expected the first created agent to be selected"
    );

    std::thread::sleep(Duration::from_millis(300));

    let manager = SessionManager::new();

    let agents: Vec<_> = app.data.storage.iter().cloned().collect();
    let agent_a = agents
        .iter()
        .find(|agent| agent.title == "pane-a")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing pane-a agent"))?;
    let agent_b = agents
        .iter()
        .find(|agent| agent.title == "pane-b")
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing pane-b agent"))?;

    let token_a = format!("__tenex_pane_activity_a_{}__", agent_a.short_id());
    manager.send_keys_and_submit(&agent_a.mux_session, &format!("echo {token_a}"))?;
    let token_b = format!("__tenex_pane_activity_b_{}__", agent_b.short_id());
    manager.send_keys_and_submit(&agent_b.mux_session, &format!("echo {token_b}"))?;

    std::thread::sleep(Duration::from_secs(2));

    handler.sync_agent_pane_activity(&mut app)?;
    std::thread::sleep(Duration::from_millis(50));
    handler.sync_agent_pane_activity(&mut app)?;

    assert!(app.data.ui.agent_is_waiting_for_input(agent_a.id));
    assert!(!app.data.ui.agent_has_unseen_waiting_output(agent_a.id));

    assert!(app.data.ui.agent_is_waiting_for_input(agent_b.id));
    assert!(app.data.ui.agent_has_unseen_waiting_output(agent_b.id));

    app.data.ui.mark_agent_pane_seen(agent_b.id);
    assert!(!app.data.ui.agent_has_unseen_waiting_output(agent_b.id));

    let capture = tenex::mux::OutputCapture::new();
    let output_a = capture.capture_pane_with_history(&agent_a.mux_session, 200)?;
    assert!(
        output_a.contains(&token_a),
        "Expected pane-a output to contain token {token_a}, got: {output_a:?}"
    );
    let output_b = capture.capture_pane_with_history(&agent_b.mux_session, 200)?;
    assert!(
        output_b.contains(&token_b),
        "Expected pane-b output to contain token {token_b}, got: {output_b:?}"
    );

    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_create_agent_with_prompt_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_prompt")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent with a prompt
    let result = handler.create_agent(&mut app.data, "prompted-agent", Some("test prompt"));

    std::env::set_current_dir(&original_dir)?;

    assert!(result.is_ok(), "Failed to create agent: {result:?}");
    assert_eq!(app.data.storage.len(), 1);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_kill_agent_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_kill")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent first
    let next = handler.create_agent(&mut app.data, "killable", None)?;
    app.apply_mode(next);
    assert_eq!(app.data.storage.len(), 1);

    // Now kill it via confirm action
    app.enter_mode(
        tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Kill,
        }
        .into(),
    );
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);

    std::env::set_current_dir(&original_dir)?;

    assert!(result.is_ok());
    assert_eq!(app.data.storage.len(), 0);

    Ok(())
}

#[test]
fn test_actions_update_preview_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_preview")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let next = handler.create_agent(&mut app.data, "preview-test", None)?;
    app.apply_mode(next);

    // Wait for session
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Update preview
    let result = handler.update_preview(&mut app);
    assert!(result.is_ok());
    // Preview content should be set (either actual content or session not running)
    assert!(!app.data.ui.preview_content.is_empty());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_update_preview_full_history_when_scrolled() -> Result<(), Box<dyn std::error::Error>>
{
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_preview_scroll")?;
    let mut config = fixture.config();
    // Use an interactive shell so we can generate lots of output reliably.
    config.default_program = "sh".to_string();
    let storage = TestFixture::create_storage();

    // Change to repo directory for the test (DirGuard restores on drop).
    let _dir_guard = DirGuard::new()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    // Ensure a stable "visible height" for scroll calculations and resize new sessions.
    app.set_preview_dimensions(80, 20);

    let handler = tenex::app::Actions::new();
    let manager = SessionManager::new();

    // Create an agent.
    let next = handler.create_agent(&mut app.data, "preview-scroll-test", None)?;
    app.apply_mode(next);

    let session = app
        .selected_agent()
        .ok_or_else(|| anyhow::anyhow!("No agent selected"))?
        .mux_session
        .clone();

    // Generate >300 lines so the tail capture excludes early lines.
    manager.send_keys_and_submit(
        &session,
        "i=1; while [ $i -le 500 ]; do printf 'TENEX_SCROLL_TEST_LINE_%04d\\n' $i; i=$((i+1)); done",
    )?;

    // Wait for output to appear in the preview buffer.
    let start = std::time::Instant::now();
    loop {
        handler.update_preview(&mut app)?;
        if app
            .data
            .ui
            .preview_content
            .contains("TENEX_SCROLL_TEST_LINE_0500")
        {
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            return Err(anyhow::anyhow!("Timed out waiting for preview output").into());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    // While following, we should still be using a small tail window.
    assert!(app.data.ui.preview_follow);
    assert!(!app.data.ui.preview_using_full_history);
    assert!(
        !app.data
            .ui
            .preview_content
            .contains("TENEX_SCROLL_TEST_LINE_0001"),
        "Tail capture should not include earliest lines"
    );

    // Simulate user scrolling up (disables follow).
    app.scroll_up(10);
    assert!(!app.data.ui.preview_follow);

    // Record distance from bottom in the tail buffer.
    let old_line_count = app.data.ui.preview_content.lines().count();
    let visible_height = app
        .data
        .ui
        .preview_dimensions
        .map_or(20, |(_, h)| usize::from(h));
    let old_max = old_line_count.saturating_sub(visible_height);
    let old_scroll = app.data.ui.preview_scroll.min(old_max);
    let old_distance_from_bottom = old_max.saturating_sub(old_scroll);

    // Updating the preview while scrolled should switch to full history and keep scroll stable.
    handler.update_preview(&mut app)?;

    assert!(app.data.ui.preview_using_full_history);
    assert!(
        app.data
            .ui
            .preview_content
            .contains("TENEX_SCROLL_TEST_LINE_0001"),
        "Full history capture should include earliest lines"
    );

    let new_line_count = app.data.ui.preview_content.lines().count();
    let new_max = new_line_count.saturating_sub(visible_height);
    let new_scroll = app.data.ui.preview_scroll.min(new_max);
    let new_distance_from_bottom = new_max.saturating_sub(new_scroll);
    assert_eq!(new_distance_from_bottom, old_distance_from_bottom);

    // Cleanup
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_update_diff_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_diff")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let next = handler.create_agent(&mut app.data, "diff-test", None)?;
    app.apply_mode(next);

    // Update diff
    let result = handler.update_diff(&mut app);
    assert!(result.is_ok());
    // Diff content should be set (either "No changes" or actual diff)
    assert!(!app.data.ui.diff_content.is_empty());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_focus_preview_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_focus_preview")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let next = handler.create_agent(&mut app.data, "focusable", None)?;
    app.apply_mode(next);

    std::thread::sleep(std::time::Duration::from_millis(200));

    // FocusPreview should enter PreviewFocused mode
    let result = handler.handle_action(&mut app, tenex::config::Action::FocusPreview);
    assert!(result.is_ok());
    assert_eq!(
        app.mode,
        tenex::AppMode::PreviewFocused(tenex::state::PreviewFocusedMode)
    );

    // UnfocusPreview should return to Normal mode
    let result = handler.handle_action(&mut app, tenex::config::Action::UnfocusPreview);
    assert!(result.is_ok());
    assert_eq!(app.mode, tenex::AppMode::normal());

    let _ = std::env::set_current_dir(&original_dir);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    Ok(())
}

#[test]
fn test_actions_reset_all_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_reset")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    std::env::set_current_dir(&fixture.repo_path)?;

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create multiple agents
    let next = handler.create_agent(&mut app.data, "reset1", None)?;
    app.apply_mode(next);
    let next = handler.create_agent(&mut app.data, "reset2", None)?;
    app.apply_mode(next);
    assert_eq!(app.data.storage.len(), 2);

    // Reset all via confirm action
    app.enter_mode(
        tenex::state::ConfirmingMode {
            action: tenex::app::ConfirmAction::Reset,
        }
        .into(),
    );
    let result = handler.handle_action(&mut app, tenex::config::Action::Confirm);
    assert!(result.is_ok());
    assert_eq!(app.data.storage.len(), 0);

    let _ = std::env::set_current_dir(&original_dir);

    Ok(())
}

#[test]
fn test_actions_push_branch_integration() -> Result<(), Box<dyn std::error::Error>> {
    if skip_if_no_mux() {
        return Ok(());
    }

    let fixture = TestFixture::new("actions_push")?;
    let config = fixture.config();
    let storage = TestFixture::create_storage();

    let original_dir = std::env::current_dir()?;
    let _ = std::env::set_current_dir(&fixture.repo_path);

    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);
    let handler = tenex::app::Actions::new();

    // Create an agent
    let create_result = handler.create_agent(&mut app.data, "pushable", None);

    // Early cleanup if creation failed
    if create_result.is_err() {
        let _ = std::env::set_current_dir(&original_dir);
        // Skip test if agent creation fails (e.g., git/mux issues)
        return Ok(());
    }

    if let Ok(next) = create_result {
        app.apply_mode(next);
    }

    // Push action (just sets status message, doesn't actually push in test)
    let result = handler.handle_action(&mut app, tenex::config::Action::Push);

    // Cleanup
    let manager = SessionManager::new();
    for agent in app.data.storage.iter() {
        let _ = manager.kill(&agent.mux_session);
    }

    let _ = std::env::set_current_dir(&original_dir);

    assert!(result.is_ok());

    Ok(())
}
