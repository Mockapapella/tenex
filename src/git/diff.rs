//! Git diff generation

use anyhow::{Context, Result};
use git2::{Delta, DiffOptions, Repository};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};

fn diff_line_content(line: &git2::DiffLine<'_>) -> String {
    let mut content = String::from_utf8_lossy(line.content()).to_string();
    if content.ends_with('\n') {
        content.pop();
        if content.ends_with('\r') {
            content.pop();
        }
    }
    content
}

fn hash_diff_context(hasher: &mut DefaultHasher, status: FileStatus, file_path: &Path) {
    status.hash(hasher);
    file_path.hash(hasher);
}

fn hash_diff_line(hasher: &mut DefaultHasher, origin: char, content: &str) {
    match origin {
        '+' | '-' | ' ' | '\\' => {
            hasher.write_u8(origin as u8);
            hasher.write_usize(content.len());
            hasher.write(content.as_bytes());
        }
        _ => {
            hasher.write_usize(content.len());
            hasher.write(content.as_bytes());
        }
    }
}

fn upsert_model_file(
    files: &mut Vec<DiffFile>,
    file_indices: &mut HashMap<PathBuf, usize>,
    file_set: &mut HashSet<PathBuf>,
    file_path: &Path,
    status: FileStatus,
) -> usize {
    file_indices.get(file_path).copied().unwrap_or_else(|| {
        let file_path_buf = file_path.to_path_buf();
        files.push(DiffFile {
            path: file_path_buf.clone(),
            status,
            meta: Vec::new(),
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
        });
        let idx = files.len() - 1;
        file_indices.insert(file_path_buf.clone(), idx);
        file_set.insert(file_path_buf);
        idx
    })
}

