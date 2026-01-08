//! Integration tests for diff-focused visual selection + multi-line delete.

use crate::common::{TestFixture, git_command};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use std::path::Path;
use tenex::agent::Agent;
use tenex::app::{Actions, DiffLineMeta, Tab};
use tenex::state::DiffFocusedMode;

fn assert_git_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn git_commit_all(repo_path: &Path, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = git_command()
        .args(["add", "."])
        .current_dir(repo_path)
        .output()?;
    assert_git_success(&output, "git add failed");

    let output = git_command()
        .args(["commit", "-m", message])
        .current_dir(repo_path)
        .output()?;
    assert_git_success(&output, "git commit failed");

    Ok(())
}

fn collect_changed_view_indices(app: &tenex::App) -> Vec<usize> {
    let Some(model) = app.data.ui.diff_model.as_ref() else {
        return Vec::new();
    };

    app.data
        .ui
        .diff_line_meta
        .iter()
        .enumerate()
        .filter_map(|(view_idx, meta)| {
            let DiffLineMeta::Line {
                file_idx,
                hunk_idx,
                line_idx,
            } = *meta
            else {
                return None;
            };

            let origin = model.files[file_idx].hunks[hunk_idx].lines[line_idx].origin;
            matches!(origin, '+' | '-').then_some(view_idx)
        })
        .collect()
}

fn setup_app_with_repo(fixture: &TestFixture) -> tenex::App {
    let config = fixture.config();
    let storage = TestFixture::create_storage();
    let mut app = tenex::App::new(config, storage, tenex::app::Settings::default(), false);

    app.data.storage.add(Agent::new(
        "diff-test".to_string(),
        "echo".to_string(),
        fixture.session_name("diff"),
        fixture.repo_path.clone(),
        None,
    ));
    app.data.selected = 0;
    app.data.active_tab = Tab::Diff;

    app
}

#[test]
fn test_diff_visual_toggle_sets_and_clears_anchor() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("diff_visual_toggle")?;

    let file_path = fixture.repo_path.join("test.txt");
    std::fs::write(&file_path, "one\ntwo\n")?;
    git_commit_all(&fixture.repo_path, "add test file")?;

    std::fs::write(&file_path, "one\ntwo\nthree\n")?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    let changed = collect_changed_view_indices(&app);
    assert!(!changed.is_empty(), "expected at least one changed line");
    app.data.ui.diff_cursor = changed[0];

    app.enter_mode(DiffFocusedMode.into());

    // Some terminals report Shift+<char> as lowercase + SHIFT.
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('v'), KeyModifiers::SHIFT)?;
    assert_eq!(app.data.ui.diff_visual_anchor, Some(changed[0]));
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Visual selection started")
    );

    // Others report the shifted character directly.
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('V'), KeyModifiers::NONE)?;
    assert_eq!(app.data.ui.diff_visual_anchor, None);
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Visual selection cleared")
    );

    Ok(())
}

#[test]
fn test_diff_visual_delete_range_single_hunk_undo_redo() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("diff_visual_delete_single_hunk")?;

    let file_path = fixture.repo_path.join("test.txt");
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\nfour\n";
    std::fs::write(&file_path, original)?;
    git_commit_all(&fixture.repo_path, "add test file")?;

    std::fs::write(&file_path, modified)?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    let changed = collect_changed_view_indices(&app);
    assert!(
        changed.len() >= 2,
        "expected at least 2 changed lines for selection"
    );

    app.enter_mode(DiffFocusedMode.into());
    app.data.ui.diff_cursor = changed[0];
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('v'), KeyModifiers::SHIFT)?;

    app.data.ui.diff_cursor = changed[1];
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)?;

    assert_eq!(std::fs::read_to_string(&file_path)?, original);
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Deleted 2 diff lines")
    );
    assert!(app.data.ui.diff_visual_anchor.is_none());

    // Undo/redo should work end-to-end through the key router.
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL)?;
    assert_eq!(std::fs::read_to_string(&file_path)?, modified);
    assert_eq!(app.data.ui.status_message.as_deref(), Some("Undo"));

    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL)?;
    assert_eq!(std::fs::read_to_string(&file_path)?, original);
    assert_eq!(app.data.ui.status_message.as_deref(), Some("Redo"));

    Ok(())
}

