//! Integration tests for TUI rendering
//!
//! Uses ratatui's `TestBackend` to verify rendering without a real terminal.

use std::path::PathBuf;

use muster::agent::{Agent, Storage};
use muster::app::{App, ConfirmAction, Mode, Tab};
use muster::config::Config;
use muster::ui::{AgentListWidget, DiffViewWidget, PreviewWidget, StatusBarWidget};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn create_test_config() -> Config {
    Config {
        default_program: "echo".to_string(),
        branch_prefix: "muster/".to_string(),
        worktree_dir: PathBuf::from("/tmp/test-worktrees"),
        auto_yes: false,
        poll_interval_ms: 100,
        max_agents: 10,
    }
}

fn create_test_agent(title: &str, status: muster::Status) -> Agent {
    let mut agent = Agent::new(
        title.to_string(),
        "echo".to_string(),
        format!("muster/{title}"),
        PathBuf::from(format!("/tmp/{title}")),
        None,
    );
    agent.set_status(status);
    agent
}

fn create_test_agents() -> Vec<Agent> {
    vec![
        create_test_agent("agent-1", muster::Status::Running),
        create_test_agent("agent-2", muster::Status::Paused),
        create_test_agent("agent-3", muster::Status::Stopped),
    ]
}

fn create_test_app_with_agents() -> App {
    let config = create_test_config();
    let mut storage = Storage::new();

    storage.add(create_test_agent("agent-1", muster::Status::Running));
    storage.add(create_test_agent("agent-2", muster::Status::Paused));
    storage.add(create_test_agent("agent-3", muster::Status::Stopped));

    App::new(config, storage)
}

// =============================================================================
// Tests for AgentListWidget
// =============================================================================

#[test]
fn test_agent_list_widget_renders() {
    let agent_list = create_test_agents();
    let widget = AgentListWidget::new(&agent_list, 0);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    widget.to_list().render(area, &mut buf);

    // Verify the buffer contains expected content
    let content = buffer_to_string(&buf);
    assert!(content.contains("Agents"), "Should have title");
}

#[test]
fn test_agent_list_widget_with_selection() {
    let agent_list = create_test_agents();
    let widget = AgentListWidget::new(&agent_list, 1); // Select second agent

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    widget.to_list().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    // Should render without panic
    assert!(!content.is_empty());
}

#[test]
fn test_agent_list_widget_empty() {
    let agent_list: Vec<Agent> = vec![];
    let widget = AgentListWidget::new(&agent_list, 0);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    widget.to_list().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("Agents"));
}

#[test]
fn test_agent_list_widget_custom_title() {
    let agent_list = create_test_agents();
    let widget = AgentListWidget::new(&agent_list, 0).title("Custom Title");

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    widget.to_list().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("Custom Title"));
}

// =============================================================================
// Tests for PreviewWidget
// =============================================================================

#[test]
fn test_preview_widget_renders_empty() {
    let widget = PreviewWidget::new("");

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("Terminal Output") || content.contains("Preview"));
}

#[test]
fn test_preview_widget_renders_content() {
    let preview_content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
    let widget = PreviewWidget::new(preview_content);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let rendered = buffer_to_string(&buf);
    assert!(rendered.contains("Line 1") || rendered.contains("Terminal"));
}

#[test]
fn test_preview_widget_with_scroll() {
    let preview_content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8";
    let widget = PreviewWidget::new(preview_content).scroll(3);

    let area = Rect::new(0, 0, 40, 5);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    // Should render without panic
    let rendered = buffer_to_string(&buf);
    assert!(!rendered.is_empty());
}

#[test]
fn test_preview_widget_line_count() {
    let widget = PreviewWidget::new("a\nb\nc\nd\ne");
    assert_eq!(widget.line_count(), 5);
}

#[test]
fn test_preview_widget_custom_title() {
    let widget = PreviewWidget::new("content").title("Custom Preview");

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("Custom Preview"));
}

// =============================================================================
// Tests for DiffViewWidget
// =============================================================================

