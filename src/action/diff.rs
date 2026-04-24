use crate::action::ValidIn;
use crate::app::{AppData, DiffEdit, DiffLineMeta, Tab};
use crate::git::{DiffFile, DiffHunk, DiffHunkLine, FileStatus};
use crate::state::{AppMode, DiffFocusedMode};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Output, Stdio};

/// Diff-focused action: exit diff focus.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnfocusDiffAction;

impl ValidIn<DiffFocusedMode> for UnfocusDiffAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        #[cfg(any(test, coverage))]
        super::force_infallible_action_error_if_enabled_for_tests()?;
        app_data.ui.diff_visual_anchor = None;
        Ok(AppMode::normal())
    }
}

/// Normal-mode action: move the diff cursor up.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffCursorUpAction;

impl ValidIn<DiffFocusedMode> for DiffCursorUpAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        #[cfg(any(test, coverage))]
        super::force_infallible_action_error_if_enabled_for_tests()?;
        if app_data.active_tab == Tab::Diff {
            app_data.ui.diff_cursor_up(1);
        }
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: move the diff cursor down.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffCursorDownAction;

impl ValidIn<DiffFocusedMode> for DiffCursorDownAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        #[cfg(any(test, coverage))]
        super::force_infallible_action_error_if_enabled_for_tests()?;
        if app_data.active_tab == Tab::Diff {
            app_data.ui.diff_cursor_down(1);
        }
        Ok(DiffFocusedMode.into())
    }
}

/// Diff-focused action: toggle visual selection.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffToggleVisualAction;

impl ValidIn<DiffFocusedMode> for DiffToggleVisualAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.active_tab != Tab::Diff {
            return Ok(DiffFocusedMode.into());
        }

        if app_data.ui.diff_visual_anchor.is_some() {
            app_data.ui.diff_visual_anchor = None;
            app_data.set_status("Visual selection cleared");
        } else {
            app_data.ui.diff_visual_anchor = Some(app_data.ui.diff_cursor);
            app_data.set_status("Visual selection started");
        }

        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: delete (revert) the selected diff line/hunk.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffDeleteLineAction;

impl ValidIn<DiffFocusedMode> for DiffDeleteLineAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.ui.diff_visual_anchor.is_some() {
            delete_selected_range(app_data)?;
            return Ok(DiffFocusedMode.into());
        }

        let meta = app_data
            .ui
            .diff_line_meta
            .get(app_data.ui.diff_cursor)
            .copied();

        if matches!(meta, Some(DiffLineMeta::Hunk { .. })) {
            delete_selected_hunk(app_data)?;
        } else {
            delete_selected_line(app_data)?;
        }
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: undo the last diff edit.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffUndoAction;

impl ValidIn<DiffFocusedMode> for DiffUndoAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        undo_redo(app_data, true)?;
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: redo the last undone diff edit.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffRedoAction;

impl ValidIn<DiffFocusedMode> for DiffRedoAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        undo_redo(app_data, false)?;
        Ok(DiffFocusedMode.into())
    }
}