#[test]
fn test_diff_visual_delete_range_spans_multiple_hunks_same_file()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("diff_visual_delete_multi_hunk")?;

    let file_path = fixture.repo_path.join("test.txt");
    let original_lines: Vec<String> = (1..=20).map(|i| format!("line{i:02}")).collect();
    let original = original_lines.join("\n") + "\n";
    std::fs::write(&file_path, &original)?;
    git_commit_all(&fixture.repo_path, "add multi-line test file")?;

    let mut modified_lines = original_lines;
    modified_lines[1] = "LINE02".to_string();
    modified_lines[17] = "LINE18".to_string();
    let modified = modified_lines.join("\n") + "\n";
    std::fs::write(&file_path, &modified)?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    let changed = collect_changed_view_indices(&app);
    assert!(
        changed.len() >= 4,
        "expected at least 4 changed lines across two hunks (2 deletions + 2 additions)"
    );

    let (Some(first), Some(last)) = (changed.iter().min(), changed.iter().max()) else {
        return Err(Box::new(std::io::Error::other(
            "expected at least one changed line",
        )));
    };
    let (first, last) = (*first, *last);

    app.enter_mode(DiffFocusedMode.into());
    app.data.ui.diff_cursor = first;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('V'), KeyModifiers::NONE)?;
    app.data.ui.diff_cursor = last;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)?;

    assert_eq!(std::fs::read_to_string(&file_path)?, original);
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Deleted 4 diff lines")
    );
    Ok(())
}

#[test]
fn test_diff_visual_delete_range_spans_multiple_files() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("diff_visual_delete_multi_file")?;

    let file1 = fixture.repo_path.join("one.txt");
    let file2 = fixture.repo_path.join("two.txt");
    let original1 = "a\nb\n";
    let original2 = "x\ny\n";
    std::fs::write(&file1, original1)?;
    std::fs::write(&file2, original2)?;
    git_commit_all(&fixture.repo_path, "add two files")?;

    let modified1 = "a\nb\nc\nd\n";
    let modified2 = "x\ny\nz\n";
    std::fs::write(&file1, modified1)?;
    std::fs::write(&file2, modified2)?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    let changed = collect_changed_view_indices(&app);
    assert!(
        changed.len() >= 3,
        "expected at least 3 changed lines across two files"
    );

    let (Some(first), Some(last)) = (changed.iter().min(), changed.iter().max()) else {
        return Err(Box::new(std::io::Error::other(
            "expected at least one changed line",
        )));
    };
    let (first, last) = (*first, *last);

    app.enter_mode(DiffFocusedMode.into());
    app.data.ui.diff_cursor = first;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('v'), KeyModifiers::SHIFT)?;
    app.data.ui.diff_cursor = last;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)?;

    assert_eq!(std::fs::read_to_string(&file1)?, original1);
    assert_eq!(std::fs::read_to_string(&file2)?, original2);
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Deleted 3 diff lines")
    );

    Ok(())
}

#[test]
fn test_diff_visual_delete_range_noops_without_changed_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("diff_visual_delete_noop")?;

    let file_path = fixture.repo_path.join("test.txt");
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    std::fs::write(&file_path, original)?;
    git_commit_all(&fixture.repo_path, "add test file")?;

    std::fs::write(&file_path, modified)?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    app.enter_mode(DiffFocusedMode.into());

    // Select an info line (not a diff line).
    app.data.ui.diff_cursor = 0;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('V'), KeyModifiers::NONE)?;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)?;

    assert_eq!(std::fs::read_to_string(&file_path)?, modified);
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Select a changed line (+/-) to delete")
    );
    assert_eq!(app.data.ui.diff_visual_anchor, Some(0));
    assert!(app.data.ui.diff_undo.is_empty());

    Ok(())
}

#[test]
fn test_diff_visual_delete_range_noops_for_deleted_file() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = TestFixture::new("diff_visual_delete_deleted_file")?;

    let file_path = fixture.repo_path.join("test.txt");
    let original = "keep\nme\n";
    std::fs::write(&file_path, original)?;
    git_commit_all(&fixture.repo_path, "add test file")?;

    std::fs::remove_file(&file_path)?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    let changed = collect_changed_view_indices(&app);
    assert!(!changed.is_empty(), "expected deleted-file diff lines");

    app.enter_mode(DiffFocusedMode.into());
    app.data.ui.diff_cursor = changed[0];
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('v'), KeyModifiers::SHIFT)?;
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)?;

    assert!(!file_path.exists());
    assert_eq!(
        app.data.ui.status_message.as_deref(),
        Some("Cannot delete a line from a deleted file (select hunk header to restore)")
    );
    Ok(())
}

#[test]
fn test_diff_cursor_cannot_enter_header_lines() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("diff_cursor_header_clamp")?;

    let file_path = fixture.repo_path.join("test.txt");
    std::fs::write(&file_path, "one\ntwo\n")?;
    git_commit_all(&fixture.repo_path, "add test file")?;

    std::fs::write(&file_path, "one\ntwo\nthree\n")?;

    let mut app = setup_app_with_repo(&fixture);
    let handler = Actions::new();
    handler.update_diff(&mut app)?;

    app.enter_mode(DiffFocusedMode.into());

    // Summary + helper lines should never be selectable.
    assert_eq!(app.data.ui.diff_cursor, 2);

    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Up, KeyModifiers::NONE)?;
    assert_eq!(app.data.ui.diff_cursor, 2);

    // Scroll-to-top should keep cursor on the first non-header line.
    tenex::action::dispatch_diff_focused_mode(&mut app, KeyCode::Char('g'), KeyModifiers::NONE)?;
    assert_eq!(app.data.ui.diff_cursor, 2);

    Ok(())
}
