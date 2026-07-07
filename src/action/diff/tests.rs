use super::*;
use crate::agent::{Agent, Storage};
use crate::app::Settings;
use crate::config::Config;
use crate::git::{DiffGenerator, Repository};
use git2::Signature;
use std::fs;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

fn create_test_data() -> (AppData, NamedTempFile) {
    let temp_file = NamedTempFile::new().expect("temp state file should be created");
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        AppData::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

fn init_test_repo_with_commit(contents: &str) -> (TempDir, Repository) {
    let temp_dir = TempDir::new().expect("temp repo dir should be created");
    let repo = Repository::init(temp_dir.path()).expect("repo should init");
    {
        let mut config = repo.config().expect("repo config should open");
        config
            .set_bool("core.autocrlf", false)
            .expect("autocrlf should set");
        config.set_str("core.eol", "lf").expect("eol should set");
    }

    let sig = Signature::now("Test", "test@test.com").expect("signature should build");
    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, contents).expect("file write should succeed");

    let mut index = repo.index().expect("repo index should open");
    index
        .add_path(std::path::Path::new("test.txt"))
        .expect("index add should succeed");
    index.write().expect("index write should succeed");

    let tree_id = index.write_tree().expect("index write tree should succeed");
    {
        let tree = repo.find_tree(tree_id).expect("repo should find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("repo commit should succeed");
    }

    (temp_dir, repo)
}

fn set_diff_view_from_repo(data: &mut AppData, repo: &Repository) {
    let model = DiffGenerator::new(repo)
        .uncommitted_model()
        .expect("uncommitted model should build");
    let (content, meta) = data.ui.build_diff_view(&model);
    data.ui.set_diff_view(content, meta);
    data.ui.diff_model = Some(model);
}

fn select_first_changed_line(data: &mut AppData) {
    let model = data.ui.diff_model.as_ref().expect("diff model set");
    let cursor = data
        .ui
        .diff_line_meta
        .iter()
        .position(|m| {
            if let DiffLineMeta::Line {
                file_idx,
                hunk_idx,
                line_idx,
            } = *m
            {
                let origin = model.files[file_idx].hunks[hunk_idx].lines[line_idx].origin;
                matches!(origin, '+' | '-')
            } else {
                false
            }
        })
        .expect("changed line present in diff view");
    data.ui.diff_cursor = cursor;
}

fn select_first_hunk_header(data: &mut AppData) {
    let cursor = data
        .ui
        .diff_line_meta
        .iter()
        .position(|m| matches!(m, DiffLineMeta::Hunk { .. }))
        .expect("hunk header present in diff view");
    data.ui.diff_cursor = cursor;
}

fn select_first_context_line(data: &mut AppData) {
    let model = data.ui.diff_model.as_ref().expect("diff model set");
    let cursor = data
        .ui
        .diff_line_meta
        .iter()
        .position(|m| {
            if let DiffLineMeta::Line {
                file_idx,
                hunk_idx,
                line_idx,
            } = *m
            {
                let origin = model.files[file_idx].hunks[hunk_idx].lines[line_idx].origin;
                origin == ' '
            } else {
                false
            }
        })
        .expect("context line present in diff view");
    data.ui.diff_cursor = cursor;
}

#[test]
fn test_unfocus_diff_action_clears_visual_anchor_and_returns_normal() {
    let temp_file = NamedTempFile::new().expect("temp state file should be created");
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);
    data.ui.diff_visual_anchor = Some(3);

    let next = UnfocusDiffAction
        .execute(DiffFocusedMode, &mut data)
        .expect("unfocus diff action should succeed");
    assert_eq!(next, AppMode::normal());
    assert_eq!(data.ui.diff_visual_anchor, None);
}

#[test]
fn test_diff_cursor_actions_respect_active_tab() {
    let (mut data, _temp) = create_test_data();
    data.ui.set_diff_content("line1\nline2\nline3\n");
    data.active_tab = Tab::Diff;

    assert_eq!(data.ui.diff_cursor, 0);
    assert_eq!(
        DiffCursorDownAction
            .execute(DiffFocusedMode, &mut data)
            .expect("cursor down should succeed"),
        DiffFocusedMode.into()
    );
    assert_eq!(data.ui.diff_cursor, 1);

    assert_eq!(
        DiffCursorUpAction
            .execute(DiffFocusedMode, &mut data)
            .expect("cursor up should succeed"),
        DiffFocusedMode.into()
    );
    assert_eq!(data.ui.diff_cursor, 0);

    data.active_tab = Tab::Preview;
    assert_eq!(
        DiffCursorDownAction
            .execute(DiffFocusedMode, &mut data)
            .expect("cursor down should succeed"),
        DiffFocusedMode.into()
    );
    assert_eq!(data.ui.diff_cursor, 0);
    assert_eq!(
        DiffCursorUpAction
            .execute(DiffFocusedMode, &mut data)
            .expect("cursor up should succeed"),
        DiffFocusedMode.into()
    );
    assert_eq!(data.ui.diff_cursor, 0);
}