fn delete_selected_range(app_data: &mut AppData) -> Result<()> {
    if app_data.active_tab != Tab::Diff {
        return Ok(());
    }

    let Some(anchor) = app_data.ui.diff_visual_anchor else {
        return Ok(());
    };

    let Some(agent) = app_data.selected_agent() else {
        app_data.set_status("No agent selected");
        return Ok(());
    };
    let worktree_path = agent.worktree_path.clone();

    let Some(model) = app_data.ui.diff_model.clone() else {
        app_data.set_status("Diff not loaded yet");
        return Ok(());
    };

    let cursor = app_data.ui.diff_cursor;
    let (start, end) = if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };

    let mut selected_lines_by_hunk: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    let mut saw_deleted_file = false;

    for view_idx in start..=end {
        let Some(DiffLineMeta::Line {
            file_idx,
            hunk_idx,
            line_idx,
        }) = app_data.ui.diff_line_meta.get(view_idx).copied()
        else {
            continue;
        };

        let Some(file) = model.files.get(file_idx) else {
            continue;
        };
        if file.status == FileStatus::Deleted {
            saw_deleted_file = true;
            continue;
        }

        let Some(hunk) = file.hunks.get(hunk_idx) else {
            continue;
        };
        let Some(line) = hunk.lines.get(line_idx) else {
            continue;
        };

        if !matches!(line.origin, '+' | '-') {
            continue;
        }

        selected_lines_by_hunk
            .entry((file_idx, hunk_idx))
            .or_default()
            .push(line_idx);
    }

    if selected_lines_by_hunk.is_empty() {
        if saw_deleted_file {
            app_data.set_status(
                "Cannot delete a line from a deleted file (select hunk header to restore)",
            );
        } else {
            app_data.set_status("Select a changed line (+/-) to delete");
        }
        return Ok(());
    }

    let mut patch = String::new();
    let mut selected_line_count = 0usize;
    for ((file_idx, hunk_idx), mut line_indices) in selected_lines_by_hunk {
        line_indices.sort_unstable();
        line_indices.dedup();
        selected_line_count = selected_line_count.saturating_add(line_indices.len());

        let file = &model.files[file_idx];
        let hunk = &file.hunks[hunk_idx];

        patch.push_str(&build_multi_line_revert_patch(file, hunk, &line_indices));
    }

    apply_git_patch(&worktree_path, &patch, false)?;

    app_data.ui.diff_undo.push(DiffEdit {
        patch,
        applied_reverse: false,
    });
    app_data.ui.diff_redo.clear();
    app_data.ui.diff_force_refresh = true;
    app_data.ui.diff_visual_anchor = None;

    if selected_line_count == 1 {
        app_data.set_status("Deleted diff line");
    } else {
        app_data.set_status(format!("Deleted {selected_line_count} diff lines"));
    }

    Ok(())
}

fn delete_selected_line(app_data: &mut AppData) -> Result<()> {
    if app_data.active_tab != Tab::Diff {
        return Ok(());
    }

    let Some(agent) = app_data.selected_agent() else {
        app_data.set_status("No agent selected");
        return Ok(());
    };
    let worktree_path = agent.worktree_path.clone();

    let Some(model) = app_data.ui.diff_model.clone() else {
        app_data.set_status("Diff not loaded yet");
        return Ok(());
    };

    let Some(meta) = app_data
        .ui
        .diff_line_meta
        .get(app_data.ui.diff_cursor)
        .copied()
    else {
        return Ok(());
    };

    let DiffLineMeta::Line {
        file_idx,
        hunk_idx,
        line_idx,
    } = meta
    else {
        return Ok(());
    };

    let Some(file) = model.files.get(file_idx) else {
        return Ok(());
    };

    if file.status == FileStatus::Deleted {
        app_data
            .set_status("Cannot delete a line from a deleted file (select hunk header to restore)");
        return Ok(());
    }

    let Some(hunk) = file.hunks.get(hunk_idx) else {
        return Ok(());
    };
    let Some(line) = hunk.lines.get(line_idx) else {
        return Ok(());
    };

    if !matches!(line.origin, '+' | '-') {
        app_data.set_status("Select a changed line (+/-) to delete");
        return Ok(());
    }

    let patch = build_line_revert_patch(file, hunk, line_idx);
    apply_git_patch(&worktree_path, &patch, false)?;

    app_data.ui.diff_undo.push(DiffEdit {
        patch,
        applied_reverse: false,
    });
    app_data.ui.diff_redo.clear();
    app_data.ui.diff_force_refresh = true;
    app_data.set_status("Deleted diff line");
    Ok(())
}

fn delete_selected_hunk(app_data: &mut AppData) -> Result<()> {
    if app_data.active_tab != Tab::Diff {
        return Ok(());
    }

    let Some(agent) = app_data.selected_agent() else {
        app_data.set_status("No agent selected");
        return Ok(());
    };
    let worktree_path = agent.worktree_path.clone();

    let Some(model) = app_data.ui.diff_model.clone() else {
        app_data.set_status("Diff not loaded yet");
        return Ok(());
    };

    let Some(meta) = app_data
        .ui
        .diff_line_meta
        .get(app_data.ui.diff_cursor)
        .copied()
    else {
        return Ok(());
    };

    let (DiffLineMeta::Hunk { file_idx, hunk_idx }
    | DiffLineMeta::Line {
        file_idx, hunk_idx, ..
    }) = meta
    else {
        return Ok(());
    };

    let Some(file) = model.files.get(file_idx) else {
        return Ok(());
    };

    if file.status == FileStatus::Deleted {
        // Restore the entire file by reverse-applying the deletion patch.
        let patch = build_file_patch(file);
        apply_git_patch(&worktree_path, &patch, true)?;

        app_data.ui.diff_undo.push(DiffEdit {
            patch,
            applied_reverse: true,
        });
        app_data.ui.diff_redo.clear();
        app_data.ui.diff_force_refresh = true;
        app_data.set_status("Restored deleted file");
        return Ok(());
    }

    let Some(hunk) = file.hunks.get(hunk_idx) else {
        return Ok(());
    };

    let patch = build_hunk_revert_patch(file, hunk);
    apply_git_patch(&worktree_path, &patch, false)?;

    app_data.ui.diff_undo.push(DiffEdit {
        patch,
        applied_reverse: false,
    });
    app_data.ui.diff_redo.clear();
    app_data.ui.diff_force_refresh = true;
    app_data.set_status("Deleted diff hunk");
    Ok(())
}

