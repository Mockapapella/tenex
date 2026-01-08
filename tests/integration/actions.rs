//! Tests for Actions handler with real operations

use crate::common::{DirGuard, TestFixture, skip_if_no_mux};
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

    // Select the agent
    app.select_next();

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
    app.select_next();

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
    app.select_next();

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
    app.select_next();

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
    app.select_next();

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

    app.select_next();

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