#[test]
fn test_delete_line_and_undo_redo_on_unstaged_change() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);

    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_changed_line(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete line should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert_eq!(data.ui.diff_undo.len(), 1);
    assert!(data.ui.diff_redo.is_empty());
    assert!(data.ui.diff_force_refresh);
    assert_eq!(data.ui.status_message.as_deref(), Some("Deleted diff line"));

    DiffUndoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("undo should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        modified
    );
    assert!(data.ui.diff_force_refresh);
    assert_eq!(data.ui.status_message.as_deref(), Some("Undo"));

    DiffRedoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("redo should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert!(data.ui.diff_force_refresh);
    assert_eq!(data.ui.status_message.as_deref(), Some("Redo"));
}

#[test]
fn test_delete_line_action_propagates_apply_errors() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    let (repo_dir, repo) = init_test_repo_with_commit(original);
    fs::write(repo_dir.path().join("test.txt"), modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    let invalid_worktree = TempDir::new().expect("invalid worktree should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        invalid_worktree.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_changed_line(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect_err("expected git apply failure");
}

#[test]
fn test_delete_hunk_action_propagates_apply_errors() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    let (repo_dir, repo) = init_test_repo_with_commit(original);
    fs::write(repo_dir.path().join("test.txt"), modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    let invalid_worktree = TempDir::new().expect("invalid worktree should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        invalid_worktree.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_hunk_header(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect_err("expected git apply failure");
}

#[test]
fn test_delete_range_action_propagates_apply_errors() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    let (repo_dir, repo) = init_test_repo_with_commit(original);
    fs::write(repo_dir.path().join("test.txt"), modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    let invalid_worktree = TempDir::new().expect("invalid worktree should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        invalid_worktree.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_changed_line(&mut data);
    data.ui.diff_visual_anchor = Some(data.ui.diff_cursor);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect_err("expected git apply failure");
}

#[test]
fn test_undo_action_propagates_apply_errors() {
    let (mut data, _temp) = create_test_data();
    let invalid_worktree = TempDir::new().expect("invalid worktree should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        invalid_worktree.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    data.ui.diff_undo.push(DiffEdit {
        patch: "not a patch".to_string(),
        applied_reverse: false,
    });

    DiffUndoAction
        .execute(DiffFocusedMode, &mut data)
        .expect_err("expected undo to fail");
    assert_eq!(data.ui.diff_undo.len(), 1);
}

#[test]
fn test_redo_action_propagates_apply_errors() {
    let (mut data, _temp) = create_test_data();
    let invalid_worktree = TempDir::new().expect("invalid worktree should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        invalid_worktree.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    data.ui.diff_redo.push(DiffEdit {
        patch: "not a patch".to_string(),
        applied_reverse: false,
    });

    DiffRedoAction
        .execute(DiffFocusedMode, &mut data)
        .expect_err("expected redo to fail");
    assert_eq!(data.ui.diff_redo.len(), 1);
}

#[test]
fn test_delete_visual_range_and_undo_redo_on_unstaged_change() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\nfour\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);

    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);

    let model = data.ui.diff_model.as_ref().expect("diff model set");
    let changed_lines: Vec<usize> = data
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
        .collect();
    assert!(changed_lines.len() >= 2);

    data.ui.diff_visual_anchor = Some(changed_lines[0]);
    data.ui.diff_cursor = changed_lines[1];

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete range should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert_eq!(data.ui.diff_undo.len(), 1);
    assert!(data.ui.diff_redo.is_empty());
    assert!(data.ui.diff_force_refresh);
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Deleted 2 diff lines")
    );
    assert!(data.ui.diff_visual_anchor.is_none());

    DiffUndoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("undo should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        modified
    );
    assert!(data.ui.diff_force_refresh);
    assert_eq!(data.ui.status_message.as_deref(), Some("Undo"));

    DiffRedoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("redo should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert!(data.ui.diff_force_refresh);
    assert_eq!(data.ui.status_message.as_deref(), Some("Redo"));
}

