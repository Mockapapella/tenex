//! Git diff generation (CLI-based, Windows)

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{Repository, git_output};

/// Generator for git diffs
pub struct Generator<'a> {
    /// Repository handle
    pub repo: &'a Repository,
}

impl std::fmt::Debug for Generator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Generator").finish_non_exhaustive()
    }
}

impl<'a> Generator<'a> {
    /// Create a new diff generator for the given repository
    #[must_use]
    pub const fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    /// Get unstaged changes (working directory vs index)
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn unstaged(&self) -> Result<Vec<FileChange>> {
        let patch = git_output(
            &self.repo.root,
            &["diff", "--no-color", "--patch", "--no-ext-diff"],
        )
        .context("Failed to get unstaged diff")?;

        let mut files = parse_patch(&patch);
        add_untracked(&self.repo.root, &mut files)?;
        Ok(files)
    }

    /// Get staged changes (index vs HEAD)
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn staged(&self) -> Result<Vec<FileChange>> {
        let patch = git_output(
            &self.repo.root,
            &["diff", "--cached", "--no-color", "--patch", "--no-ext-diff"],
        )
        .context("Failed to get staged diff")?;

        Ok(parse_patch(&patch))
    }

    /// Get all uncommitted changes (working directory vs HEAD)
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn uncommitted(&self) -> Result<Vec<FileChange>> {
        let patch = git_output(
            &self.repo.root,
            &["diff", "HEAD", "--no-color", "--patch", "--no-ext-diff"],
        )
        .context("Failed to get uncommitted diff")?;

        let mut files = parse_patch(&patch);
        add_untracked(&self.repo.root, &mut files)?;
        Ok(files)
    }

    /// Get diff between two commits
    ///
    /// # Errors
    ///
    /// Returns an error if the commits cannot be found or diff cannot be generated
    pub fn between_commits(&self, old_commit: &str, new_commit: &str) -> Result<Vec<FileChange>> {
        let patch = git_output(
            &self.repo.root,
            &[
                "diff",
                old_commit,
                new_commit,
                "--no-color",
                "--patch",
                "--no-ext-diff",
            ],
        )
        .with_context(|| format!("Failed to diff {old_commit}..{new_commit}"))?;

        Ok(parse_patch(&patch))
    }

    /// Get diff between a branch and HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn branch_diff(&self, branch: &str) -> Result<Vec<FileChange>> {
        self.between_commits(branch, "HEAD")
    }

    /// Get a summary of changes (files changed, additions, deletions)
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn summary(&self) -> Result<Summary> {
        let files = self.uncommitted()?;

        let additions: usize = files.iter().map(|f| f.additions).sum();
        let deletions: usize = files.iter().map(|f| f.deletions).sum();

        Ok(Summary {
            files_changed: files.len(),
            additions,
            deletions,
        })
    }

    /// Check if there are any uncommitted changes
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be checked
    pub fn has_changes(&self) -> Result<bool> {
        let output = git_output(
            &self.repo.root,
            &["status", "--porcelain", "--untracked-files=normal"],
        )
        .context("Failed to get repository status")?;
        Ok(!output.trim().is_empty())
    }
}

fn parse_patch(patch: &str) -> Vec<FileChange> {
    let mut files = Vec::new();
    let mut current: Option<FileChange> = None;

    for raw_line in patch.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);

        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(file) = current.take() {
                files.push(file);
            }

            let mut parts = rest.split_whitespace();
            let a_path = parts.next().unwrap_or("");
            let b_path = parts.next().unwrap_or("");
            let diff_path = normalize_diff_path(b_path).or_else(|| normalize_diff_path(a_path));

            if let Some(diff_path) = diff_path {
                let file = FileChange {
                    path: diff_path,
                    status: FileStatus::Modified,
                    lines: Vec::new(),
                    additions: 0,
                    deletions: 0,
                };
                current = Some(file);
            }
            continue;
        }

        let Some(file) = current.as_mut() else {
            continue;
        };

        if let Some(status) = parse_status_line(line, file) {
            file.status = status;
            continue;
        }

        if is_diff_header(line) {
            continue;
        }

        if let Some(line_change) = parse_line_change(line, raw_line) {
            match line_change {
                LineChange::Added(_) => file.additions += 1,
                LineChange::Removed(_) => file.deletions += 1,
                LineChange::Context(_) => {}
            }
            file.lines.push(line_change);
        }
    }

    if let Some(file) = current.take() {
        files.push(file);
    }

    // Deduplicate by path while preserving first occurrence
    if files.len() > 1 {
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for file in files {
            if seen.insert(file.path.clone()) {
                unique.push(file);
            }
        }
        unique
    } else {
        files
    }
}