/// Generator for git diffs
pub struct Generator<'a> {
    repo: &'a Repository,
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
        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.show_untracked_content(true);

        let diff = self
            .repo
            .diff_index_to_workdir(None, Some(&mut opts))
            .context("Failed to get unstaged diff")?;

        Self::parse_diff(&diff)
    }

    /// Get staged changes (index vs HEAD)
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn staged(&self) -> Result<Vec<FileChange>> {
        let head = self.repo.head().ok();
        let tree = head.and_then(|h| h.peel_to_tree().ok());

        let diff = self
            .repo
            .diff_tree_to_index(tree.as_ref(), None, None)
            .context("Failed to get staged diff")?;

        Self::parse_diff(&diff)
    }

    /// Get all uncommitted changes (working directory vs HEAD)
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn uncommitted(&self) -> Result<Vec<FileChange>> {
        let head = self.repo.head().ok();
        let tree = head.and_then(|h| h.peel_to_tree().ok());

        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.show_untracked_content(true);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(tree.as_ref(), Some(&mut opts))
            .context("Failed to get uncommitted diff")?;

        Self::parse_diff(&diff)
    }

    /// Get a structured uncommitted diff model suitable for interactive UIs.
    ///
    /// Includes staged + unstaged + untracked changes vs `HEAD`.
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated or parsed.
    pub fn uncommitted_model(&self) -> Result<DiffModel> {
        let head = self.repo.head().ok();
        let tree = head.and_then(|h| h.peel_to_tree().ok());

        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.show_untracked_content(true);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(tree.as_ref(), Some(&mut opts))
            .context("Failed to get uncommitted diff")?;

        Self::parse_diff_model(&diff)
    }

    /// Get a lightweight digest of the uncommitted diff for change detection.
    ///
    /// This hashes the patch output and includes a summary, without storing the full model.
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated or printed.
    pub fn uncommitted_digest(&self) -> Result<DiffDigest> {
        let head = self.repo.head().ok();
        let tree = head.and_then(|h| h.peel_to_tree().ok());

        let mut opts = DiffOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.show_untracked_content(true);

        let diff = self
            .repo
            .diff_tree_to_workdir_with_index(tree.as_ref(), Some(&mut opts))
            .context("Failed to get uncommitted diff")?;

        Self::digest_diff(&diff)
    }

    /// Get diff between two commits
    ///
    /// # Errors
    ///
    /// Returns an error if the commits cannot be found or diff cannot be generated
    pub fn between_commits(&self, old_commit: &str, new_commit: &str) -> Result<Vec<FileChange>> {
        let old_oid = self
            .repo
            .revparse_single(old_commit)
            .with_context(|| format!("Could not find commit: {old_commit}"))?
            .peel_to_commit()
            .context("Old reference is not a commit")?;

        let new_oid = self
            .repo
            .revparse_single(new_commit)
            .with_context(|| format!("Could not find commit: {new_commit}"))?
            .peel_to_commit()
            .context("New reference is not a commit")?;

        let old_tree = old_oid.tree().context("Could not get old tree")?;
        let new_tree = new_oid.tree().context("Could not get new tree")?;

        let diff = self
            .repo
            .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
            .context("Failed to diff trees")?;

        Self::parse_diff(&diff)
    }

    /// Get diff between a branch and HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the diff cannot be generated
    pub fn branch_diff(&self, branch: &str) -> Result<Vec<FileChange>> {
        self.between_commits(branch, "HEAD")
    }

    /// Parse a `git2::Diff` into our `FileDiff` structs
    fn parse_diff(diff: &git2::Diff<'_>) -> Result<Vec<FileChange>> {
        let mut files = Vec::new();
        let mut file_indices: HashMap<PathBuf, usize> = HashMap::new();

        diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
            // Avoid O(n²) scanning by indexing file changes by path.
            // Use a borrowed `&Path` for lookups to avoid per-line `PathBuf` allocations.
            let file_path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .unwrap_or_else(|| Path::new(""));

            let file_idx = file_indices.get(file_path).copied().map_or_else(
                || {
                    let file_path_buf = file_path.to_path_buf();
                    files.push(FileChange {
                        path: file_path_buf.clone(),
                        status: delta_to_status(delta.status()),
                        lines: Vec::new(),
                        additions: 0,
                        deletions: 0,
                    });
                    let idx = files.len() - 1;
                    file_indices.insert(file_path_buf, idx);
                    idx
                },
                |file_idx| file_idx,
            );
            let file = &mut files[file_idx];

            let content = String::from_utf8_lossy(line.content()).to_string();
            let line_diff = match line.origin() {
                '+' => {
                    file.additions += 1;
                    LineChange::Added(content)
                }
                '-' => {
                    file.deletions += 1;
                    LineChange::Removed(content)
                }
                ' ' | '\\' => LineChange::Context(content),
                _ => return true,
            };

            file.lines.push(line_diff);
            true
        })
        .context("Failed to parse diff")?;

        Ok(files)
    }

    /// Parse a `git2::Diff` into a structured `DiffModel`.
    fn parse_diff_model(diff: &git2::Diff<'_>) -> Result<DiffModel> {
        let mut files: Vec<DiffFile> = Vec::new();
        let mut file_indices: HashMap<PathBuf, usize> = HashMap::new();

        let mut hasher = DefaultHasher::new();
        let mut file_set: HashSet<PathBuf> = HashSet::new();
        let mut additions = 0usize;
        let mut deletions = 0usize;

        diff.print(git2::DiffFormat::Patch, |delta, hunk, line| {
            let file_path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .unwrap_or_else(|| Path::new(""));

            let status = delta_to_status(delta.status());
            hash_diff_context(&mut hasher, status, file_path);

            let file_idx = upsert_model_file(
                &mut files,
                &mut file_indices,
                &mut file_set,
                file_path,
                status,
            );

            let file = &mut files[file_idx];

            let content = diff_line_content(&line);

            let origin = line.origin();
            hash_diff_line(&mut hasher, origin, &content);

            match origin {
                'H' => {
                    // Start a new hunk.
                    let (old_start, old_lines, new_start, new_lines) = hunk
                        .map_or((0, 0, 0, 0), |h| {
                            (h.old_start(), h.old_lines(), h.new_start(), h.new_lines())
                        });
                    file.hunks.push(DiffHunk {
                        header: content,
                        old_start,
                        old_lines,
                        new_start,
                        new_lines,
                        lines: Vec::new(),
                    });
                }
                '+' | '-' | ' ' | '\\' => {
                    // Hunk line. Ensure we have a hunk to attach to.
                    if file.hunks.is_empty() {
                        file.hunks.push(DiffHunk {
                            header: "@@ -0,0 +0,0 @@".to_string(),
                            old_start: 0,
                            old_lines: 0,
                            new_start: 0,
                            new_lines: 0,
                            lines: Vec::new(),
                        });
                    }
                    let line_entry = DiffHunkLine {
                        origin,
                        content,
                        old_lineno: line.old_lineno(),
                        new_lineno: line.new_lineno(),
                    };
                    if let Some(last) = file.hunks.last_mut() {
                        last.lines.push(line_entry);
                    }

                    match origin {
                        '+' => {
                            additions += 1;
                            file.additions += 1;
                        }
                        '-' => {
                            deletions += 1;
                            file.deletions += 1;
                        }
                        _ => {}
                    }
                }
                _ => {
                    // File-level metadata or other patch lines (diff --git, index, ---/+++).
                    file.meta.push(content);
                }
            }

            true
        })
        .context("Failed to parse diff patch")?;

        let files_changed = file_set.len();
        let summary = Summary {
            files_changed,
            additions,
            deletions,
        };

        let hash = if files_changed == 0 {
            0
        } else {
            hasher.finish()
        };

        Ok(DiffModel {
            files,
            summary,
            hash,
        })
    }

    fn digest_diff(diff: &git2::Diff<'_>) -> Result<DiffDigest> {
        let mut hasher = DefaultHasher::new();
        let mut file_set: HashSet<PathBuf> = HashSet::new();
        let mut additions = 0usize;
        let mut deletions = 0usize;

        diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
            let file_path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .unwrap_or_else(|| Path::new(""));

            file_set.insert(file_path.to_path_buf());

            let status = delta_to_status(delta.status());
            hash_diff_context(&mut hasher, status, file_path);

            let content = diff_line_content(&line);
            hash_diff_line(&mut hasher, line.origin(), &content);

            match line.origin() {
                '+' => additions += 1,
                '-' => deletions += 1,
                _ => {}
            }

            true
        })
        .context("Failed to compute diff digest")?;

        let files_changed = file_set.len();
        let summary = Summary {
            files_changed,
            additions,
            deletions,
        };
        let hash = if files_changed == 0 {
            0
        } else {
            hasher.finish()
        };

        Ok(DiffDigest { hash, summary })
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
        let statuses = self
            .repo
            .statuses(None)
            .context("Failed to get repository status")?;
        Ok(!statuses.is_empty())
    }
}