#[test]
fn test_delete_line_requires_changed_line() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);

    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_context_line(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete line should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        modified
    );
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Select a changed line (+/-) to delete")
    );
    assert!(data.ui.diff_undo.is_empty());
}

#[test]
fn test_undo_redo_noop_when_empty() {
    let (mut data, _temp) = create_test_data();
    data.active_tab = Tab::Diff;

    DiffUndoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("undo should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        PathBuf::from("/tmp"),
    ));
    DiffUndoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("undo should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("Nothing to undo"));

    DiffRedoAction
        .execute(DiffFocusedMode, &mut data)
        .expect("redo should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("Nothing to redo"));
}

#[test]
fn test_delete_hunk_applies_with_index_on_staged_change() {
    let original = "a\nb\n";
    let modified = "a\nb\nc\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);

    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, modified).expect("write should succeed");
    {
        let mut index = repo.index().expect("repo index should open");
        index
            .add_path(std::path::Path::new("test.txt"))
            .expect("index add should succeed");
        index.write().expect("index write should succeed");
    }

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_hunk_header(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete hunk should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert_eq!(data.ui.status_message.as_deref(), Some("Deleted diff hunk"));
}

#[test]
fn test_restore_deleted_file_from_diff() {
    let original = "keep\nme\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);
    let file_path = temp_dir.path().join("test.txt");

    fs::remove_file(&file_path).expect("remove should succeed");
    {
        let mut index = repo.index().expect("repo index should open");
        index
            .remove_path(std::path::Path::new("test.txt"))
            .expect("remove should succeed");
        index.write().expect("index write should succeed");
    }

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_hunk_header(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("restore should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Restored deleted file")
    );
    assert_eq!(data.ui.diff_undo.len(), 1);
    assert!(data.ui.diff_undo[0].applied_reverse);
}

#[test]
fn test_restore_deleted_file_propagates_apply_errors() {
    let original = "keep\nme\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);
    let file_path = temp_dir.path().join("test.txt");

    fs::remove_file(&file_path).expect("remove should succeed");
    {
        let mut index = repo.index().expect("repo index should open");
        index
            .remove_path(std::path::Path::new("test.txt"))
            .expect("remove should succeed");
        index.write().expect("index write should succeed");
    }

    let (mut data, _temp) = create_test_data();
    let invalid_worktree = NamedTempFile::new().expect("invalid worktree should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        invalid_worktree.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_hunk_header(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect_err("expected git apply failure");
    assert!(data.ui.diff_undo.is_empty());
    assert!(data.ui.status_message.is_none());
}

#[test]
fn test_delete_line_noops_for_deleted_file() {
    let original = "keep\nme\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);
    let file_path = temp_dir.path().join("test.txt");

    fs::remove_file(&file_path).expect("remove should succeed");
    {
        let mut index = repo.index().expect("repo index should open");
        index
            .remove_path(std::path::Path::new("test.txt"))
            .expect("remove should succeed");
        index.write().expect("index write should succeed");
    }

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_changed_line(&mut data);

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete line should succeed");
    assert!(!file_path.exists());
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Cannot delete a line from a deleted file (select hunk header to restore)")
    );
    assert!(data.ui.diff_undo.is_empty());
}

#[test]
fn test_run_git_apply_errors_on_invalid_patch() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let err = run_git_apply(temp_dir.path(), "not a patch", false, false)
        .expect_err("invalid patch should fail");
    assert!(format!("{err:#}").contains("git apply failed"));
}

#[test]
fn test_run_git_apply_errors_on_invalid_patch_with_index_suffix() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let err = run_git_apply(temp_dir.path(), "not a patch", false, true)
        .expect_err("invalid patch should fail");
    assert!(format!("{err:#}").contains("git apply failed (--index)"));
}

#[test]
fn test_apply_git_patch_includes_index_error_context_when_retry_fails() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let err = apply_git_patch(temp_dir.path(), "not a patch", false)
        .expect_err("invalid patch should fail");
    assert!(format!("{err:#}").contains("git apply failed (index)"));
}

