use crate::action::ValidIn;
use crate::app::{AppData, DiffEdit, DiffLineMeta, Tab};
use crate::git::{DiffFile, DiffHunk, DiffHunkLine, FileStatus};
use crate::state::{AppMode, DiffFocusedMode};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

/// Diff-focused action: exit diff focus.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnfocusDiffAction;

impl ValidIn<DiffFocusedMode> for UnfocusDiffAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
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

        let Some(file) = model.files.get(file_idx) else {
            continue;
        };
        let Some(hunk) = file.hunks.get(hunk_idx) else {
            continue;
        };

        patch.push_str(&build_multi_line_revert_patch(file, hunk, &line_indices));
        if !patch.ends_with('\n') {
            patch.push('\n');
        }
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

fn run_git_apply(worktree_path: &Path, patch: &str, reverse: bool, with_index: bool) -> Result<()> {
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

    let mut child = cmd.spawn().context("Failed to spawn git apply")?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to open stdin for git apply"))?;
        stdin
            .write_all(patch.as_bytes())
            .context("Failed to write patch to git apply stdin")?;
    }

    let output = child
        .wait_with_output()
        .context("Failed to wait for git apply")?;

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

    fn create_test_data() -> Result<(AppData, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            AppData::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    fn init_test_repo_with_commit(
        contents: &str,
    ) -> Result<(TempDir, Repository), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let repo = Repository::init(temp_dir.path())?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, contents)?;

        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("test.txt"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        {
            let tree = repo.find_tree(tree_id)?;
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
        }

        Ok((temp_dir, repo))
    }

    fn set_diff_view_from_repo(data: &mut AppData, repo: &Repository) -> Result<(), anyhow::Error> {
        let model = DiffGenerator::new(repo).uncommitted_model()?;
        let (content, meta) = data.ui.build_diff_view(&model);
        data.ui.set_diff_view(content, meta);
        data.ui.diff_model = Some(model);
        Ok(())
    }

    fn select_first_changed_line(data: &mut AppData) -> Result<()> {
        let model = data.ui.diff_model.as_ref().context("diff model set")?;
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
            .context("changed line present in diff view")?;
        data.ui.diff_cursor = cursor;
        Ok(())
    }

    fn select_first_hunk_header(data: &mut AppData) -> Result<()> {
        let cursor = data
            .ui
            .diff_line_meta
            .iter()
            .position(|m| matches!(m, DiffLineMeta::Hunk { .. }))
            .context("hunk header present in diff view")?;
        data.ui.diff_cursor = cursor;
        Ok(())
    }

    fn select_first_context_line(data: &mut AppData) -> Result<()> {
        let model = data.ui.diff_model.as_ref().context("diff model set")?;
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
            .context("context line present in diff view")?;
        data.ui.diff_cursor = cursor;
        Ok(())
    }

    #[test]
    fn test_diff_cursor_actions_respect_active_tab() -> Result<(), Box<dyn std::error::Error>> {
        let (mut data, _temp) = create_test_data()?;
        data.ui.set_diff_content("line1\nline2\nline3\n");
        data.active_tab = Tab::Diff;

        assert_eq!(data.ui.diff_cursor, 0);
        assert_eq!(
            DiffCursorDownAction.execute(DiffFocusedMode, &mut data)?,
            DiffFocusedMode.into()
        );
        assert_eq!(data.ui.diff_cursor, 1);

        assert_eq!(
            DiffCursorUpAction.execute(DiffFocusedMode, &mut data)?,
            DiffFocusedMode.into()
        );
        assert_eq!(data.ui.diff_cursor, 0);

        data.active_tab = Tab::Preview;
        assert_eq!(
            DiffCursorDownAction.execute(DiffFocusedMode, &mut data)?,
            DiffFocusedMode.into()
        );
        assert_eq!(data.ui.diff_cursor, 0);
        Ok(())
    }

    #[test]
    fn test_delete_line_and_undo_redo_on_unstaged_change() -> Result<(), Box<dyn std::error::Error>>
    {
        let original = "one\ntwo\n";
        let modified = "one\ntwo\nthree\n";
        let (temp_dir, repo) = init_test_repo_with_commit(original)?;

        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, modified)?;

        let (mut data, _temp) = create_test_data()?;
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));
        data.active_tab = Tab::Diff;
        set_diff_view_from_repo(&mut data, &repo)?;
        select_first_changed_line(&mut data)?;

        DiffDeleteLineAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, original);
        assert_eq!(data.ui.diff_undo.len(), 1);
        assert!(data.ui.diff_redo.is_empty());
        assert!(data.ui.diff_force_refresh);
        assert_eq!(data.ui.status_message.as_deref(), Some("Deleted diff line"));

        DiffUndoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, modified);
        assert!(data.ui.diff_force_refresh);
        assert_eq!(data.ui.status_message.as_deref(), Some("Undo"));

        DiffRedoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, original);
        assert!(data.ui.diff_force_refresh);
        assert_eq!(data.ui.status_message.as_deref(), Some("Redo"));
        Ok(())
    }

    #[test]
    fn test_delete_visual_range_and_undo_redo_on_unstaged_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let original = "one\ntwo\n";
        let modified = "one\ntwo\nthree\nfour\n";
        let (temp_dir, repo) = init_test_repo_with_commit(original)?;

        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, modified)?;

        let (mut data, _temp) = create_test_data()?;
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));
        data.active_tab = Tab::Diff;
        set_diff_view_from_repo(&mut data, &repo)?;

        let model = data.ui.diff_model.as_ref().context("diff model set")?;
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

        DiffDeleteLineAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, original);
        assert_eq!(data.ui.diff_undo.len(), 1);
        assert!(data.ui.diff_redo.is_empty());
        assert!(data.ui.diff_force_refresh);
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Deleted 2 diff lines")
        );
        assert!(data.ui.diff_visual_anchor.is_none());

        DiffUndoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, modified);
        assert!(data.ui.diff_force_refresh);
        assert_eq!(data.ui.status_message.as_deref(), Some("Undo"));

        DiffRedoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, original);
        assert!(data.ui.diff_force_refresh);
        assert_eq!(data.ui.status_message.as_deref(), Some("Redo"));
        Ok(())
    }

    #[test]
    fn test_delete_line_requires_changed_line() -> Result<(), Box<dyn std::error::Error>> {
        let original = "one\ntwo\n";
        let modified = "one\ntwo\nthree\n";
        let (temp_dir, repo) = init_test_repo_with_commit(original)?;

        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, modified)?;

        let (mut data, _temp) = create_test_data()?;
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));
        data.active_tab = Tab::Diff;
        set_diff_view_from_repo(&mut data, &repo)?;
        select_first_context_line(&mut data)?;

        DiffDeleteLineAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, modified);
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Select a changed line (+/-) to delete")
        );
        assert!(data.ui.diff_undo.is_empty());
        Ok(())
    }

    #[test]
    fn test_undo_redo_noop_when_empty() -> Result<(), Box<dyn std::error::Error>> {
        let (mut data, _temp) = create_test_data()?;
        data.active_tab = Tab::Diff;

        DiffUndoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(data.ui.status_message.as_deref(), Some("No agent selected"));

        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));
        DiffUndoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(data.ui.status_message.as_deref(), Some("Nothing to undo"));

        DiffRedoAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(data.ui.status_message.as_deref(), Some("Nothing to redo"));
        Ok(())
    }

    #[test]
    fn test_delete_hunk_applies_with_index_on_staged_change()
    -> Result<(), Box<dyn std::error::Error>> {
        let original = "a\nb\n";
        let modified = "a\nb\nc\n";
        let (temp_dir, repo) = init_test_repo_with_commit(original)?;

        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, modified)?;
        {
            let mut index = repo.index()?;
            index.add_path(std::path::Path::new("test.txt"))?;
            index.write()?;
        }

        let (mut data, _temp) = create_test_data()?;
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));
        data.active_tab = Tab::Diff;
        set_diff_view_from_repo(&mut data, &repo)?;
        select_first_hunk_header(&mut data)?;

        DiffDeleteLineAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, original);
        assert_eq!(data.ui.status_message.as_deref(), Some("Deleted diff hunk"));
        Ok(())
    }

    #[test]
    fn test_restore_deleted_file_from_diff() -> Result<(), Box<dyn std::error::Error>> {
        let original = "keep\nme\n";
        let (temp_dir, repo) = init_test_repo_with_commit(original)?;
        let file_path = temp_dir.path().join("test.txt");

        fs::remove_file(&file_path)?;
        {
            let mut index = repo.index()?;
            index.remove_path(std::path::Path::new("test.txt"))?;
            index.write()?;
        }

        let (mut data, _temp) = create_test_data()?;
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));
        data.active_tab = Tab::Diff;
        set_diff_view_from_repo(&mut data, &repo)?;
        select_first_hunk_header(&mut data)?;

        DiffDeleteLineAction.execute(DiffFocusedMode, &mut data)?;
        assert_eq!(fs::read_to_string(&file_path)?, original);
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Restored deleted file")
        );
        assert_eq!(data.ui.diff_undo.len(), 1);
        assert!(data.ui.diff_undo[0].applied_reverse);
        Ok(())
    }

    #[test]
    fn test_delete_line_noops_for_deleted_file() -> Result<(), Box<dyn std::error::Error>> {
        let original = "keep\nme\n";
        let (temp_dir, repo) = init_test_repo_with_commit(original)?;
        let file_path = temp_dir.path().join("test.txt");

        fs::remove_file(&file_path)?;
        {
            let mut index = repo.index()?;
            index.remove_path(std::path::Path::new("test.txt"))?;
            index.write()?;
        }

        let (mut data, _temp) = create_test_data()?;
        data.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));
        data.active_tab = Tab::Diff;
        set_diff_view_from_repo(&mut data, &repo)?;
        select_first_changed_line(&mut data)?;

        DiffDeleteLineAction.execute(DiffFocusedMode, &mut data)?;
        assert!(!file_path.exists());
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Cannot delete a line from a deleted file (select hunk header to restore)")
        );
        assert!(data.ui.diff_undo.is_empty());
        Ok(())
    }

    #[test]
    fn test_run_git_apply_errors_on_invalid_patch() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, _repo) = init_test_repo_with_commit("a\n")?;
        let err = match run_git_apply(temp_dir.path(), "not a patch", false, false) {
            Ok(()) => {
                return Err(std::io::Error::other("invalid patch should fail").into());
            }
            Err(err) => err,
        };
        assert!(format!("{err:#}").contains("git apply failed"));
        Ok(())
    }

    #[test]
    fn test_diff_helpers_cover_edge_cases() {
        assert_eq!(diff_path(&PathBuf::from(r"dir\file.txt")), "dir/file.txt");
        assert_eq!(hunk_header_suffix("not a header"), "");

        let line = DiffHunkLine {
            origin: '?',
            content: "content".to_string(),
            old_lineno: None,
            new_lineno: None,
        };
        assert_eq!(raw_hunk_line(&line), "content");
    }
}
