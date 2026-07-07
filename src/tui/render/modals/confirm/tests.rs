use super::*;
use crate::app::Settings;
use crate::app::WorktreeConflictInfo;
use crate::config::Config;
use crate::update::UpdateInfo;
use crate::{Agent, App, agent::Storage};
use ratatui::{Terminal, backend::TestBackend};
use semver::Version;
use std::path::PathBuf;

fn app_with_agent() -> App {
    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    let agent = Agent::new(
        "render-agent".to_string(),
        "echo".to_string(),
        "tenex/render-agent".to_string(),
        PathBuf::from("/tmp"),
    );
    let id = agent.id;
    app.data.storage.add(agent);
    app.data.git_op.agent_id = Some(id);
    app.data.git_op.branch_name = "tenex/render-agent".to_string();
    app
}

#[test]
fn test_render_confirm_overlay_renders_content() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_confirm_overlay(frame, vec![Line::from("Testing confirm overlay")]);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_confirm_push_overlay_without_agent() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    terminal
        .draw(|frame| {
            render_confirm_push_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_confirm_push_overlay_with_agent() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = app_with_agent();

    terminal
        .draw(|frame| {
            render_confirm_push_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_confirm_push_for_pr_overlay() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = app_with_agent();
    app.data.git_op.base_branch = "main".to_string();

    terminal
        .draw(|frame| {
            render_confirm_push_for_pr_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_keyboard_remap_overlay() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_keyboard_remap_overlay(frame);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_update_prompt_overlay() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(2, 0, 0),
    };

    terminal
        .draw(|frame| {
            render_update_prompt_overlay(frame, &info);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_worktree_conflict_overlay_omits_optional_existing_branch_and_commit() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );

    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "wt".to_string(),
        prompt: None,
        branch: "wt-branch".to_string(),
        worktree_path: PathBuf::from("/tmp/wt"),
        repo_root: PathBuf::from("/tmp"),
        existing_branch: None,
        existing_commit: None,
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: None,
    });

    terminal
        .draw(|frame| {
            render_worktree_conflict_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}