fn spawn_err(_cmd: &mut Command) -> std::io::Result<Child> {
    Err(std::io::Error::other("boom"))
}

fn spawn_true(_cmd: &mut Command) -> std::io::Result<Child> {
    let mut cmd = Command::new("true");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn()
}

fn take_stdin_none(_child: &mut Child) -> Option<ChildStdin> {
    None
}

fn write_all_err(_stdin: &mut ChildStdin, _bytes: &[u8]) -> std::io::Result<()> {
    Err(std::io::Error::other("boom"))
}

fn wait_with_output_err(_child: Child) -> std::io::Result<Output> {
    Err(std::io::Error::other("boom"))
}

#[test]
fn test_run_git_apply_with_deps_propagates_spawn_errors() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let deps = GitApplyDeps {
        spawn: spawn_err,
        ..GitApplyDeps::production()
    };

    let err = run_git_apply_with_deps(temp_dir.path(), "not a patch", false, false, deps)
        .expect_err("spawn errors should fail");
    assert!(format!("{err:#}").contains("Failed to spawn git apply"));
}

#[test]
fn test_run_git_apply_with_deps_errors_when_stdin_missing() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let deps = GitApplyDeps {
        spawn: spawn_true,
        take_stdin: take_stdin_none,
        ..GitApplyDeps::production()
    };

    let err = run_git_apply_with_deps(temp_dir.path(), "not a patch", false, false, deps)
        .expect_err("missing stdin should fail");
    assert!(format!("{err:#}").contains("Failed to open stdin for git apply"));
}

#[test]
fn test_run_git_apply_with_deps_propagates_write_errors() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let deps = GitApplyDeps {
        spawn: spawn_true,
        write_all: write_all_err,
        ..GitApplyDeps::production()
    };

    let err = run_git_apply_with_deps(temp_dir.path(), "not a patch", false, false, deps)
        .expect_err("write errors should fail");
    assert!(format!("{err:#}").contains("Failed to write patch to git apply stdin"));
}

#[test]
fn test_run_git_apply_with_deps_propagates_wait_errors() {
    let (temp_dir, _repo) = init_test_repo_with_commit("a\n");
    let deps = GitApplyDeps {
        spawn: spawn_true,
        wait_with_output: wait_with_output_err,
        ..GitApplyDeps::production()
    };

    let err = run_git_apply_with_deps(temp_dir.path(), "not a patch", false, false, deps)
        .expect_err("wait errors should fail");
    assert!(format!("{err:#}").contains("Failed to wait for git apply"));
}

#[test]
fn test_diff_helpers_cover_edge_cases() {
    assert_eq!(diff_path(&PathBuf::from(r"dir\file.txt")), "dir/file.txt");
    assert_eq!(hunk_header_suffix("not a header"), "");

    let line = DiffHunkLine {
        origin: '\\',
        content: "no newline at end of file".to_string(),
        old_lineno: None,
        new_lineno: None,
    };
    assert_eq!(raw_hunk_line(&line), "\\no newline at end of file");

    let line = DiffHunkLine {
        origin: '?',
        content: "content".to_string(),
        old_lineno: None,
        new_lineno: None,
    };
    assert_eq!(raw_hunk_line(&line), "content");
}

#[test]
fn test_toggle_visual_action_returns_early_when_diff_tab_inactive() {
    let (mut data, _temp) = create_test_data();
    data.active_tab = Tab::Preview;
    data.ui.diff_visual_anchor = Some(3);

    assert_eq!(
        DiffToggleVisualAction
            .execute(DiffFocusedMode, &mut data)
            .expect("toggle should succeed"),
        DiffFocusedMode.into()
    );
    assert_eq!(data.ui.diff_visual_anchor, Some(3));
    assert_eq!(data.ui.status_message, None);
}

#[test]
fn test_toggle_visual_action_sets_and_clears_anchor_when_diff_tab_active() {
    let (mut data, _temp) = create_test_data();
    data.active_tab = Tab::Diff;
    data.ui.diff_cursor = 4;

    DiffToggleVisualAction
        .execute(DiffFocusedMode, &mut data)
        .expect("toggle should succeed");
    assert_eq!(data.ui.diff_visual_anchor, Some(4));
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Visual selection started")
    );

    DiffToggleVisualAction
        .execute(DiffFocusedMode, &mut data)
        .expect("toggle should succeed");
    assert_eq!(data.ui.diff_visual_anchor, None);
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Visual selection cleared")
    );
}