/// A lightweight fingerprint of a diff for change detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffDigest {
    /// Hash of the patch output (0 when no changes).
    pub hash: u64,
    /// Summary statistics derived from the patch output.
    pub summary: Summary,
}

/// A structured diff model suitable for interactive UIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffModel {
    /// Files included in this diff.
    pub files: Vec<DiffFile>,
    /// Summary of changes.
    pub summary: Summary,
    /// Hash of the patch content (0 when no changes).
    pub hash: u64,
}

/// A single file in a structured diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    /// Path to the file.
    pub path: PathBuf,
    /// Status of the file.
    pub status: FileStatus,
    /// File-level metadata lines (e.g. `diff --git`, `index`, `---/+++`).
    pub meta: Vec<String>,
    /// Hunks in the file.
    pub hunks: Vec<DiffHunk>,
    /// Number of added lines in this file.
    pub additions: usize,
    /// Number of removed lines in this file.
    pub deletions: usize,
}

/// A hunk within a file diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// The raw hunk header line (starts with `@@`).
    pub header: String,
    /// Old-file starting line (as reported by libgit2).
    pub old_start: u32,
    /// Old-file line count (as reported by libgit2).
    pub old_lines: u32,
    /// New-file starting line (as reported by libgit2).
    pub new_start: u32,
    /// New-file line count (as reported by libgit2).
    pub new_lines: u32,
    /// Lines within the hunk.
    pub lines: Vec<DiffHunkLine>,
}