fn undo_redo(app_data: &mut AppData, undo: bool) -> Result<()> {
    if app_data.active_tab != Tab::Diff {
        return Ok(());
    }

    let Some(agent) = app_data.selected_agent() else {
        app_data.set_status("No agent selected");
        return Ok(());
    };
    let worktree_path = agent.worktree_path.clone();

    if undo {
        let Some(edit) = app_data.ui.diff_undo.pop() else {
            app_data.set_status("Nothing to undo");
            return Ok(());
        };

        if let Err(err) = apply_git_patch(&worktree_path, &edit.patch, !edit.applied_reverse) {
            app_data.ui.diff_undo.push(edit);
            return Err(err);
        }

        app_data.ui.diff_redo.push(edit);
        app_data.ui.diff_force_refresh = true;
        app_data.set_status("Undo");
    } else {
        let Some(edit) = app_data.ui.diff_redo.pop() else {
            app_data.set_status("Nothing to redo");
            return Ok(());
        };

        if let Err(err) = apply_git_patch(&worktree_path, &edit.patch, edit.applied_reverse) {
            app_data.ui.diff_redo.push(edit);
            return Err(err);
        }

        app_data.ui.diff_undo.push(edit);
        app_data.ui.diff_force_refresh = true;
        app_data.set_status("Redo");
    }

    Ok(())
}

fn build_file_patch(file: &DiffFile) -> String {
    let mut patch = String::new();

    for line in &file.meta {
        let _ = writeln!(patch, "{line}");
    }

    for hunk in &file.hunks {
        let _ = writeln!(patch, "{}", hunk.header);
        for line in &hunk.lines {
            let raw = raw_hunk_line(line);
            let _ = writeln!(patch, "{raw}");
        }
    }

    patch
}

fn build_hunk_revert_patch(file: &DiffFile, hunk: &DiffHunk) -> String {
    let file_path = diff_path(&file.path);
    let mut patch = String::new();
    let _ = writeln!(patch, "diff --git a/{file_path} b/{file_path}");
    let _ = writeln!(patch, "--- a/{file_path}");
    let _ = writeln!(patch, "+++ b/{file_path}");

    let suffix = hunk_header_suffix(&hunk.header);

    let old_start = hunk.new_start;
    let old_count = u32::try_from(hunk_old_count_from_current(hunk)).unwrap_or(0);
    let new_start = if matches!(file.status, FileStatus::Added | FileStatus::Untracked) {
        old_start
    } else {
        hunk.old_start
    };
    let new_count = if matches!(file.status, FileStatus::Added | FileStatus::Untracked) {
        u32::try_from(hunk_new_count_after_revert(hunk)).unwrap_or(0)
    } else {
        hunk.old_lines
    };

    let _ = writeln!(
        patch,
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}"
    );

    for line in &hunk.lines {
        let raw = match line.origin {
            ' ' => format!(" {}", line.content),
            '+' => format!("-{}", line.content),
            '-' => format!("+{}", line.content),
            '\\' => format!("\\{}", line.content),
            _ => continue,
        };
        let _ = writeln!(patch, "{raw}");
    }

    patch
}