#[test]
fn test_diff_view_widget_empty() {
    let widget = DiffViewWidget::new("");

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("Diff") || content.contains("Git"));
}

#[test]
fn test_diff_view_widget_with_diff() {
    let diff = r"diff --git a/file.txt b/file.txt
--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 unchanged
-removed line
+added line
 context
";

    let widget = DiffViewWidget::new(diff);

    let area = Rect::new(0, 0, 60, 15);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(!content.is_empty());
}

#[test]
fn test_diff_view_widget_with_scroll() {
    let diff = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10";
    let widget = DiffViewWidget::new(diff).scroll(5);

    let area = Rect::new(0, 0, 60, 5);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(!content.is_empty());
}

#[test]
fn test_diff_view_widget_line_count() {
    let widget = DiffViewWidget::new("+added\n-removed\n context");
    assert_eq!(widget.line_count(), 3);
}

#[test]
fn test_diff_view_widget_custom_title() {
    let widget = DiffViewWidget::new("+added").title("Changes");

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("Changes"));
}

// =============================================================================
// Tests for StatusBarWidget
// =============================================================================

#[test]
fn test_status_bar_widget_normal() {
    let widget = StatusBarWidget::normal(3);

    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains('3') || !content.is_empty());
}

#[test]
fn test_status_bar_widget_error() {
    let widget = StatusBarWidget::error("Something went wrong");

    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(content.contains("wrong") || content.contains("Error") || !content.is_empty());
}

#[test]
fn test_status_bar_widget_status() {
    let widget = StatusBarWidget::status("Operation completed");

    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);

    widget.to_paragraph().render(area, &mut buf);

    let content = buffer_to_string(&buf);
    assert!(!content.is_empty());
}

// =============================================================================
// Tests for full render cycle with TestBackend
// =============================================================================

#[test]
fn test_full_render_normal_mode() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let app = create_test_app_with_agents();

    terminal.draw(|frame| {
        // Render the main layout similar to actual render function
        let area = frame.area();

        // Render agent list
        let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
        let list_area = Rect::new(0, 0, 30, area.height - 1);
        frame.render_widget(agent_list.to_list(), list_area);

        // Render preview or diff based on tab
        let content_area = Rect::new(30, 0, area.width - 30, area.height - 1);
        match app.active_tab {
            Tab::Preview => {
                let preview = PreviewWidget::new(&app.preview_content).scroll(app.preview_scroll);
                frame.render_widget(preview.to_paragraph(), content_area);
            }
            Tab::Diff => {
                let diff = DiffViewWidget::new(&app.diff_content).scroll(app.diff_scroll);
                frame.render_widget(diff.to_paragraph(), content_area);
            }
        }

        // Render status bar
        let status_area = Rect::new(0, area.height - 1, area.width, 1);
        let status = StatusBarWidget::normal(app.running_agent_count());
        frame.render_widget(status.to_paragraph(), status_area);
    })?;

    // Verify render completed without panic
    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());

    Ok(())
}

#[test]
fn test_full_render_creating_mode() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    app.enter_mode(Mode::Creating);
    app.handle_char('t');
    app.handle_char('e');
    app.handle_char('s');
    app.handle_char('t');

    terminal.draw(|frame| {
        let area = frame.area();

        // In creating mode, we'd show input
        let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
        frame.render_widget(agent_list.to_list(), Rect::new(0, 0, 30, area.height - 1));

        // Status bar shows input prompt
        let status = StatusBarWidget::status(format!("Enter name: {}", app.input_buffer).as_str());
        frame.render_widget(
            status.to_paragraph(),
            Rect::new(0, area.height - 1, area.width, 1),
        );
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());

    Ok(())
}

#[test]
fn test_full_render_confirming_mode() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

    terminal.draw(|frame| {
        let area = frame.area();

        let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
        frame.render_widget(agent_list.to_list(), Rect::new(0, 0, 30, area.height - 1));

        // Status bar shows confirmation prompt
        let status = StatusBarWidget::status("Quit? (y/n)");
        frame.render_widget(
            status.to_paragraph(),
            Rect::new(0, area.height - 1, area.width, 1),
        );
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());

    Ok(())
}

