//! Git diff generation

use anyhow::{Context, Result};
use git2::{Delta, DiffOptions, Repository, Status, StatusOptions};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::hash::{Hash as _, Hasher as _};
use std::io::{Read as _, Seek as _};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::{fs, io};

fn diff_line_content(line: &git2::DiffLine<'_>) -> String {
    let content = String::from_utf8_lossy(line.content());
    content.trim_end_matches(['\n', '\r']).to_string()
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct StatusMarker {
    path: PathBuf,
    status_bits: u32,
    head_to_index: Option<DeltaMarker>,
    index_to_workdir: Option<DeltaMarker>,
    workdir_meta: Option<WorktreeMetaMarker>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DeltaMarker {
    status: u8,
    old_file: DiffFileMarker,
    new_file: DiffFileMarker,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct DiffFileMarker {
    path: Option<PathBuf>,
    oid: String,
    mode: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WorktreeMetaMarker {
    kind: u8,
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
    sample_hash: u64,
}

fn diff_file_marker(file: &git2::DiffFile<'_>) -> DiffFileMarker {
    DiffFileMarker {
        path: file.path().map(Path::to_path_buf),
        oid: file.id().to_string(),
        mode: file.mode().into(),
    }
}

const fn file_status_rank(status: FileStatus) -> u8 {
    match status {
        FileStatus::Added => 1,
        FileStatus::Deleted => 2,
        FileStatus::Modified => 3,
        FileStatus::Renamed => 4,
        FileStatus::Copied => 5,
        FileStatus::TypeChange => 6,
        FileStatus::Untracked => 7,
        FileStatus::Unknown => 8,
    }
}

fn delta_marker(delta: &git2::DiffDelta<'_>) -> DeltaMarker {
    DeltaMarker {
        status: file_status_rank(delta_to_status(delta.status())),
        old_file: diff_file_marker(&delta.old_file()),
        new_file: diff_file_marker(&delta.new_file()),
    }
}

fn status_has_workdir_change(status: Status) -> bool {
    status.intersects(
        Status::WT_NEW
            | Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::WT_TYPECHANGE,
    )
}

fn worktree_file_sample_hash(full_path: &Path, len: u64) -> u64 {
    const SAMPLE_BYTES: usize = 4096;
    const SAMPLE_BYTES_I64: i64 = 4096;

    let mut hasher = DefaultHasher::new();
    hasher.write_u64(len);

    let Ok(mut file) = fs::File::open(full_path) else {
        return hasher.finish();
    };

    let mut buf = [0_u8; SAMPLE_BYTES];
    let head_len = usize::try_from(len)
        .ok()
        .map_or(SAMPLE_BYTES, |v| v.min(SAMPLE_BYTES));
    if head_len > 0
        && let Ok(read) = file.read(&mut buf[..head_len])
    {
        hasher.write_usize(read);
        hasher.write(&buf[..read]);
    }

    if len > SAMPLE_BYTES as u64
        && file.seek(io::SeekFrom::End(-SAMPLE_BYTES_I64)).is_ok()
        && let Ok(read) = file.read(&mut buf[..SAMPLE_BYTES])
    {
        hasher.write_usize(read);
        hasher.write(&buf[..read]);
    }

    hasher.finish()
}

fn worktree_meta_marker(
    workdir: Option<&Path>,
    path: Option<&Path>,
    include_content_sample: bool,
) -> Option<WorktreeMetaMarker> {
    let path = path?;
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workdir?.join(path)
    };
    let metadata = std::fs::symlink_metadata(&full_path).ok()?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        1
    } else if file_type.is_dir() {
        2
    } else if file_type.is_symlink() {
        3
    } else {
        4
    };
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    let sample_hash = (include_content_sample && file_type.is_file())
        .then(|| worktree_file_sample_hash(&full_path, metadata.len()));

    Some(WorktreeMetaMarker {
        kind,
        len: metadata.len(),
        modified_secs: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
        sample_hash: sample_hash.unwrap_or(0),
    })
}

fn status_marker(entry: &git2::StatusEntry<'_>, workdir: Option<&Path>) -> StatusMarker {
    let status = entry.status();
    let include_content_sample =
        status_has_workdir_change(status) && !status.contains(Status::WT_NEW);
    let head_to_index = entry.head_to_index().map(|delta| delta_marker(&delta));
    let index_to_workdir = entry.index_to_workdir().map(|delta| delta_marker(&delta));
    let path = entry.path().map_or_else(PathBuf::new, PathBuf::from);
    let workdir_path = index_to_workdir
        .as_ref()
        .and_then(|delta| {
            delta
                .new_file
                .path
                .as_deref()
                .or(delta.old_file.path.as_deref())
        })
        .map(Path::to_path_buf)
        .or_else(|| (!path.as_os_str().is_empty()).then_some(path.clone()));

    StatusMarker {
        path,
        status_bits: status.bits(),
        head_to_index,
        index_to_workdir,
        workdir_meta: if status_has_workdir_change(status) {
            worktree_meta_marker(workdir, workdir_path.as_deref(), include_content_sample)
        } else {
            None
        },
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

fn push_model_hunk_line(
    file: &mut DiffFile,
    origin: char,
    content: String,
    old_lineno: Option<u32>,
    new_lineno: Option<u32>,
) {
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

    let idx = file.hunks.len() - 1;
    file.hunks[idx].lines.push(DiffHunkLine {
        origin,
        content,
        old_lineno,
        new_lineno,
    });
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
        let index_path = self.repo.path().join("index");
        if index_path.exists() && !index_path.is_file() {
            return Err(anyhow::anyhow!(
                "git index path is not a file: {}",
                index_path.display()
            ))
            .context("Failed to get staged diff");
        }

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

    /// Get a cheap marker hash for background diff polling.
    ///
    /// This intentionally avoids generating patch text. It hashes git status entries plus
    /// lightweight filesystem metadata for worktree changes, which keeps inactive-tab polling
    /// responsive even when large untracked build directories are present.
    ///
    /// # Errors
    ///
    /// Returns an error if repository status cannot be read.
    pub fn uncommitted_change_marker(&self) -> Result<u64> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true);
        opts.recurse_untracked_dirs(true);
        opts.renames_head_to_index(true);
        opts.renames_index_to_workdir(true);

        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .context("Failed to get repository status for diff marker")?;

        if statuses.is_empty() {
            return Ok(0);
        }

        let mut markers = statuses
            .iter()
            .map(|entry| status_marker(&entry, self.repo.workdir()))
            .collect::<Vec<_>>();
        markers.sort_unstable();

        let mut hasher = DefaultHasher::new();
        for marker in markers {
            marker.hash(&mut hasher);
        }
        Ok(hasher.finish())
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
            let old_path = delta.old_file().path();
            let empty_path = Path::new("");
            let file_path = delta.new_file().path().or(old_path).unwrap_or(empty_path);

            let file_idx = file_indices.get(file_path).copied().unwrap_or_else(|| {
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
            });
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
            let old_path = delta.old_file().path();
            let empty_path = Path::new("");
            let file_path = delta.new_file().path().or(old_path).unwrap_or(empty_path);

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
                    push_model_hunk_line(
                        file,
                        origin,
                        content,
                        line.old_lineno(),
                        line.new_lineno(),
                    );

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
            let old_path = delta.old_file().path();
            let empty_path = Path::new("");
            let file_path = delta.new_file().path().or(old_path).unwrap_or(empty_path);

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