#[test]
fn test_delete_selected_range_returns_early_for_missing_prerequisites() {
    let (mut data, _temp) = create_test_data();
    data.ui.diff_visual_anchor = Some(0);
    data.active_tab = Tab::Preview;

    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete range should succeed");
    assert_eq!(data.ui.diff_visual_anchor, Some(0));
    assert_eq!(data.ui.status_message, None);

    data.active_tab = Tab::Diff;
    data.ui.diff_visual_anchor = None;
    DiffDeleteLineAction
        .execute(DiffFocusedMode, &mut data)
        .expect("delete range should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

    data.ui.status_message = None;
    data.ui.diff_visual_anchor = Some(0);
    delete_selected_range(&mut data, 0).expect("delete range should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

    let temp_dir = TempDir::new().expect("temp dir should create");
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.ui.status_message = None;
    delete_selected_range(&mut data, 0).expect("delete range should succeed");
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Diff not loaded yet")
    );
}

#[test]
fn test_delete_selected_range_uses_anchor_after_cursor_and_deletes_single_line() {
    let original = "one\ntwo\n";
    let modified = "one\ntwo\nthree\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);

    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, modified).expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);

    select_first_changed_line(&mut data);
    let changed_idx = data.ui.diff_cursor;
    select_first_context_line(&mut data);
    assert!(changed_idx > data.ui.diff_cursor);

    data.ui.diff_visual_anchor = Some(changed_idx);
    delete_selected_range(&mut data, changed_idx).expect("delete range should succeed");

    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        original
    );
    assert_eq!(data.ui.diff_visual_anchor, None);
    assert_eq!(data.ui.status_message.as_deref(), Some("Deleted diff line"));
    assert_eq!(data.ui.diff_undo.len(), 1);
    assert!(data.ui.diff_redo.is_empty());
}

#[test]
fn test_delete_selected_range_noops_for_deleted_file_lines() {
    let original = "keep\nme\n";
    let (temp_dir, repo) = init_test_repo_with_commit(original);
    let file_path = temp_dir.path().join("test.txt");
    fs::remove_file(&file_path).expect("remove should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);
    select_first_changed_line(&mut data);
    let cursor = data.ui.diff_cursor;
    data.ui.diff_visual_anchor = Some(cursor);

    delete_selected_range(&mut data, cursor).expect("delete range should succeed");

    assert!(!file_path.exists());
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Cannot delete a line from a deleted file (select hunk header to restore)")
    );
    assert!(data.ui.diff_undo.is_empty());
}

#[test]
fn test_delete_selected_range_skips_invalid_meta_entries() {
    let (temp_dir, repo) = init_test_repo_with_commit("one\n");
    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, "one\ntwo\n").expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);

    data.ui.diff_line_meta = vec![
        DiffLineMeta::Line {
            file_idx: 123,
            hunk_idx: 0,
            line_idx: 0,
        },
        DiffLineMeta::Line {
            file_idx: 0,
            hunk_idx: 123,
            line_idx: 0,
        },
        DiffLineMeta::Line {
            file_idx: 0,
            hunk_idx: 0,
            line_idx: 123,
        },
    ];
    data.ui.diff_cursor = 2;
    data.ui.diff_visual_anchor = Some(0);

    delete_selected_range(&mut data, 0).expect("delete range should succeed");
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Select a changed line (+/-) to delete")
    );
}

#[test]
fn test_delete_selected_line_returns_early_when_hunk_or_line_missing_but_file_exists() {
    let (temp_dir, repo) = init_test_repo_with_commit("one\n");
    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, "one\ntwo\n").expect("write should succeed");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.active_tab = Tab::Diff;
    set_diff_view_from_repo(&mut data, &repo);

    let model = data.ui.diff_model.as_ref().expect("diff model set");
    assert!(!model.files.is_empty());

    data.ui.diff_line_meta = vec![DiffLineMeta::Line {
        file_idx: 0,
        hunk_idx: 123,
        line_idx: 0,
    }];
    data.ui.diff_cursor = 0;
    delete_selected_line(&mut data).expect("delete selected line should succeed");
    assert_eq!(data.ui.status_message, None);

    data.ui.diff_line_meta = vec![DiffLineMeta::Line {
        file_idx: 0,
        hunk_idx: 0,
        line_idx: 123,
    }];
    delete_selected_line(&mut data).expect("delete selected line should succeed");
    assert_eq!(data.ui.status_message, None);
}