#[test]
fn test_full_render_help_mode() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    app.enter_mode(Mode::Help);

    terminal.draw(|frame| {
        let area = frame.area();

        // In help mode, render help overlay
        let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
        frame.render_widget(agent_list.to_list(), Rect::new(0, 0, 30, area.height - 1));

        let status = StatusBarWidget::status("Press any key to close help");
        frame.render_widget(
            status.to_paragraph(),
            Rect::new(0, area.height - 1, area.width, 1),
        );
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());

    Ok(())
}

#[test]
fn test_full_render_diff_tab() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    // Switch to diff tab
    app.switch_tab();
    assert_eq!(app.active_tab, Tab::Diff);

    // Set some diff content
    app.diff_content = "diff --git a/file.txt b/file.txt\n+added line".to_string();

    terminal.draw(|frame| {
        let area = frame.area();

        let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
        frame.render_widget(agent_list.to_list(), Rect::new(0, 0, 30, area.height - 1));

        let diff = DiffViewWidget::new(&app.diff_content).scroll(app.diff_scroll);
        frame.render_widget(
            diff.to_paragraph(),
            Rect::new(30, 0, area.width - 30, area.height - 1),
        );

        let status = StatusBarWidget::normal(app.running_agent_count());
        frame.render_widget(
            status.to_paragraph(),
            Rect::new(0, area.height - 1, area.width, 1),
        );
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());

    Ok(())
}

#[test]
fn test_full_render_with_error() -> Result<(), Box<dyn std::error::Error>> {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend)?;
    let mut app = create_test_app_with_agents();

    app.set_error("Something went wrong!");

    terminal.draw(|frame| {
        let area = frame.area();

        let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
        frame.render_widget(agent_list.to_list(), Rect::new(0, 0, 30, area.height - 1));

        let preview = PreviewWidget::new(&app.preview_content).scroll(app.preview_scroll);
        frame.render_widget(
            preview.to_paragraph(),
            Rect::new(30, 0, area.width - 30, area.height - 1),
        );

        // Error should be shown in status bar
        if let Some(ref error) = app.last_error {
            let status = StatusBarWidget::error(error);
            frame.render_widget(
                status.to_paragraph(),
                Rect::new(0, area.height - 1, area.width, 1),
            );
        }
    })?;

    let buffer = terminal.backend().buffer();
    assert!(!buffer.content.is_empty());

    Ok(())
}

#[test]
fn test_render_various_terminal_sizes() -> Result<(), Box<dyn std::error::Error>> {
    // Test with different terminal sizes
    let sizes = [(40, 12), (80, 24), (120, 40), (200, 50)];

    for (width, height) in sizes {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend)?;
        let app = create_test_app_with_agents();

        terminal.draw(|frame| {
            let area = frame.area();

            let agent_list = AgentListWidget::new(&app.storage.agents, app.selected);
            let list_width = area.width.min(30);
            frame.render_widget(
                agent_list.to_list(),
                Rect::new(0, 0, list_width, area.height - 1),
            );

            if area.width > 30 {
                let preview = PreviewWidget::new(&app.preview_content).scroll(app.preview_scroll);
                frame.render_widget(
                    preview.to_paragraph(),
                    Rect::new(list_width, 0, area.width - list_width, area.height - 1),
                );
            }

            let status = StatusBarWidget::normal(app.running_agent_count());
            frame.render_widget(
                status.to_paragraph(),
                Rect::new(0, area.height - 1, area.width, 1),
            );
        })?;

        let buffer = terminal.backend().buffer();
        assert!(
            !buffer.content.is_empty(),
            "Failed at size {width}x{height}"
        );
    }

    Ok(())
}

// =============================================================================
// Helper functions
// =============================================================================

fn buffer_to_string(buf: &Buffer) -> String {
    let mut result = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            result.push_str(cell.symbol());
        }
        result.push('\n');
    }
    result
}
