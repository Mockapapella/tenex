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
        if app_data.active_tab != Tab::Diff {
            return Ok(DiffFocusedMode.into());
        }

        if let Some(anchor) = app_data.ui.diff_visual_anchor {
            delete_selected_range(app_data, anchor)?;
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

fn delete_selected_range(app_data: &mut AppData, anchor: usize) -> Result<()> {
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
mod tests;