#[test]
fn test_delete_selected_hunk_covers_more_missing_prereq_and_meta_branches() {
    let (mut data, _temp) = create_test_data();
    data.active_tab = Tab::Diff;
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

    let (temp_dir, repo) = init_test_repo_with_commit("one\n");
    let file_path = temp_dir.path().join("test.txt");
    fs::write(&file_path, "one\ntwo\n").expect("write should succeed");

    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    data.ui.status_message = None;
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Diff not loaded yet")
    );

    set_diff_view_from_repo(&mut data, &repo);
    data.ui.status_message = None;
    data.ui.diff_line_meta.clear();
    data.ui.diff_cursor = 0;
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
    assert_eq!(data.ui.status_message, None);

    data.ui.diff_line_meta = vec![DiffLineMeta::Hunk {
        file_idx: 0,
        hunk_idx: 123,
    }];
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
    assert_eq!(data.ui.status_message, None);

    set_diff_view_from_repo(&mut data, &repo);
    select_first_changed_line(&mut data);
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
    assert_eq!(
        fs::read_to_string(&file_path).expect("read should succeed"),
        "one\n"
    );
    assert_eq!(data.ui.status_message.as_deref(), Some("Deleted diff hunk"));
}

#[test]
fn test_delete_selected_line_and_hunk_return_early_on_missing_meta() {
    let (temp_dir, repo) = init_test_repo_with_commit("one\n");

    let (mut data, _temp) = create_test_data();
    data.active_tab = Tab::Diff;
    delete_selected_line(&mut data).expect("delete selected line should succeed");
    assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));
    delete_selected_line(&mut data).expect("delete selected line should succeed");
    assert_eq!(
        data.ui.status_message.as_deref(),
        Some("Diff not loaded yet")
    );

    set_diff_view_from_repo(&mut data, &repo);
    data.ui.diff_line_meta.clear();
    data.ui.diff_cursor = 0;
    delete_selected_line(&mut data).expect("delete selected line should succeed");

    data.ui.diff_line_meta = vec![DiffLineMeta::Info];
    delete_selected_line(&mut data).expect("delete selected line should succeed");

    data.ui.diff_line_meta = vec![DiffLineMeta::Line {
        file_idx: 123,
        hunk_idx: 0,
        line_idx: 0,
    }];
    delete_selected_line(&mut data).expect("delete selected line should succeed");

    data.ui.diff_line_meta = vec![DiffLineMeta::Line {
        file_idx: 0,
        hunk_idx: 123,
        line_idx: 0,
    }];
    delete_selected_line(&mut data).expect("delete selected line should succeed");

    data.ui.diff_line_meta = vec![DiffLineMeta::Line {
        file_idx: 0,
        hunk_idx: 0,
        line_idx: 123,
    }];
    delete_selected_line(&mut data).expect("delete selected line should succeed");

    data.ui.diff_line_meta = vec![DiffLineMeta::File { file_idx: 0 }];
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");

    data.ui.diff_line_meta = vec![DiffLineMeta::Hunk {
        file_idx: 0,
        hunk_idx: 123,
    }];
    delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
}

#[test]
fn test_hunk_header_suffix_returns_empty_when_header_has_no_suffix() {
    assert_eq!(hunk_header_suffix("@@ -1,1 +1,1"), "");
}

#[test]
fn test_undo_redo_returns_error_and_preserves_stacks_when_patch_fails() {
    let (temp_dir, _repo) = init_test_repo_with_commit("one\n");

    let (mut data, _temp) = create_test_data();
    data.storage.add(Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "feature/root".to_string(),
        temp_dir.path().to_path_buf(),
    ));

    data.active_tab = Tab::Preview;
    undo_redo(&mut data, true).expect("undo redo should succeed");
    assert_eq!(data.ui.status_message, None);

    data.active_tab = Tab::Diff;
    data.ui.diff_undo.push(DiffEdit {
        patch: "not a patch".to_string(),
        applied_reverse: false,
    });
    assert!(undo_redo(&mut data, true).is_err());
    assert_eq!(data.ui.diff_undo.len(), 1);
    assert!(data.ui.diff_redo.is_empty());

    data.ui.diff_redo.push(DiffEdit {
        patch: "not a patch".to_string(),
        applied_reverse: false,
    });
    assert!(undo_redo(&mut data, false).is_err());
    assert_eq!(data.ui.diff_redo.len(), 1);
}