fn build_line_revert_patch(file: &DiffFile, hunk: &DiffHunk, target_line_idx: usize) -> String {
    let file_path = diff_path(&file.path);
    let mut patch = String::new();
    let _ = writeln!(patch, "diff --git a/{file_path} b/{file_path}");
    let _ = writeln!(patch, "--- a/{file_path}");
    let _ = writeln!(patch, "+++ b/{file_path}");

    let suffix = hunk_header_suffix(&hunk.header);

    let old_start = hunk.new_start;
    let old_count = u32::try_from(hunk_old_count_from_current(hunk)).unwrap_or(0);

    let mut out_lines: Vec<String> = Vec::new();
    for (idx, line) in hunk.lines.iter().enumerate() {
        match line.origin {
            ' ' => out_lines.push(format!(" {}", line.content)),
            '+' => {
                if idx == target_line_idx {
                    out_lines.push(format!("-{}", line.content));
                } else {
                    out_lines.push(format!(" {}", line.content));
                }
            }
            '-' => {
                if idx == target_line_idx {
                    out_lines.push(format!("+{}", line.content));
                }
            }
            '\\' => out_lines.push(format!("\\{}", line.content)),
            _ => {}
        }
    }

    let new_count = u32::try_from(count_new_lines(&out_lines)).unwrap_or(0);
    let new_start = old_start;

    let _ = writeln!(
        patch,
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}"
    );
    for line in out_lines {
        let _ = writeln!(patch, "{line}");
    }

    patch
}

fn build_multi_line_revert_patch(
    file: &DiffFile,
    hunk: &DiffHunk,
    target_line_idxs: &[usize],
) -> String {
    let selected: HashSet<usize> = target_line_idxs.iter().copied().collect();

    let file_path = diff_path(&file.path);
    let mut patch = String::new();
    let _ = writeln!(patch, "diff --git a/{file_path} b/{file_path}");
    let _ = writeln!(patch, "--- a/{file_path}");
    let _ = writeln!(patch, "+++ b/{file_path}");

    let suffix = hunk_header_suffix(&hunk.header);

    let old_start = hunk.new_start;
    let old_count = u32::try_from(hunk_old_count_from_current(hunk)).unwrap_or(0);

    let mut out_lines: Vec<String> = Vec::new();
    for (idx, line) in hunk.lines.iter().enumerate() {
        match line.origin {
            ' ' => out_lines.push(format!(" {}", line.content)),
            '+' => {
                if selected.contains(&idx) {
                    out_lines.push(format!("-{}", line.content));
                } else {
                    out_lines.push(format!(" {}", line.content));
                }
            }
            '-' => {
                if selected.contains(&idx) {
                    out_lines.push(format!("+{}", line.content));
                }
            }
            '\\' => out_lines.push(format!("\\{}", line.content)),
            _ => {}
        }
    }

    let new_count = u32::try_from(count_new_lines(&out_lines)).unwrap_or(0);
    let new_start = old_start;

    let _ = writeln!(
        patch,
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}"
    );
    for line in out_lines {
        let _ = writeln!(patch, "{line}");
    }

    patch
}

fn diff_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn raw_hunk_line(line: &DiffHunkLine) -> String {
    match line.origin {
        '+' | '-' | ' ' => format!("{}{}", line.origin, line.content),
        '\\' => format!("\\{}", line.content),
        _ => line.content.clone(),
    }
}

fn hunk_header_suffix(header: &str) -> &str {
    let mut iter = header.match_indices("@@");
    let _ = iter.next();
    let Some((idx, _)) = iter.next() else {
        return "";
    };
    header.get(idx + 2..).unwrap_or("")
}

fn hunk_old_count_from_current(hunk: &DiffHunk) -> usize {
    // Old side for our patches is the current file (original "new" side).
    usize::try_from(hunk.new_lines).unwrap_or(0)
}

fn hunk_new_count_after_revert(hunk: &DiffHunk) -> usize {
    usize::try_from(hunk.old_lines).unwrap_or(0)
}

fn count_new_lines(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|l| l.starts_with(' ') || l.starts_with('+'))
        .count()
}

fn apply_git_patch(worktree_path: &Path, patch: &str, reverse: bool) -> Result<()> {
    match run_git_apply(worktree_path, patch, reverse, true) {
        Ok(()) => Ok(()),
        Err(index_err) => {
            // Retry without touching the index (helps when index differs from worktree).
            let worktree_err = run_git_apply(worktree_path, patch, reverse, false);
            worktree_err.with_context(|| format!("git apply failed (index): {index_err:#}"))
        }
    }
}