fn normalize_diff_path(token: &str) -> Option<PathBuf> {
    let trimmed = token.trim_matches('"');
    let trimmed = trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))?;
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn parse_status_line(line: &str, file: &mut FileChange) -> Option<FileStatus> {
    if let Some(path) = line.strip_prefix("rename to ") {
        file.path = PathBuf::from(path.trim());
        return Some(FileStatus::Renamed);
    }
    if line.starts_with("rename from ") {
        return Some(FileStatus::Renamed);
    }
    if let Some(path) = line.strip_prefix("copy to ") {
        file.path = PathBuf::from(path.trim());
        return Some(FileStatus::Copied);
    }
    if line.starts_with("copy from ") {
        return Some(FileStatus::Copied);
    }
    if line.starts_with("new file mode ") {
        return Some(FileStatus::Added);
    }
    if line.starts_with("deleted file mode ") {
        return Some(FileStatus::Deleted);
    }
    if line.starts_with("old mode ") || line.starts_with("new mode ") {
        return Some(FileStatus::TypeChange);
    }
    None
}

fn is_diff_header(line: &str) -> bool {
    line.starts_with("index ")
        || line.starts_with("@@ ")
        || line.starts_with("@@@ ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
}

fn parse_line_change(line: &str, raw_line: &str) -> Option<LineChange> {
    if line.starts_with('+') && !line.starts_with("+++") {
        return Some(LineChange::Added(raw_line.to_string()));
    }
    if line.starts_with('-') && !line.starts_with("---") {
        return Some(LineChange::Removed(raw_line.to_string()));
    }
    if line.starts_with(' ') {
        return Some(LineChange::Context(raw_line.to_string()));
    }
    if line.starts_with('\\') {
        return Some(LineChange::Context(raw_line.to_string()));
    }
    None
}

fn add_untracked(repo_root: &Path, files: &mut Vec<FileChange>) -> Result<()> {
    let output = git_output(repo_root, &["ls-files", "--others", "--exclude-standard"])
        .context("Failed to list untracked files")?;

    for line in output.lines() {
        let rel_path = line.trim();
        if rel_path.is_empty() {
            continue;
        }

        let path = PathBuf::from(rel_path);
        if files.iter().any(|f| f.path == path) {
            continue;
        }

        let abs_path = repo_root.join(&path);
        let contents = match std::fs::read(&abs_path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Err(err) => {
                let file = FileChange {
                    path,
                    status: FileStatus::Untracked,
                    lines: vec![LineChange::Context(format!("Unable to read file: {err}\n"))],
                    additions: 0,
                    deletions: 0,
                };
                files.push(file);
                continue;
            }
        };

        if contents.is_empty() {
            files.push(FileChange {
                path,
                status: FileStatus::Untracked,
                lines: Vec::new(),
                additions: 0,
                deletions: 0,
            });
            continue;
        }

        let ends_with_newline = contents.ends_with('\n');
        let mut parts: Vec<&str> = contents.split('\n').collect();
        if ends_with_newline {
            parts.pop();
        }

        let mut line_changes = Vec::new();
        let mut additions = 0;
        for (idx, line) in parts.iter().enumerate() {
            let mut rendered = (*line).to_string();
            if idx + 1 < parts.len() || ends_with_newline {
                rendered.push('\n');
            }
            additions += 1;
            line_changes.push(LineChange::Added(rendered));
        }

        files.push(FileChange {
            path,
            status: FileStatus::Untracked,
            lines: line_changes,
            additions,
            deletions: 0,
        });
    }

    Ok(())
}

/// Represents a single file's diff
#[derive(Debug, Clone)]
pub struct FileChange {
    /// Path to the file
    pub path: PathBuf,
    /// Status of the file (added, modified, deleted, etc.)
    pub status: FileStatus,
    /// Lines of the diff
    pub lines: Vec<LineChange>,
    /// Number of lines added
    pub additions: usize,
    /// Number of lines deleted
    pub deletions: usize,
}

impl FileChange {
    /// Get the diff as a formatted string
    #[must_use]
    pub fn to_string_colored(&self) -> String {
        let mut output = String::new();
        output.push_str("--- a/");
        output.push_str(&self.path.display().to_string());
        output.push_str("\n+++ b/");
        output.push_str(&self.path.display().to_string());
        output.push('\n');

        for line in &self.lines {
            match line {
                LineChange::Added(content) => {
                    output.push('+');
                    output.push_str(content);
                }
                LineChange::Removed(content) => {
                    output.push('-');
                    output.push_str(content);
                }
                LineChange::Context(content) => {
                    output.push(' ');
                    output.push_str(content);
                }
            }
        }

        output
    }
}

/// A single line in a diff
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineChange {
    /// Line was added
    Added(String),
    /// Line was removed
    Removed(String),
    /// Context line (unchanged)
    Context(String),
}

/// Status of a file in the diff
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    /// File was added
    Added,
    /// File was deleted
    Deleted,
    /// File was modified
    Modified,
    /// File was renamed
    Renamed,
    /// File was copied
    Copied,
    /// File type changed
    TypeChange,
    /// Untracked file
    Untracked,
    /// Unknown status
    Unknown,
}

impl std::fmt::Display for FileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Modified => "M",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::TypeChange => "T",
            Self::Untracked => "?",
            Self::Unknown => "X",
        };
        write!(f, "{s}")
    }
}

/// Summary of diff statistics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Number of files changed
    pub files_changed: usize,
    /// Total lines added
    pub additions: usize,
    /// Total lines deleted
    pub deletions: usize,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} file(s) changed, {} insertion(s), {} deletion(s)",
            self.files_changed, self.additions, self.deletions
        )
    }
}