#[test]
fn test_build_hunk_revert_patch_includes_special_origins_and_skips_unknown() {
    let (file, hunk) = added_file_and_base_hunk();
    let patch = build_hunk_revert_patch(&file, &hunk);
    assert!(patch.contains("@@ -1,2 +1,1 @@"));
    assert!(patch.contains(" context"));
    assert!(patch.contains("-added"));
    assert!(patch.contains("+removed"));
    assert!(patch.contains("\\no newline at end of file"));
    assert!(!patch.contains("unknown"));
}

#[test]
fn test_build_line_revert_patch_covers_added_and_deleted_origins() {
    let (file, hunk) = added_file_and_base_hunk();
    let line_patch = build_line_revert_patch(
        &file,
        &DiffHunk {
            lines: vec![
                DiffHunkLine {
                    origin: '+',
                    content: "target".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '+',
                    content: "keep".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '\\',
                    content: "no newline at end of file".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '?',
                    content: "ignored".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
            ],
            ..hunk.clone()
        },
        0,
    );
    assert!(line_patch.contains("-target"));
    assert!(line_patch.contains(" keep"));
    assert!(line_patch.contains("\\no newline at end of file"));

    let deleted_hunk = DiffHunk {
        lines: deleted_lines(),
        ..hunk
    };
    let deleted_patch = build_line_revert_patch(&file, &deleted_hunk, 1);
    assert!(deleted_patch.contains("+removed"));
    assert!(!deleted_patch.contains("unselected"));
}

#[test]
fn test_build_multi_line_revert_patch_keeps_context_and_skips_unknown() {
    let (file, hunk) = added_file_and_base_hunk();
    let multi_patch = build_multi_line_revert_patch(
        &file,
        &DiffHunk {
            lines: vec![
                DiffHunkLine {
                    origin: '+',
                    content: "selected".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '-',
                    content: "removed-selected".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '-',
                    content: "removed-unselected".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '+',
                    content: "unselected".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '\\',
                    content: "no newline at end of file".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
                DiffHunkLine {
                    origin: '?',
                    content: "unknown".to_string(),
                    old_lineno: None,
                    new_lineno: None,
                },
            ],
            ..hunk
        },
        &[0, 1],
    );
    assert!(multi_patch.contains("-selected"));
    assert!(multi_patch.contains("+removed-selected"));
    assert!(multi_patch.contains(" unselected"));
    assert!(multi_patch.contains("\\no newline at end of file"));
    assert!(!multi_patch.contains("removed-unselected"));
    assert!(!multi_patch.contains("unknown"));
}

fn added_file_and_base_hunk() -> (DiffFile, DiffHunk) {
    let file = DiffFile {
        path: PathBuf::from("file.txt"),
        status: FileStatus::Added,
        meta: vec!["diff --git a/file.txt b/file.txt".to_string()],
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
    };

    let hunk = DiffHunk {
        header: "@@ -1,1 +1,2 @@".to_string(),
        old_start: 1,
        old_lines: 1,
        new_start: 1,
        new_lines: 2,
        lines: vec![
            DiffHunkLine {
                origin: ' ',
                content: "context".to_string(),
                old_lineno: None,
                new_lineno: None,
            },
            DiffHunkLine {
                origin: '+',
                content: "added".to_string(),
                old_lineno: None,
                new_lineno: None,
            },
            DiffHunkLine {
                origin: '-',
                content: "removed".to_string(),
                old_lineno: None,
                new_lineno: None,
            },
            DiffHunkLine {
                origin: '\\',
                content: "no newline at end of file".to_string(),
                old_lineno: None,
                new_lineno: None,
            },
            DiffHunkLine {
                origin: '?',
                content: "unknown".to_string(),
                old_lineno: None,
                new_lineno: None,
            },
        ],
    };

    (file, hunk)
}

fn deleted_lines() -> Vec<DiffHunkLine> {
    vec![
        DiffHunkLine {
            origin: ' ',
            content: "context".to_string(),
            old_lineno: None,
            new_lineno: None,
        },
        DiffHunkLine {
            origin: '-',
            content: "removed".to_string(),
            old_lineno: None,
            new_lineno: None,
        },
        DiffHunkLine {
            origin: '-',
            content: "unselected".to_string(),
            old_lineno: None,
            new_lineno: None,
        },
    ]
}