type CommandSpawn = fn(&mut Command) -> std::io::Result<Child>;
type ChildTakeStdin = fn(&mut Child) -> Option<ChildStdin>;
type ChildStdinWriteAll = fn(&mut ChildStdin, &[u8]) -> std::io::Result<()>;
type ChildWaitWithOutput = fn(Child) -> std::io::Result<Output>;

#[derive(Clone, Copy)]
struct GitApplyDeps {
    spawn: CommandSpawn,
    take_stdin: ChildTakeStdin,
    write_all: ChildStdinWriteAll,
    wait_with_output: ChildWaitWithOutput,
}

fn command_spawn(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.spawn()
}

#[expect(
    clippy::missing_const_for_fn,
    reason = "Used as a dependency function pointer for git apply tests."
)]
fn child_take_stdin(child: &mut Child) -> Option<ChildStdin> {
    child.stdin.take()
}

fn child_stdin_write_all(stdin: &mut ChildStdin, bytes: &[u8]) -> std::io::Result<()> {
    stdin.write_all(bytes)
}

fn child_wait_with_output(child: Child) -> std::io::Result<Output> {
    child.wait_with_output()
}

impl GitApplyDeps {
    fn production() -> Self {
        Self {
            spawn: command_spawn,
            take_stdin: child_take_stdin,
            write_all: child_stdin_write_all,
            wait_with_output: child_wait_with_output,
        }
    }
}

fn run_git_apply(worktree_path: &Path, patch: &str, reverse: bool, with_index: bool) -> Result<()> {
    run_git_apply_with_deps(
        worktree_path,
        patch,
        reverse,
        with_index,
        GitApplyDeps::production(),
    )
}

fn run_git_apply_with_deps(
    worktree_path: &Path,
    patch: &str,
    reverse: bool,
    with_index: bool,
    deps: GitApplyDeps,
) -> Result<()> {
    let mut cmd = crate::git::git_command();
    cmd.arg("-C")
        .arg(worktree_path)
        .arg("apply")
        .arg("--recount")
        .arg("--whitespace=nowarn");

    if reverse {
        cmd.arg("-R");
    }
    if with_index {
        cmd.arg("--index");
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = (deps.spawn)(&mut cmd).context("Failed to spawn git apply")?;
    let mut stdin = (deps.take_stdin)(&mut child)
        .ok_or_else(|| anyhow::anyhow!("Failed to open stdin for git apply"))?;
    (deps.write_all)(&mut stdin, patch.as_bytes())
        .context("Failed to write patch to git apply stdin")?;
    drop(stdin);

    let output = (deps.wait_with_output)(child).context("Failed to wait for git apply")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(anyhow::anyhow!(
        "git apply failed{}: {}\n{}",
        if with_index { " (--index)" } else { "" },
        stdout.trim(),
        stderr.trim()
    ))
}

#[cfg(test)]
mod tests {
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
    fn test_delete_selected_range_returns_early_for_missing_prerequisites() {
        let (mut data, _temp) = create_test_data();
        data.ui.diff_visual_anchor = Some(0);
        data.active_tab = Tab::Preview;

        delete_selected_range(&mut data).expect("delete range should succeed");
        assert_eq!(data.ui.diff_visual_anchor, Some(0));
        assert_eq!(data.ui.status_message, None);

        data.active_tab = Tab::Diff;
        data.ui.diff_visual_anchor = None;
        delete_selected_range(&mut data).expect("delete range should succeed");
        assert_eq!(data.ui.status_message, None);

        data.ui.diff_visual_anchor = Some(0);
        delete_selected_range(&mut data).expect("delete range should succeed");
        assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

        let temp_dir = TempDir::new().expect("temp dir should create");
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        data.ui.status_message = None;
        delete_selected_range(&mut data).expect("delete range should succeed");
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
        delete_selected_range(&mut data).expect("delete range should succeed");

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

        delete_selected_range(&mut data).expect("delete range should succeed");
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
        data.active_tab = Tab::Preview;
        delete_selected_hunk(&mut data).expect("delete selected hunk should succeed");
        assert_eq!(data.ui.status_message, None);

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
        data.active_tab = Tab::Preview;
        delete_selected_line(&mut data).expect("delete selected line should succeed");
        assert_eq!(data.ui.status_message, None);

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
            &[0],
        );
        assert!(multi_patch.contains("-selected"));
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
}