/// A single line within a hunk.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiffHunkLine {
    /// The line origin character (`+`, `-`, ` `, or `\\`).
    pub origin: char,
    /// The line content without the origin prefix, and without trailing newline.
    pub content: String,
    /// 1-based line number in the old file, if applicable.
    pub old_lineno: Option<u32>,
    /// 1-based line number in the new file, if applicable.
    pub new_lineno: Option<u32>,
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
        let _ = write!(
            output,
            "--- a/{}\n+++ b/{}\n",
            self.path.display(),
            self.path.display()
        );

        for line in &self.lines {
            match line {
                LineChange::Added(content) => {
                    let _ = write!(output, "+{content}");
                }
                LineChange::Removed(content) => {
                    let _ = write!(output, "-{content}");
                }
                LineChange::Context(content) => {
                    let _ = write!(output, " {content}");
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Convert git2 Delta to our `FileStatus`
const fn delta_to_status(delta: Delta) -> FileStatus {
    match delta {
        Delta::Added => FileStatus::Added,
        Delta::Deleted => FileStatus::Deleted,
        Delta::Modified => FileStatus::Modified,
        Delta::Renamed => FileStatus::Renamed,
        Delta::Copied => FileStatus::Copied,
        Delta::Typechange => FileStatus::TypeChange,
        Delta::Untracked => FileStatus::Untracked,
        _ => FileStatus::Unknown,
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

#[cfg(test)]
mod tests {
    use super::*;
    use git2::Signature;
    use std::fs;
    use tempfile::TempDir;

    fn init_test_repo_with_commit() -> Result<(TempDir, Repository), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let repo = Repository::init(temp_dir.path())?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test\n")?;

        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;

        {
            let tree = repo.find_tree(tree_id)?;
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;
        }

        Ok((temp_dir, repo))
    }

    #[test]
    fn test_no_changes() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let uncommitted = generator.uncommitted()?;
        assert!(uncommitted.is_empty());
        assert!(!generator.has_changes()?);
        Ok(())
    }

    #[test]
    fn test_unstaged_changes() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test\n\nNew content\n")?;

        let unstaged = generator.unstaged()?;
        assert_eq!(unstaged.len(), 1);
        assert_eq!(unstaged[0].status, FileStatus::Modified);
        assert!(generator.has_changes()?);
        Ok(())
    }

    #[test]
    fn test_staged_changes() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let new_file = temp_dir.path().join("new.txt");
        fs::write(&new_file, "New file content\n")?;

        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("new.txt"))?;
        index.write()?;

        let staged = generator.staged()?;
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].status, FileStatus::Added);
        Ok(())
    }

    #[test]
    fn test_uncommitted_changes() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Modified\n")?;

        let uncommitted = generator.uncommitted()?;
        assert!(!uncommitted.is_empty());
        Ok(())
    }

    #[test]
    fn test_untracked_file_included_in_uncommitted() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let untracked_path = temp_dir.path().join("untracked.txt");
        fs::write(&untracked_path, "hello\n")?;

        let files = generator.uncommitted()?;
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Untracked);
        assert_eq!(files[0].path.as_path(), Path::new("untracked.txt"));
        assert!(files[0].additions > 0);
        assert!(generator.has_changes()?);
        Ok(())
    }

    #[test]
    fn test_untracked_file_in_directory_included_in_uncommitted()
    -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let dir = temp_dir.path().join("newdir");
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("nested.txt"), "nested\n")?;

        let files = generator.uncommitted()?;
        assert!(
            files
                .iter()
                .any(|file| file.path.as_path() == Path::new("newdir/nested.txt"))
        );
        Ok(())
    }

    #[test]
    fn test_summary() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Modified\nNew line\n")?;

        let summary = generator.summary()?;
        assert_eq!(summary.files_changed, 1);
        assert!(summary.additions > 0 || summary.deletions > 0);
        Ok(())
    }

    #[test]
    fn test_summary_display() {
        let summary = Summary {
            files_changed: 3,
            additions: 10,
            deletions: 5,
        };

        let display = format!("{summary}");
        assert!(display.contains("3 file(s) changed"));
        assert!(display.contains("10 insertion(s)"));
        assert!(display.contains("5 deletion(s)"));
    }

    #[test]
    fn test_file_status_display() {
        assert_eq!(format!("{}", FileStatus::Added), "A");
        assert_eq!(format!("{}", FileStatus::Deleted), "D");
        assert_eq!(format!("{}", FileStatus::Modified), "M");
        assert_eq!(format!("{}", FileStatus::Renamed), "R");
        assert_eq!(format!("{}", FileStatus::Copied), "C");
        assert_eq!(format!("{}", FileStatus::TypeChange), "T");
        assert_eq!(format!("{}", FileStatus::Untracked), "?");
        assert_eq!(format!("{}", FileStatus::Unknown), "X");
    }

    #[test]
    fn test_file_diff_to_string() {
        let diff = FileChange {
            path: PathBuf::from("test.txt"),
            status: FileStatus::Modified,
            lines: vec![
                LineChange::Context("unchanged\n".to_string()),
                LineChange::Removed("old line\n".to_string()),
                LineChange::Added("new line\n".to_string()),
            ],
            additions: 1,
            deletions: 1,
        };

        let output = diff.to_string_colored();
        assert!(output.contains("--- a/test.txt"));
        assert!(output.contains("+++ b/test.txt"));
        assert!(output.contains("-old line"));
        assert!(output.contains("+new line"));
        assert!(output.contains(" unchanged"));
    }

    #[test]
    fn test_line_diff_equality() {
        assert_eq!(
            LineChange::Added("test".to_string()),
            LineChange::Added("test".to_string())
        );
        assert_ne!(
            LineChange::Added("test".to_string()),
            LineChange::Removed("test".to_string())
        );
    }

    #[test]
    fn test_between_commits() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, repo) = init_test_repo_with_commit()?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Modified\n")?;

        let mut index = repo.index()?;
        index.add_path(std::path::Path::new("README.md"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let head = repo.head()?.peel_to_commit()?;

        repo.commit(Some("HEAD"), &sig, &sig, "Second commit", &tree, &[&head])?;

        let generator = Generator::new(&repo);
        let diff = generator.between_commits("HEAD~1", "HEAD")?;
        assert_eq!(diff.len(), 1);
        Ok(())
    }

    #[test]
    fn test_between_commits_invalid() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, repo) = init_test_repo_with_commit()?;
        let generator = Generator::new(&repo);

        let result = generator.between_commits("invalid", "HEAD");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_delta_to_status() {
        assert_eq!(delta_to_status(Delta::Added), FileStatus::Added);
        assert_eq!(delta_to_status(Delta::Deleted), FileStatus::Deleted);
        assert_eq!(delta_to_status(Delta::Modified), FileStatus::Modified);
        assert_eq!(delta_to_status(Delta::Renamed), FileStatus::Renamed);
        assert_eq!(delta_to_status(Delta::Copied), FileStatus::Copied);
        assert_eq!(delta_to_status(Delta::Typechange), FileStatus::TypeChange);
        assert_eq!(delta_to_status(Delta::Untracked), FileStatus::Untracked);
        assert_eq!(delta_to_status(Delta::Ignored), FileStatus::Unknown);
    }
}
