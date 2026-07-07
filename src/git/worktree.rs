//! Git worktree management

use anyhow::{Context, Result, bail};
use git2::Repository;
use git2::string_array::StringArray;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tracing::{debug, info, warn};

const LOCAL_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

#[cfg(any(test, coverage))]
thread_local! {
    static FORCE_REMOVE_DIR_ALL_WITH_RETRIES_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
    static FORCE_REPO_WORKTREES_ERROR: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
    static FORCE_REPO_HEAD_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FORCE_WORKTREE_PRUNE_ERROR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FORCE_REPO_FIND_WORKTREE_VERIFY_MISSING: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
fn with_forced_remove_dir_all_with_retries_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    FORCE_REMOVE_DIR_ALL_WITH_RETRIES_ERROR.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
pub(super) fn with_forced_repo_worktrees_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    FORCE_REPO_WORKTREES_ERROR.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
fn with_forced_repo_head_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    FORCE_REPO_HEAD_ERROR.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
fn with_forced_worktree_prune_error_for_tests<T>(f: impl FnOnce() -> T) -> T {
    FORCE_WORKTREE_PRUNE_ERROR.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
fn with_forced_repo_find_worktree_verify_missing_for_tests<T>(f: impl FnOnce() -> T) -> T {
    FORCE_REPO_FIND_WORKTREE_VERIFY_MISSING.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

fn remove_dir_all_with_retries(path: &Path) -> Result<()> {
    #[cfg(any(test, coverage))]
    if FORCE_REMOVE_DIR_ALL_WITH_RETRIES_ERROR.with(std::cell::Cell::get) {
        bail!("Forced remove_dir_all_with_retries failure");
    }

    if !path.exists() {
        return Ok(());
    }

    let mut last_err = "directory still exists".to_string();
    for attempt in 0u64..10 {
        match fs::remove_dir_all(path) {
            Ok(()) => break,
            Err(e) => last_err = e.to_string(),
        }

        std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));

        if !path.exists() {
            break;
        }
    }

    if path.exists() {
        bail!(
            "Failed to remove directory at {}: {}",
            path.display(),
            last_err
        );
    }

    Ok(())
}

fn repo_worktrees(repo: &Repository) -> std::result::Result<StringArray, git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_REPO_WORKTREES_ERROR.with(std::cell::Cell::get) {
        return Err(git2::Error::new(
            git2::ErrorCode::GenericError,
            git2::ErrorClass::Repository,
            "Forced repo.worktrees failure",
        ));
    }
    repo.worktrees()
}

fn repo_head(repo: &Repository) -> std::result::Result<git2::Reference<'_>, git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_REPO_HEAD_ERROR.with(std::cell::Cell::get) {
        return Err(git2::Error::new(
            git2::ErrorCode::GenericError,
            git2::ErrorClass::Reference,
            "Forced repo.head failure",
        ));
    }
    repo.head()
}

fn worktree_prune(
    worktree: &git2::Worktree,
    opts: &mut git2::WorktreePruneOptions,
) -> std::result::Result<(), git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_WORKTREE_PRUNE_ERROR.with(std::cell::Cell::get) {
        return Err(git2::Error::new(
            git2::ErrorCode::GenericError,
            git2::ErrorClass::Worktree,
            "Forced worktree.prune failure",
        ));
    }

    worktree.prune(Some(opts))
}

fn repo_find_worktree_is_ok(repo: &Repository, name: &str) -> bool {
    #[cfg(any(test, coverage))]
    if FORCE_REPO_FIND_WORKTREE_VERIFY_MISSING.with(std::cell::Cell::get) {
        return false;
    }

    repo.find_worktree(name).is_ok()
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("Failed to read directory {}", path.display()))?;
    Ok(entries.next().is_none())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TargetPreparationKind {
    Registered(PathBuf),
    Ready { cleaned_stale_target: bool },
}

/// Result of checking a worktree target before creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetPreparation {
    kind: TargetPreparationKind,
}

impl TargetPreparation {
    const fn registered(path: PathBuf) -> Self {
        Self {
            kind: TargetPreparationKind::Registered(path),
        }
    }

    const fn ready(cleaned_stale_target: bool) -> Self {
        Self {
            kind: TargetPreparationKind::Ready {
                cleaned_stale_target,
            },
        }
    }

    /// Returns the registered worktree path when git already knows this branch's worktree.
    #[must_use]
    pub fn registered_path(&self) -> Option<&Path> {
        match &self.kind {
            TargetPreparationKind::Registered(path) => Some(path),
            TargetPreparationKind::Ready { .. } => None,
        }
    }

    /// Returns true when preparation removed a stale Tenex-owned target directory.
    #[must_use]
    pub const fn cleaned_stale_target(&self) -> bool {
        match &self.kind {
            TargetPreparationKind::Registered(_) => false,
            TargetPreparationKind::Ready {
                cleaned_stale_target,
            } => *cleaned_stale_target,
        }
    }
}

fn normalize_ignored_rel_path(raw_path: &[u8]) -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::path::Component;

    if raw_path.is_empty() {
        return None;
    }

    let mut rel = String::from_utf8_lossy(raw_path).into_owned();
    rel.truncate(rel.trim_end_matches('/').len());

    if rel.is_empty() {
        return None;
    }

    let rel_path = PathBuf::from(rel);
    if rel_path.is_absolute()
        || rel_path.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return None;
    }

    let Some(Component::Normal(top)) = rel_path.components().next() else {
        return None;
    };
    if top == OsStr::new(".git") || top == OsStr::new(".tenex") {
        return None;
    }

    Some(rel_path)
}

fn list_ignored_rel_paths(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let output = super::git_command()
        .args([
            "ls-files",
            "-o",
            "-i",
            "--exclude-standard",
            "--directory",
            "-z",
        ])
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("Failed to list ignored files from {}", repo_root.display()))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git ls-files failed (stdout: {stdout}, stderr: {stderr})");
    }

    Ok(output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter_map(normalize_ignored_rel_path)
        .collect())
}

#[cfg(test)]
thread_local! {
    static TEST_DROP_GIT_CHECK_IGNORE_STDIN: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
    static TEST_YIELD_GIT_CHECK_IGNORE_BEFORE_DELIMITER_WRITE: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
    static TEST_GIT_CHECK_IGNORE_DELIMITER_READY_PATH: std::cell::RefCell<Option<PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
    static TEST_GIT_CHECK_IGNORE_DELIMITER_MAX_WAIT: std::cell::Cell<std::time::Duration> = const {
        std::cell::Cell::new(std::time::Duration::from_secs(2))
    };
    static TEST_PREWAIT_GIT_CHECK_IGNORE_CHILD: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
fn with_dropped_git_check_ignore_stdin_for_tests<T>(f: impl FnOnce() -> T) -> T {
    TEST_DROP_GIT_CHECK_IGNORE_STDIN.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
fn with_yield_before_git_check_ignore_delimiter_write_for_tests<T>(f: impl FnOnce() -> T) -> T {
    TEST_YIELD_GIT_CHECK_IGNORE_BEFORE_DELIMITER_WRITE.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
fn with_git_check_ignore_delimiter_ready_path_for_tests<T>(
    path: PathBuf,
    f: impl FnOnce() -> T,
) -> T {
    TEST_GIT_CHECK_IGNORE_DELIMITER_READY_PATH.with(|slot| {
        let previous = slot.replace(Some(path));
        let result = f();
        slot.replace(previous);
        result
    })
}

#[cfg(test)]
fn with_git_check_ignore_delimiter_max_wait_for_tests<T>(
    max_wait: std::time::Duration,
    f: impl FnOnce() -> T,
) -> T {
    TEST_GIT_CHECK_IGNORE_DELIMITER_MAX_WAIT.with(|slot| {
        let previous = slot.replace(max_wait);
        let result = f();
        slot.set(previous);
        result
    })
}

#[cfg(test)]
fn with_prewaited_git_check_ignore_child_for_tests<T>(f: impl FnOnce() -> T) -> T {
    TEST_PREWAIT_GIT_CHECK_IGNORE_CHILD.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

fn git_check_ignore_ignored_paths(
    repo_dir: &Path,
    rel_paths: &[PathBuf],
) -> Result<std::collections::HashSet<PathBuf>> {
    use std::io::Write;
    use std::process::Stdio;

    if rel_paths.is_empty() {
        return Ok(std::collections::HashSet::new());
    }

    let mut child = super::git_command()
        .args(["check-ignore", "-z", "--stdin"])
        .current_dir(repo_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn git check-ignore")?;

    {
        #[cfg(test)]
        if TEST_DROP_GIT_CHECK_IGNORE_STDIN.with(std::cell::Cell::get) {
            let _ = child.stdin.take();
        }

        let mut stdin = child.stdin.take().context("Failed to open git stdin")?;
        for rel_path in rel_paths {
            let rel_path_bytes = git_path_bytes(rel_path);
            stdin.write_all(&rel_path_bytes).with_context(|| {
                format!(
                    "Failed to write path {} to git check-ignore stdin",
                    rel_path.display()
                )
            })?;

            #[cfg(test)]
            if TEST_YIELD_GIT_CHECK_IGNORE_BEFORE_DELIMITER_WRITE.with(std::cell::Cell::get) {
                let ready_path = TEST_GIT_CHECK_IGNORE_DELIMITER_READY_PATH
                    .with(|slot| slot.borrow().as_ref().cloned());

                if let Some(ready_path) = ready_path {
                    let max_wait =
                        TEST_GIT_CHECK_IGNORE_DELIMITER_MAX_WAIT.with(std::cell::Cell::get);
                    let deadline = std::time::Instant::now() + max_wait;
                    while std::time::Instant::now() < deadline {
                        if ready_path.exists() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            stdin
                .write_all(b"\0")
                .context("Failed to write NUL delimiter to git check-ignore stdin")?;
        }
    }

    #[cfg(test)]
    if TEST_PREWAIT_GIT_CHECK_IGNORE_CHILD.with(std::cell::Cell::get) {
        #[cfg(unix)]
        {
            let pid = i32::from_ne_bytes(child.id().to_ne_bytes());
            let _ = nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(pid), None);
        }

        #[cfg(not(unix))]
        let _ = child.wait();
    }

    let output = child
        .wait_with_output()
        .context("Failed to read git check-ignore output")?;

    if !output.status.success() && output.status.code() != Some(1) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git check-ignore failed (stdout: {stdout}, stderr: {stderr})");
    }

    let mut fields = output
        .stdout
        .split(|byte| *byte == b'\0')
        .collect::<Vec<_>>();
    if fields.last().is_some_and(|value| value.is_empty()) {
        fields.pop();
    }

    Ok(fields
        .into_iter()
        .filter(|value| !value.is_empty())
        .map(git_path_from_bytes)
        .collect())
}

fn git_path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }

    #[cfg(not(unix))]
    {
        path.to_string_lossy().into_owned().into_bytes()
    }
}

fn git_path_from_bytes(value: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(OsString::from_vec(value.to_vec()))
    }

    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(value).into_owned())
    }
}

fn symlink_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)
    }

    #[cfg(windows)]
    {
        let metadata = fs::symlink_metadata(src)?;
        if metadata.file_type().is_dir() {
            std::os::windows::fs::symlink_dir(src, dst)
        } else {
            std::os::windows::fs::symlink_file(src, dst)
        }
    }
}

fn clone_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    let link_target = fs::read_link(src)?;

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&link_target, dst)
    }

    #[cfg(windows)]
    {
        let resolved_target = if link_target.is_absolute() {
            link_target.clone()
        } else {
            src.parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&link_target)
        };
        if fs::metadata(&resolved_target)?.is_dir() {
            std::os::windows::fs::symlink_dir(&link_target, dst)
        } else {
            std::os::windows::fs::symlink_file(&link_target, dst)
        }
    }
}

/// Options controlling how a new worktree is materialized on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateOptions {
    /// Whether ignored repo-root files should be linked into the new worktree.
    pub link_ignored_files: bool,
}

impl Default for CreateOptions {
    fn default() -> Self {
        Self {
            link_ignored_files: true,
        }
    }
}

impl CreateOptions {
    /// Return options for worktrees that should not inherit ignored repo-root file links.
    #[must_use]
    pub const fn without_ignored_file_links() -> Self {
        Self {
            link_ignored_files: false,
        }
    }
}

/// Manager for git worktree operations
pub struct Manager<'a> {
    repo: &'a Repository,
}

impl std::fmt::Debug for Manager<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manager").finish_non_exhaustive()
    }
}

impl<'a> Manager<'a> {
    /// Create a new worktree manager for the given repository
    #[must_use]
    pub const fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    /// Prepare a worktree target path before creating or reusing it.
    ///
    /// # Errors
    ///
    /// Returns an error when the path already exists but is not a safe Tenex-owned
    /// stale worktree target, or when stale cleanup fails.
    pub fn prepare_worktree_creation_target(
        &self,
        path: &Path,
        branch: &str,
        worktree_root: &Path,
    ) -> Result<TargetPreparation> {
        if let Some(registered_path) = self.worktree_path(branch) {
            return Ok(TargetPreparation::registered(registered_path));
        }

        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TargetPreparation::ready(false));
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("Failed to inspect worktree target {}", path.display())
                });
            }
        };
        if metadata.file_type().is_symlink() {
            bail!(
                "Cannot create Tenex worktree at {} because that path is a symlink; Tenex will not create a worktree at or traverse it",
                path.display()
            );
        }
        if metadata.is_file() {
            bail!(
                "Cannot create Tenex worktree at {} because that path is a file, not a directory Tenex can use",
                path.display()
            );
        }
        if !metadata.is_dir() {
            bail!(
                "Cannot create Tenex worktree at {} because that path is not a directory Tenex can use",
                path.display()
            );
        }
        if !Self::existing_path_is_inside_root(path, worktree_root) {
            bail!(
                "Cannot create Tenex worktree at {} because it is outside the configured Tenex worktree root {}; Tenex will not touch it automatically",
                path.display(),
                worktree_root.display()
            );
        }

        let worktree_name = branch.replace('/', "-");
        let stale_empty = is_empty_dir(path)?;
        let stale_owned = if stale_empty {
            false
        } else {
            self.target_dir_looks_like_stale_own_worktree(path, &worktree_name)?
        };

        if stale_empty || stale_owned {
            remove_dir_all_with_retries(path).with_context(|| {
                format!(
                    "Failed to remove stale Tenex worktree target {}",
                    path.display()
                )
            })?;
            return Ok(TargetPreparation::ready(true));
        }

        bail!(
            "Cannot create Tenex worktree at {} because that directory already exists and does not look like one of this repo's Tenex worktrees; Tenex will not delete or reuse it automatically",
            path.display()
        );
    }

    fn existing_path_is_inside_root(path: &Path, root: &Path) -> bool {
        let Ok(path) = path.canonicalize() else {
            return false;
        };
        let Ok(root) = root.canonicalize() else {
            return false;
        };

        path != root && path.starts_with(root)
    }

    fn target_dir_looks_like_stale_own_worktree(
        &self,
        path: &Path,
        worktree_name: &str,
    ) -> Result<bool> {
        let admin_dir = self.repo.path().join("worktrees").join(worktree_name);
        Self::target_git_file_points_to_admin_dir(path, &admin_dir)
    }

    fn target_git_file_points_to_admin_dir(path: &Path, admin_dir: &Path) -> Result<bool> {
        let git_file = path.join(".git");
        if !git_file.exists() {
            return Ok(false);
        }

        let metadata = fs::symlink_metadata(&git_file)
            .with_context(|| format!("Failed to inspect {}", git_file.display()))?;
        if !metadata.is_file() {
            return Ok(false);
        }

        let Some(gitdir) = Self::read_gitdir_pointer(&git_file, path)? else {
            return Ok(false);
        };

        Ok(Self::paths_match(&gitdir, admin_dir))
    }

    fn read_gitdir_pointer(path: &Path, relative_base: &Path) -> Result<Option<PathBuf>> {
        Self::read_resolved_gitdir(path, relative_base, |raw| {
            raw.trim()
                .strip_prefix("gitdir:")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
    }

    fn read_gitdir_path_file(path: &Path, relative_base: &Path) -> Result<Option<PathBuf>> {
        Self::read_resolved_gitdir(path, relative_base, |raw| {
            let gitdir = raw.trim();
            if gitdir.is_empty() {
                None
            } else {
                Some(gitdir.to_string())
            }
        })
    }

    fn read_resolved_gitdir(
        path: &Path,
        relative_base: &Path,
        parse: impl FnOnce(&str) -> Option<String>,
    ) -> Result<Option<PathBuf>> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Ok(parse(&raw).map(|gitdir| Self::resolve_gitdir_path(&gitdir, relative_base)))
    }

    fn resolve_gitdir_path(gitdir: &str, relative_base: &Path) -> PathBuf {
        let candidate = PathBuf::from(gitdir);
        if candidate.is_absolute() {
            candidate
        } else {
            relative_base.join(candidate)
        }
    }

    fn paths_match(left: &Path, right: &Path) -> bool {
        if let (Ok(left), Ok(right)) = (left.canonicalize(), right.canonicalize()) {
            return left == right;
        }

        Self::normalize_absolute_path(left) == Self::normalize_absolute_path(right)
    }

    fn normalize_absolute_path(path: &Path) -> PathBuf {
        Self::normalize_path_components(path)
    }

    fn normalize_path_components(path: &Path) -> PathBuf {
        use std::path::Component;

        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::Normal(part) => normalized.push(part),
                component @ (Component::Prefix(_) | Component::RootDir) => {
                    normalized.push(component.as_os_str());
                }
            }
        }
        normalized
    }

    /// Create a new worktree for a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be created
    pub fn create(&self, path: &Path, branch: &str) -> Result<()> {
        self.create_with_options(path, branch, CreateOptions::default())
    }

    /// Create a new worktree for a branch with explicit materialization options.
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be created.
    pub fn create_with_options(
        &self,
        path: &Path,
        branch: &str,
        options: CreateOptions,
    ) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory {}", parent.display())
            })?;
        }

        let branch_ref = self
            .repo
            .find_branch(branch, git2::BranchType::Local)
            .with_context(|| format!("Branch not found: {branch}"))?;

        let reference = branch_ref.into_reference();

        // Worktree name cannot contain slashes (it becomes a directory name in .git/worktrees/)
        let worktree_name = branch.replace('/', "-");

        let mut add_opts = git2::WorktreeAddOptions::new();
        add_opts.reference(Some(&reference));

        if let Err(err) = self.repo.worktree(&worktree_name, path, Some(&add_opts)) {
            if Self::should_force_worktree_add(path, &worktree_name, &err) {
                self.create_with_git_force(path, branch)
                    .with_context(|| format!("Failed to create worktree at {}", path.display()))?;
            } else {
                return Err(err)
                    .with_context(|| format!("Failed to create worktree at {}", path.display()));
            }
        }

        self.finish_worktree_create(path, options);

        Ok(())
    }

    fn should_force_worktree_add(path: &Path, worktree_name: &str, err: &git2::Error) -> bool {
        if err.class() != git2::ErrorClass::Worktree {
            return false;
        }
        if !err.message().contains("already checked out") {
            return false;
        }

        let Some(leaf) = path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };

        leaf == worktree_name
    }

    fn create_with_git_force(&self, path: &Path, branch: &str) -> Result<()> {
        let repo_root = self
            .repo
            .workdir()
            .context("Repository has no working directory")?;

        let output = super::git_command()
            .args(["worktree", "add", "--force"])
            .arg(path)
            .arg(branch)
            .current_dir(repo_root)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("Failed to run git worktree add for branch '{branch}'"))?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree add failed (stdout: {stdout}, stderr: {stderr})");
        }

        Ok(())
    }

    /// Create a worktree with a new branch from HEAD
    ///
    /// If the branch already exists (e.g., from a previous run), it will be deleted
    /// and recreated from HEAD.
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree or branch cannot be created
    pub fn create_with_new_branch(&self, path: &Path, branch: &str) -> Result<()> {
        self.create_with_new_branch_with_options(path, branch, CreateOptions::default())
    }

    /// Create a new worktree with a new branch from HEAD using explicit materialization options.
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree or branch cannot be created.
    pub fn create_with_new_branch_with_options(
        &self,
        path: &Path,
        branch: &str,
        options: CreateOptions,
    ) -> Result<()> {
        debug!(branch, ?path, "Creating worktree with new branch");

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory {}", parent.display())
            })?;
        }

        let head = self.repo.head().context("Failed to get HEAD")?;
        let commit = head.peel_to_commit().context("Failed to get HEAD commit")?;

        // Clean up any existing worktree/branch with this name (from a previous run)
        // Must remove worktree first, as the branch can't be deleted while it's
        // the HEAD of a linked worktree
        let worktree_name = branch.replace('/', "-");
        if let Ok(worktree) = self.repo.find_worktree(&worktree_name) {
            debug!(branch, "Removing existing worktree before recreation");
            let wt_path = worktree.path().to_path_buf();
            let _ = worktree.prune(Some(
                git2::WorktreePruneOptions::new()
                    .valid(true)
                    .working_tree(true),
            ));
            if wt_path.exists() {
                let _ = fs::remove_dir_all(&wt_path);
            }
        }

        self.cleanup_orphaned_worktree_admin_dir(&worktree_name)?;

        // Now delete the branch if it exists
        let branch_mgr = super::BranchManager::new(self.repo);
        if branch_mgr.exists(branch) {
            debug!(branch, "Deleting existing branch before recreation");
            let _ = branch_mgr.delete(branch);
        }

        let branch_ref = self
            .repo
            .branch(branch, &commit, false)
            .with_context(|| format!("Failed to create branch '{branch}'"))?;

        let reference = branch_ref.into_reference();

        self.repo
            .worktree(
                &worktree_name,
                path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&reference))),
            )
            .with_context(|| format!("Failed to create worktree at {}", path.display()))?;

        self.finish_worktree_create(path, options);

        info!(branch, ?path, "Worktree created");
        Ok(())
    }

    fn finish_worktree_create(&self, path: &Path, options: CreateOptions) {
        if options.link_ignored_files
            && let Err(err) = self.symlink_ignored_files_into_worktree(path)
        {
            warn!(?path, error = %err, "Failed to symlink ignored files into worktree");
        }

        if let Err(err) = self.symlink_local_instruction_files_into_worktree(path) {
            warn!(?path, error = %err, "Failed to symlink local instruction files into worktree");
        }
    }

    fn symlink_local_instruction_files_into_worktree(&self, worktree_path: &Path) -> Result<()> {
        let Some(repo_root) = self.repo.workdir() else {
            return Ok(());
        };

        for file_name in LOCAL_INSTRUCTION_FILE_NAMES {
            Self::symlink_local_instruction_file(repo_root, worktree_path, file_name)?;
        }

        Ok(())
    }

    fn symlink_local_instruction_file(
        repo_root: &Path,
        worktree_path: &Path,
        file_name: &str,
    ) -> Result<()> {
        let src = repo_root.join(file_name);
        let Ok(src_meta) = fs::symlink_metadata(&src) else {
            return Ok(());
        };

        let dst = worktree_path.join(file_name);
        if fs::symlink_metadata(&dst).is_ok() {
            return Ok(());
        }

        if src_meta.file_type().is_symlink() {
            clone_symlink(&src, &dst).with_context(|| {
                format!(
                    "Failed to symlink local instruction file {} -> {}",
                    dst.display(),
                    src.display()
                )
            })?;
            return Ok(());
        }

        if !src_meta.is_file() {
            return Ok(());
        }

        symlink_path(&src, &dst).with_context(|| {
            format!(
                "Failed to symlink local instruction file {} -> {}",
                dst.display(),
                src.display()
            )
        })?;

        Ok(())
    }

    fn cleanup_orphaned_worktree_admin_dir(&self, worktree_name: &str) -> Result<()> {
        #[derive(Clone, Debug)]
        enum RemoveReason {
            Orphaned,
            MissingGitdir,
            EmptyGitdir,
            Stale { resolved_gitdir: PathBuf },
        }

        let admin_dir = self.repo.path().join("worktrees").join(worktree_name);
        if !admin_dir.exists() {
            return Ok(());
        }

        if !admin_dir.is_dir() {
            bail!(
                "Worktree admin path exists but is not a directory: {}",
                admin_dir.display()
            );
        }

        let reason = if is_empty_dir(&admin_dir)? {
            Some(RemoveReason::Orphaned)
        } else {
            let gitdir_path = admin_dir.join("gitdir");
            if gitdir_path.exists() {
                Self::read_gitdir_path_file(&gitdir_path, &admin_dir)?.map_or(
                    Some(RemoveReason::EmptyGitdir),
                    |resolved| {
                        if resolved.exists() {
                            None
                        } else {
                            Some(RemoveReason::Stale {
                                resolved_gitdir: resolved,
                            })
                        }
                    },
                )
            } else {
                Some(RemoveReason::MissingGitdir)
            }
        };

        if let Some(reason) = reason {
            match &reason {
                RemoveReason::Orphaned => {
                    warn!(?admin_dir, "Removing orphaned worktree admin directory");
                }
                RemoveReason::MissingGitdir => {
                    warn!(
                        ?admin_dir,
                        "Removing invalid worktree admin directory (missing gitdir)"
                    );
                }
                RemoveReason::EmptyGitdir => {
                    warn!(
                        ?admin_dir,
                        "Removing invalid worktree admin directory (empty gitdir)"
                    );
                }
                RemoveReason::Stale { resolved_gitdir } => {
                    warn!(
                        ?admin_dir,
                        gitdir = %resolved_gitdir.display(),
                        "Removing stale worktree admin directory"
                    );
                }
            }
            remove_dir_all_with_retries(&admin_dir)?;
        }

        Ok(())
    }

    /// Remove a worktree and its associated branch
    ///
    /// Always attempts to delete the branch, even if the worktree is missing.
    /// This ensures cleanup works even if the worktree was manually removed.
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree exists but cannot be removed after retries.
    /// Does not return errors for missing worktrees or branches.
    pub fn remove(&self, name: &str) -> Result<()> {
        debug!(name, "Removing worktree and branch");

        // Worktree name has slashes replaced with dashes
        let worktree_name = name.replace('/', "-");

        // Try to remove the worktree with retries (may already be gone)
        if let Ok(worktree) = self.repo.find_worktree(&worktree_name) {
            let wt_path = worktree.path().to_path_buf();

            // Retry prune up to 3 times with increasing delays
            // This handles race conditions where processes are still terminating
            let mut prune_succeeded = false;
            for attempt in 0u64..3 {
                if attempt > 0 {
                    debug!(name, attempt, "Retrying worktree prune");
                    std::thread::sleep(std::time::Duration::from_millis(100 * attempt));
                }

                let mut opts = git2::WorktreePruneOptions::new();
                opts.valid(true).working_tree(true);
                match worktree_prune(&worktree, &mut opts) {
                    Ok(()) => {
                        prune_succeeded = true;
                        break;
                    }
                    Err(e) => {
                        debug!(name, error = %e, attempt, "Worktree prune failed");
                    }
                }
            }

            // Ensure libgit2 doesn't keep the worktree alive on platforms with strict file locking.
            drop(worktree);

            // Always try to remove the directory even if prune failed (may take time for processes
            // to release handles).
            if let Err(e) = remove_dir_all_with_retries(&wt_path) {
                warn!(name, path = ?wt_path, error = %e, "Failed to remove worktree directory");
                return Err(e);
            }

            // Verify the worktree is actually gone from git's perspective
            if !prune_succeeded {
                // Check if it's still registered with git
                if self.repo.find_worktree(&worktree_name).is_ok() {
                    warn!(name, "Worktree still exists in git after prune attempts");
                    return Err(anyhow::anyhow!(
                        "Failed to remove worktree '{name}' - it may still be in use"
                    ));
                }
            }

            debug!(name, "Worktree pruned");
        }

        // Always try to delete the branch (critical for cleanup)
        // Ignore errors - branch may already be deleted or checked out elsewhere
        let branch_mgr = super::BranchManager::new(self.repo);
        let _ = branch_mgr.delete(name);

        info!(name, "Worktree removed");
        Ok(())
    }

    /// Remove a worktree but keep its associated branch.
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree exists but cannot be removed after retries.
    /// Does not return errors for missing worktrees.
    pub fn remove_worktree_only(&self, name: &str) -> Result<()> {
        debug!(name, "Removing worktree (keeping branch)");

        // Worktree name has slashes replaced with dashes
        let worktree_name = name.replace('/', "-");

        // Try to remove the worktree with retries (may already be gone)
        if let Ok(worktree) = self.repo.find_worktree(&worktree_name) {
            let wt_path = worktree.path().to_path_buf();

            // Retry prune up to 3 times with increasing delays
            // This handles race conditions where processes are still terminating
            let mut prune_succeeded = false;
            for attempt in 0u64..3 {
                if attempt > 0 {
                    debug!(name, attempt, "Retrying worktree prune");
                    std::thread::sleep(std::time::Duration::from_millis(100 * attempt));
                }

                let mut opts = git2::WorktreePruneOptions::new();
                opts.valid(true).working_tree(true);
                match worktree_prune(&worktree, &mut opts) {
                    Ok(()) => {
                        prune_succeeded = true;
                        break;
                    }
                    Err(e) => {
                        debug!(name, error = %e, attempt, "Worktree prune failed");
                    }
                }
            }

            // Ensure libgit2 doesn't keep the worktree alive on platforms with strict file locking.
            drop(worktree);

            // Always try to remove the directory even if prune failed (may take time for processes
            // to release handles).
            if let Err(e) = remove_dir_all_with_retries(&wt_path) {
                warn!(name, path = ?wt_path, error = %e, "Failed to remove worktree directory");
                return Err(e);
            }

            // Verify the worktree is actually gone from git's perspective
            if !prune_succeeded && repo_find_worktree_is_ok(self.repo, &worktree_name) {
                warn!(name, "Worktree still exists in git after prune attempts");
                return Err(anyhow::anyhow!(
                    "Failed to remove worktree '{name}' - it may still be in use"
                ));
            }

            debug!(name, "Worktree pruned");
        }

        info!(name, "Worktree removed (branch preserved)");
        Ok(())
    }

    /// List all worktrees
    ///
    /// # Errors
    ///
    /// Returns an error if worktrees cannot be listed
    pub fn list(&self) -> Result<Vec<Info>> {
        let worktrees = repo_worktrees(self.repo).context("Failed to list worktrees")?;

        let mut infos = Vec::new();
        for name in worktrees.iter().flatten() {
            self.push_worktree_info_if_openable(&mut infos, name);
        }

        Ok(infos)
    }

    fn push_worktree_info_if_openable(&self, infos: &mut Vec<Info>, name: &str) {
        if let Ok(wt) = self.repo.find_worktree(name) {
            let is_locked = matches!(wt.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_)));
            infos.push(Info {
                name: name.to_string(),
                path: wt.path().to_path_buf(),
                is_locked,
            });
        }
    }

    /// Check if a worktree exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        let worktree_name = name.replace('/', "-");
        self.repo.find_worktree(&worktree_name).is_ok()
    }

    /// Returns the filesystem path for a worktree, if present.
    #[must_use]
    pub fn worktree_path(&self, name: &str) -> Option<PathBuf> {
        let worktree_name = name.replace('/', "-");
        self.repo
            .find_worktree(&worktree_name)
            .ok()
            .map(|wt| wt.path().to_path_buf())
    }

    /// Lock a worktree to prevent it from being pruned
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be locked
    pub fn lock(&self, name: &str, reason: Option<&str>) -> Result<()> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .context(format!("Worktree not found: {name}"))?;

        worktree
            .lock(reason)
            .context(format!("Failed to lock worktree '{name}'"))?;

        Ok(())
    }

    /// Unlock a worktree
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be unlocked
    pub fn unlock(&self, name: &str) -> Result<()> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .context(format!("Worktree not found: {name}"))?;

        let is_locked = matches!(
            worktree.is_locked(),
            Ok(git2::WorktreeLockStatus::Locked(_))
        );
        if !is_locked {
            bail!("Worktree '{name}' is not locked");
        }

        worktree
            .unlock()
            .context(format!("Failed to unlock worktree '{name}'"))?;

        Ok(())
    }

    /// Validate a worktree (check if it's valid)
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree is invalid
    pub fn validate(&self, name: &str) -> Result<()> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .context(format!("Worktree not found: {name}"))?;

        worktree
            .validate()
            .context(format!("Worktree '{name}' is invalid"))?;

        Ok(())
    }

    /// Get the HEAD commit information for the main repository
    ///
    /// Returns (`branch_name`, `short_commit_hash`)
    ///
    /// # Errors
    ///
    /// Returns an error if HEAD cannot be read
    pub fn head_info(&self) -> Result<(String, String)> {
        let head = self.repo.head().context("Failed to get HEAD")?;
        let commit = head.peel_to_commit().context("Failed to get HEAD commit")?;

        let branch_name = if head.is_branch() {
            head.shorthand().unwrap_or("HEAD").to_string()
        } else {
            "HEAD (detached)".to_string()
        };

        let short_hash = commit.id().to_string()[..7].to_string();

        Ok((branch_name, short_hash))
    }

    /// Get the HEAD commit information for an existing worktree
    ///
    /// Returns (`branch_name`, `short_commit_hash`) if the worktree exists and has a valid HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree or its HEAD cannot be read
    pub fn worktree_head_info(&self, name: &str) -> Result<(String, String)> {
        let worktree_name = name.replace('/', "-");
        let worktree = self
            .repo
            .find_worktree(&worktree_name)
            .with_context(|| format!("Worktree not found: {name}"))?;

        // Open the worktree as a repository to get its HEAD
        let wt_repo = git2::Repository::open(worktree.path()).with_context(|| {
            format!(
                "Failed to open worktree repository at {}",
                worktree.path().display()
            )
        })?;

        let head = repo_head(&wt_repo).context("Failed to get worktree HEAD")?;
        let commit = head
            .peel_to_commit()
            .context("Failed to get worktree HEAD commit")?;

        let branch_name = if head.is_branch() {
            head.shorthand().unwrap_or("HEAD").to_string()
        } else {
            "HEAD (detached)".to_string()
        };

        let short_hash = commit.id().to_string()[..7].to_string();

        Ok((branch_name, short_hash))
    }

    fn symlink_ignored_files_into_worktree(&self, worktree_path: &Path) -> Result<()> {
        let Some(repo_root) = self.repo.workdir() else {
            return Ok(());
        };

        let rel_paths = list_ignored_rel_paths(repo_root)?;
        let ignored_as_file_in_worktree =
            git_check_ignore_ignored_paths(worktree_path, &rel_paths)?;

        for rel_path in rel_paths {
            Self::symlink_ignored_path(
                repo_root,
                worktree_path,
                &rel_path,
                &ignored_as_file_in_worktree,
            )?;
        }

        Ok(())
    }

    fn symlink_ignored_path(
        repo_root: &Path,
        worktree_path: &Path,
        rel_path: &Path,
        ignored_as_file_in_worktree: &std::collections::HashSet<PathBuf>,
    ) -> Result<()> {
        if !ignored_as_file_in_worktree.contains(rel_path) {
            debug!(
                "Skipping ignored path {} because it is not ignored as a file in the worktree",
                rel_path.display()
            );
            return Ok(());
        }

        let src = repo_root.join(rel_path);
        let Ok(src_meta) = fs::symlink_metadata(&src) else {
            return Ok(());
        };

        if src_meta.file_type().is_symlink() {
            return Ok(());
        }

        if worktree_path.starts_with(&src) {
            return Ok(());
        }

        let dst = worktree_path.join(rel_path);
        if fs::symlink_metadata(&dst).is_ok() {
            return Ok(());
        }

        let Some(parent) = dst.parent() else {
            return Ok(());
        };

        let parent_is_symlink = fs::symlink_metadata(parent)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if parent_is_symlink {
            return Ok(());
        }

        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory {} for ignored file link",
                parent.display()
            )
        })?;

        symlink_path(&src, &dst).with_context(|| {
            format!(
                "Failed to symlink ignored path {} -> {}",
                dst.display(),
                src.display()
            )
        })?;

        Ok(())
    }
}

/// Information about a worktree
#[derive(Debug, Clone)]
pub struct Info {
    /// Name of the worktree (usually branch name)
    pub name: String,
    /// Path to the worktree directory
    pub path: PathBuf,
    /// Whether the worktree is locked
    pub is_locked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::with_git_program_override_for_tests;
    use git2::Signature;
    use git2::{ErrorClass, ErrorCode};
    use tempfile::TempDir;

    fn init_test_repo_with_commit() -> (TempDir, Repository) {
        let temp_dir = TempDir::new().unwrap();
        let mut init_opts = git2::RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();

        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (temp_dir, repo)
    }

    #[cfg(unix)]
    fn write_fake_git_script(temp: &TempDir, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let script = temp.path().join("git");
        fs::write(&script, body).unwrap();
        let mut perms = fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).unwrap();
        script
    }

    fn with_tracing_dispatch<T>(f: impl FnOnce() -> T) -> T {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(std::io::sink)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, f)
    }

    #[test]
    fn test_normalize_ignored_rel_path_covers_rejections_and_trimming() {
        assert_eq!(normalize_ignored_rel_path(b""), None);
        assert_eq!(normalize_ignored_rel_path(b"/"), None);
        assert_eq!(normalize_ignored_rel_path(b"////"), None);
        assert_eq!(normalize_ignored_rel_path(b"/abs/path"), None);
        assert_eq!(normalize_ignored_rel_path(b"../parent"), None);
        assert_eq!(normalize_ignored_rel_path(b"foo/../bar"), None);
        assert_eq!(normalize_ignored_rel_path(b"./foo"), None);
        assert_eq!(normalize_ignored_rel_path(b".git/config"), None);
        assert_eq!(normalize_ignored_rel_path(b".tenex/state.json"), None);
        assert_eq!(
            normalize_ignored_rel_path(b"ignored.txt/"),
            Some(PathBuf::from("ignored.txt"))
        );
        assert_eq!(
            normalize_ignored_rel_path(b"ignored_dir//"),
            Some(PathBuf::from("ignored_dir"))
        );
        assert_eq!(
            normalize_ignored_rel_path(b"nested/ignored.log"),
            Some(PathBuf::from("nested/ignored.log"))
        );
    }

    #[test]
    fn test_remove_dir_all_with_retries_bails_when_path_is_not_a_directory() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("not-a-dir");
        fs::write(&file_path, "payload").unwrap();

        let err = remove_dir_all_with_retries(&file_path).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Failed to remove directory at"));
        assert!(message.contains(&file_path.display().to_string()));
    }

    #[test]
    fn test_remove_dir_all_with_retries_breaks_when_path_removed_between_attempts() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("removed-mid-flight");
        fs::write(&file_path, "payload").unwrap();
        assert!(file_path.exists());

        let file_path_for_thread = file_path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _ = fs::remove_file(&file_path_for_thread);
        });

        remove_dir_all_with_retries(&file_path).unwrap();
        assert!(!file_path.exists());
    }

    fn admin_dir_for_branch(repo: &Repository, branch: &str) -> PathBuf {
        repo.path().join("worktrees").join(branch.replace('/', "-"))
    }

    #[test]
    fn test_prepare_worktree_creation_target_reports_missing_path_ready() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("missing");

        let branch = "feature/missing";
        let root = worktree_root.path();
        let preparation = manager.prepare_worktree_creation_target(&path, branch, root)?;

        assert!(preparation.registered_path().is_none());
        assert!(!preparation.cleaned_stale_target());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_reports_registered_worktree() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("registered");
        let branch = "feature/registered";
        manager.create_with_new_branch(&path, branch)?;

        let preparation =
            manager.prepare_worktree_creation_target(&path, branch, worktree_root.path())?;
        let registered_path = preparation
            .registered_path()
            .ok_or_else(|| anyhow::anyhow!("missing registered path"))?;

        assert_eq!(registered_path.canonicalize()?, path.canonicalize()?);
        assert!(!preparation.cleaned_stale_target());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_removes_empty_stale_dir() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("empty-stale");
        let branch = "feature/empty-stale";
        fs::create_dir_all(&path)?;

        let preparation =
            manager.prepare_worktree_creation_target(&path, branch, worktree_root.path())?;

        assert!(preparation.registered_path().is_none());
        assert!(preparation.cleaned_stale_target());
        assert!(!path.exists());
        manager.create_with_new_branch(&path, branch)?;
        assert!(path.join(".git").is_file());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_removes_target_git_file_match() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("git-file-stale");
        let branch = "feature/git-file-stale";
        let admin_dir = admin_dir_for_branch(&repo, branch);
        fs::create_dir_all(&admin_dir)?;
        fs::create_dir_all(&path)?;
        let gitdir = format!("gitdir: {}\n", admin_dir.display());
        fs::write(path.join(".git"), gitdir)?;
        fs::write(path.join("payload.txt"), "stale")?;

        let preparation =
            manager.prepare_worktree_creation_target(&path, branch, worktree_root.path())?;

        assert!(preparation.cleaned_stale_target());
        assert!(!path.exists());
        manager.create_with_new_branch(&path, branch)?;
        assert!(path.join(".git").is_file());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_removes_missing_admin_git_file_match() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("missing-admin-stale");
        let branch = "feature/missing-admin-stale";
        let admin_dir = admin_dir_for_branch(&repo, branch);
        fs::create_dir_all(&path)?;
        let gitdir = format!("gitdir: {}\n", admin_dir.display());
        fs::write(path.join(".git"), gitdir)?;
        fs::write(path.join("payload.txt"), "stale")?;

        let preparation =
            manager.prepare_worktree_creation_target(&path, branch, worktree_root.path())?;

        assert!(preparation.cleaned_stale_target());
        assert!(!path.exists());
        manager.create_with_new_branch(&path, branch)?;
        assert!(path.join(".git").is_file());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_non_empty_admin_gitdir_file_match_without_target_git_file()
    -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("admin-gitdir-file-stale");
        let branch = "feature/admin-gitdir-file-stale";
        let admin_dir = admin_dir_for_branch(&repo, branch);
        fs::create_dir_all(&admin_dir)?;
        fs::create_dir_all(&path)?;
        let gitdir = path.join(".git").display().to_string();
        fs::write(admin_dir.join("gitdir"), gitdir)?;
        fs::write(path.join("payload.txt"), "stale")?;

        let err = manager
            .prepare_worktree_creation_target(&path, branch, worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected admin-only foreign path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("does not look like one of this repo's Tenex worktrees"));
        assert!(path.join("payload.txt").exists());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_removes_empty_admin_gitdir_target_match() -> Result<()>
    {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("admin-gitdir-target-stale");
        let branch = "feature/admin-gitdir-target-stale";
        let admin_dir = admin_dir_for_branch(&repo, branch);
        fs::create_dir_all(&admin_dir)?;
        fs::create_dir_all(&path)?;
        fs::write(admin_dir.join("gitdir"), path.display().to_string())?;

        let preparation =
            manager.prepare_worktree_creation_target(&path, branch, worktree_root.path())?;

        assert!(preparation.cleaned_stale_target());
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_removes_relative_target_git_file_match() -> Result<()>
    {
        let (repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = repo_dir.path().join("tenex-worktrees");
        let path = worktree_root.join("relative-gitdir-stale");
        let branch = "feature/relative-gitdir-stale";
        let worktree_name = branch.replace('/', "-");
        let admin_dir = admin_dir_for_branch(&repo, branch);
        fs::create_dir_all(&admin_dir)?;
        fs::create_dir_all(&path)?;
        let gitdir = format!("gitdir: ../../.git/worktrees/{worktree_name}\n");
        fs::write(path.join(".git"), gitdir)?;
        fs::write(path.join("payload.txt"), "stale")?;

        let preparation =
            manager.prepare_worktree_creation_target(&path, branch, &worktree_root)?;

        assert!(preparation.cleaned_stale_target());
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_foreign_non_empty_dir() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("foreign");
        fs::create_dir_all(&path)?;
        fs::write(path.join("payload.txt"), "foreign")?;

        let err = manager
            .prepare_worktree_creation_target(&path, "feature/foreign", worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected foreign path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("does not look like one of this repo's Tenex worktrees"));
        assert!(path.join("payload.txt").exists());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_file_path() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("file-target");
        fs::write(&path, "not a directory")?;

        let err = manager
            .prepare_worktree_creation_target(&path, "feature/file-target", worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected file path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("is a file, not a directory"));
        assert!(path.is_file());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_worktree_creation_target_rejects_dangling_symlink_path() -> Result<()> {
        use std::os::unix::fs::symlink;

        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("dangling-symlink-target");
        symlink(worktree_root.path().join("missing-destination"), &path)?;

        let err = manager
            .prepare_worktree_creation_target(
                &path,
                "feature/dangling-symlink",
                worktree_root.path(),
            )
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected symlink path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("is a symlink"));
        assert!(fs::symlink_metadata(&path)?.file_type().is_symlink());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_worktree_creation_target_rejects_outside_symlink_path() -> Result<()> {
        use std::os::unix::fs::symlink;

        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let outside = TempDir::new()?;
        let path = worktree_root.path().join("outside-symlink-target");
        symlink(outside.path(), &path)?;

        let err = manager
            .prepare_worktree_creation_target(
                &path,
                "feature/outside-symlink",
                worktree_root.path(),
            )
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected symlink path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("is a symlink"));
        assert!(outside.path().exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_worktree_creation_target_rejects_inside_symlink_path() -> Result<()> {
        use std::os::unix::fs::symlink;

        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let destination = worktree_root.path().join("inside-destination");
        let path = worktree_root.path().join("inside-symlink-target");
        fs::create_dir_all(&destination)?;
        symlink(&destination, &path)?;

        let err = manager
            .prepare_worktree_creation_target(&path, "feature/inside-symlink", worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected symlink path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("is a symlink"));
        assert!(destination.exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_worktree_creation_target_rejects_socket_path() -> Result<()> {
        use std::os::unix::net::UnixListener;

        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("socket-target");
        let listener = UnixListener::bind(&path)?;

        let err = manager
            .prepare_worktree_creation_target(&path, "feature/socket", worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected socket path error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("is not a directory Tenex can use"));
        drop(listener);
        fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_outside_worktree_root() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let outside = TempDir::new()?;
        let path = outside.path().join("stale");
        fs::create_dir_all(&path)?;

        let err = manager
            .prepare_worktree_creation_target(&path, "feature/outside", worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected outside root error"))?;
        let message = err.to_string();

        assert!(message.contains(&path.display().to_string()));
        assert!(message.contains("outside the configured Tenex worktree root"));
        assert!(path.exists());
        Ok(())
    }

    #[test]
    fn test_existing_path_is_inside_root_rejects_uncanonicalizable_inputs() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let existing_path = temp_dir.path().join("path");
        let missing_path = temp_dir.path().join("missing-path");
        let missing_root = temp_dir.path().join("missing-root");
        fs::create_dir_all(&existing_path)?;

        assert!(!Manager::existing_path_is_inside_root(
            &missing_path,
            temp_dir.path()
        ));
        assert!(!Manager::existing_path_is_inside_root(
            &existing_path,
            &missing_root
        ));
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_git_dir_marker_as_foreign() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("git-dir-marker");
        fs::create_dir_all(path.join(".git"))?;

        let err = manager
            .prepare_worktree_creation_target(&path, "feature/git-dir-marker", worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected git dir marker error"))?;

        assert!(err.to_string().contains("does not look like"));
        assert!(path.join(".git").is_dir());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_invalid_git_file_marker() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("invalid-git-file");
        fs::create_dir_all(&path)?;
        fs::write(path.join(".git"), "not a gitdir pointer")?;

        let err = manager
            .prepare_worktree_creation_target(
                &path,
                "feature/invalid-git-file",
                worktree_root.path(),
            )
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected invalid git file error"))?;

        assert!(err.to_string().contains("does not look like"));
        assert!(path.join(".git").is_file());
        Ok(())
    }

    #[test]
    fn test_prepare_worktree_creation_target_rejects_empty_admin_gitdir_file() -> Result<()> {
        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("empty-admin-gitdir");
        let branch = "feature/empty-admin-gitdir";
        let admin_dir = admin_dir_for_branch(&repo, branch);
        fs::create_dir_all(&admin_dir)?;
        fs::write(admin_dir.join("gitdir"), "\n")?;
        fs::create_dir_all(&path)?;
        fs::write(path.join("payload.txt"), "foreign")?;

        let err = manager
            .prepare_worktree_creation_target(&path, branch, worktree_root.path())
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected empty admin gitdir error"))?;

        assert!(err.to_string().contains("does not look like"));
        assert!(path.join("payload.txt").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_prepare_worktree_creation_target_reports_stale_removal_errors() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let (_repo_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);
        let worktree_root = TempDir::new()?;
        let path = worktree_root.path().join("unremovable-empty");
        fs::create_dir_all(&path)?;
        fs::set_permissions(worktree_root.path(), fs::Permissions::from_mode(0o500))?;

        let result = manager.prepare_worktree_creation_target(
            &path,
            "feature/unremovable",
            worktree_root.path(),
        );
        fs::set_permissions(worktree_root.path(), fs::Permissions::from_mode(0o700))?;
        let err = result
            .err()
            .ok_or_else(|| anyhow::anyhow!("expected stale removal error"))?;
        let message = err.to_string();

        assert!(message.contains("Failed to remove stale Tenex worktree target"));
        assert!(message.contains(&path.display().to_string()));
        assert!(path.exists());
        Ok(())
    }

    #[test]
    fn test_normalize_path_components_covers_dot_and_parent_segments() {
        let normalized = Manager::normalize_path_components(Path::new("./tmp/../worktree"));
        assert_eq!(normalized, PathBuf::from("worktree"));
    }

    #[test]
    fn test_is_empty_dir_reports_error_context_for_non_directory() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("not-a-directory");
        fs::write(&file_path, "payload").unwrap();

        let err = is_empty_dir(&file_path).unwrap_err();
        assert!(err.to_string().contains("Failed to read directory"));
    }

    #[test]
    fn test_should_force_worktree_add_covers_error_filters() {
        let mismatched_class = git2::Error::new(
            ErrorCode::GenericError,
            ErrorClass::Checkout,
            "already checked out",
        );
        assert!(!Manager::should_force_worktree_add(
            Path::new("worktrees/feature"),
            "feature",
            &mismatched_class
        ));

        let mismatched_message =
            git2::Error::new(ErrorCode::GenericError, ErrorClass::Worktree, "other error");
        assert!(!Manager::should_force_worktree_add(
            Path::new("worktrees/feature"),
            "feature",
            &mismatched_message
        ));

        let root_path = git2::Error::new(
            ErrorCode::GenericError,
            ErrorClass::Worktree,
            "already checked out",
        );
        assert!(!Manager::should_force_worktree_add(
            Path::new("/"),
            "feature",
            &root_path
        ));

        assert!(!Manager::should_force_worktree_add(
            Path::new("worktrees/not-feature"),
            "feature",
            &root_path
        ));

        assert!(Manager::should_force_worktree_add(
            Path::new("worktrees/feature"),
            "feature",
            &root_path
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_list_ignored_rel_paths_errors_when_git_ls_files_fails() {
        let temp_dir = TempDir::new().unwrap();
        let fake_git = write_fake_git_script(
            &temp_dir,
            "#!/bin/sh\necho stdout\necho stderr 1>&2\nexit 2\n",
        );

        with_git_program_override_for_tests(fake_git, || {
            let err = list_ignored_rel_paths(temp_dir.path()).unwrap_err();
            let message = err.to_string();
            assert!(message.contains("git ls-files failed"));
        });
    }

    #[test]
    fn test_list_ignored_rel_paths_reports_spawn_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let missing_git = PathBuf::from("/definitely/missing/tenex-git");

        with_git_program_override_for_tests(missing_git, || {
            let err = list_ignored_rel_paths(temp_dir.path()).unwrap_err();
            assert!(
                err.to_string()
                    .contains("Failed to list ignored files from")
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_git_check_ignore_ignored_paths_errors_on_broken_pipe_writing_stdin() {
        let temp_dir = TempDir::new().unwrap();
        let fake_git = write_fake_git_script(
            &temp_dir,
            "#!/bin/sh\ndd bs=1 count=1 >/dev/null 2>/dev/null || true\nexit 0\n",
        );

        with_git_program_override_for_tests(fake_git, || {
            let chunk = "a".repeat(2048);
            let rel_paths = (0..2048)
                .map(|idx| PathBuf::from(format!("ignored-{idx}-{chunk}")))
                .collect::<Vec<_>>();
            let err = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap_err();
            assert!(err.to_string().contains("Failed to write path ignored-"));
            assert!(err.to_string().contains("git check-ignore stdin"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_git_check_ignore_ignored_paths_errors_when_status_is_unexpected() {
        let temp_dir = TempDir::new().unwrap();
        let fake_git = write_fake_git_script(
            &temp_dir,
            "#!/bin/sh\ncat >/dev/null\necho out\necho err 1>&2\nexit 2\n",
        );

        with_git_program_override_for_tests(fake_git, || {
            let rel_paths = [PathBuf::from("ignored.txt")];
            let err = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap_err();
            assert!(err.to_string().contains("git check-ignore failed"));
        });
    }

    #[test]
    fn test_git_check_ignore_ignored_paths_reports_spawn_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let missing_git = PathBuf::from("/definitely/missing/tenex-git");

        with_git_program_override_for_tests(missing_git, || {
            let rel_paths = [PathBuf::from("ignored.txt")];
            let err = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap_err();
            assert!(err.to_string().contains("Failed to spawn git check-ignore"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_git_check_ignore_ignored_paths_reports_stdin_take_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let fake_git = write_fake_git_script(&temp_dir, "#!/bin/sh\nexit 0\n");

        with_dropped_git_check_ignore_stdin_for_tests(|| {
            with_git_program_override_for_tests(fake_git, || {
                let rel_paths = [PathBuf::from("ignored.txt")];
                let err = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap_err();
                assert!(err.to_string().contains("Failed to open git stdin"));
            });
        });
    }

    #[test]
    fn test_git_check_ignore_ignored_paths_yields_before_delimiter_when_ready_path_missing() {
        let temp_dir = TempDir::new().unwrap();
        let _repo = Repository::init(temp_dir.path()).expect("Init repository");

        with_yield_before_git_check_ignore_delimiter_write_for_tests(|| {
            let rel_paths = [PathBuf::from("ignored.txt")];
            let ignored = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap();
            assert!(ignored.is_empty());
        });
    }

    #[test]
    fn test_git_check_ignore_ignored_paths_yields_until_timeout_when_ready_path_never_appears() {
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let _repo = Repository::init(temp_dir.path()).expect("Init repository");

        let ready_path = temp_dir.path().join("ready-never-appears");
        assert!(!ready_path.exists());

        with_git_check_ignore_delimiter_ready_path_for_tests(ready_path, || {
            with_git_check_ignore_delimiter_max_wait_for_tests(Duration::from_millis(0), || {
                with_yield_before_git_check_ignore_delimiter_write_for_tests(|| {
                    let rel_paths = [PathBuf::from("ignored.txt")];
                    let ignored =
                        git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap();
                    assert!(ignored.is_empty());
                });
            });
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_git_check_ignore_ignored_paths_reports_delimiter_write_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let ready_path = temp_dir.path().join("ready");
        let fake_git = write_fake_git_script(
            &temp_dir,
            "#!/bin/sh\ndd bs=1 count=1 >/dev/null 2>/dev/null || true\nexec 0<&-\nready=\"$(dirname \"$0\")/ready\"\ntouch \"$ready\"\nsleep 0.1\nexit 0\n",
        );

        with_git_check_ignore_delimiter_ready_path_for_tests(ready_path, || {
            with_yield_before_git_check_ignore_delimiter_write_for_tests(|| {
                with_git_program_override_for_tests(fake_git, || {
                    let rel_paths = [PathBuf::from("a")];
                    let err =
                        git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap_err();
                    assert!(
                        err.to_string()
                            .contains("Failed to write NUL delimiter to git check-ignore stdin")
                    );
                });
            });
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_git_check_ignore_ignored_paths_reports_wait_output_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let fake_git = write_fake_git_script(&temp_dir, "#!/bin/sh\ncat >/dev/null\nexit 0\n");

        with_prewaited_git_check_ignore_child_for_tests(|| {
            with_git_program_override_for_tests(fake_git, || {
                let rel_paths = [PathBuf::from("ignored.txt")];
                let err = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap_err();
                assert!(
                    err.to_string()
                        .contains("Failed to read git check-ignore output")
                );
            });
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_git_check_ignore_ignored_paths_parses_output_without_trailing_nul() {
        let temp_dir = TempDir::new().unwrap();
        let fake_git = write_fake_git_script(
            &temp_dir,
            "#!/bin/sh\ncat >/dev/null\nprintf 'ignored.txt'\nexit 0\n",
        );

        with_git_program_override_for_tests(fake_git, || {
            let rel_paths = [PathBuf::from("ignored.txt")];
            let ignored = git_check_ignore_ignored_paths(temp_dir.path(), &rel_paths).unwrap();
            assert!(ignored.contains(Path::new("ignored.txt")));
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_clone_symlink_reports_read_link_errors() {
        let temp_dir = TempDir::new().unwrap();
        let src = temp_dir.path().join("missing");
        let dst = temp_dir.path().join("dst");

        let err = clone_symlink(&src, &dst).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn test_create_with_options_errors_when_parent_dir_is_not_directory() {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).expect("Init repository");
        let manager = Manager::new(&repo);

        let parent = temp_dir.path().join("worktrees");
        fs::write(&parent, "not-a-dir").expect("Write parent marker");
        let wt_path = parent.join("feature");

        let err = manager
            .create_with_options(&wt_path, "missing", CreateOptions::default())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to create parent directory")
        );
    }

    #[test]
    fn test_create_with_options_skips_parent_creation_when_parent_is_none() {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();
        let manager = Manager::new(&repo);

        let err = manager
            .create_with_options(Path::new("/"), "missing", CreateOptions::default())
            .unwrap_err();
        assert!(err.to_string().contains("Branch not found"));
    }

    #[test]
    fn test_create_with_new_branch_with_options_errors_when_parent_dir_is_not_directory() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let parent = temp_dir.path().join("worktrees");
        fs::write(&parent, "not-a-dir").expect("Write parent marker");
        let wt_path = parent.join("feature");

        let err = manager
            .create_with_new_branch_with_options(&wt_path, "feature-test", CreateOptions::default())
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to create parent directory")
        );
    }

    #[test]
    fn test_create_with_new_branch_skips_parent_creation_when_parent_is_none() {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();
        let manager = Manager::new(&repo);

        let err = manager
            .create_with_new_branch_with_options(
                Path::new("/"),
                "feature",
                CreateOptions::default(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("Failed to get HEAD"));
    }

    #[test]
    fn test_create_with_options_returns_context_when_worktree_add_fails_without_force() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");
        repo.branch("feature-add", &head, false)
            .expect("Create feature branch");

        let wt_path = temp_dir.path().join("worktrees").join("feature-add");
        fs::create_dir_all(&wt_path).expect("Create worktree directory");
        fs::write(wt_path.join("existing.txt"), "payload").expect("Write worktree marker");

        let err = manager.create(&wt_path, "feature-add").unwrap_err();
        assert!(err.to_string().contains("Failed to create worktree at"));
    }

    #[cfg(unix)]
    #[test]
    fn test_create_with_git_force_errors_when_git_fails() {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).expect("Init repository");
        let manager = Manager::new(&repo);

        let fake_git = write_fake_git_script(&temp_dir, "#!/bin/sh\necho nope\nexit 1\n");
        with_git_program_override_for_tests(fake_git, || {
            let err = manager
                .create_with_git_force(&temp_dir.path().join("worktrees").join("force"), "branch")
                .unwrap_err();
            assert!(err.to_string().contains("git worktree add failed"));
        });
    }

    #[test]
    fn test_create_with_git_force_errors_when_repo_has_no_workdir() {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init_bare(temp_dir.path()).expect("Init bare repository");
        let manager = Manager::new(&repo);

        let err = manager
            .create_with_git_force(&temp_dir.path().join("worktrees").join("force"), "branch")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Repository has no working directory")
        );
    }

    #[test]
    fn test_create_with_git_force_reports_spawn_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let repo = Repository::init(temp_dir.path()).expect("Init repository");
        let manager = Manager::new(&repo);

        let missing_git = PathBuf::from("/definitely/missing/tenex-git");
        with_git_program_override_for_tests(missing_git, || {
            let err = manager
                .create_with_git_force(&temp_dir.path().join("worktrees").join("force"), "branch")
                .unwrap_err();
            assert!(
                err.to_string()
                    .contains("Failed to run git worktree add for branch")
            );
        });
    }

    #[test]
    fn test_push_worktree_info_if_openable_skips_missing_and_reports_lock_status() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let mut infos = Vec::new();
        manager.push_worktree_info_if_openable(&mut infos, "does-not-exist");
        assert!(infos.is_empty());

        let unlocked_path = temp_dir.path().join("worktrees").join("feature-unlocked");
        manager
            .create_with_new_branch(&unlocked_path, "feature-unlocked-test")
            .expect("Create worktree");
        manager.push_worktree_info_if_openable(&mut infos, "feature-unlocked-test");

        let wt_path = temp_dir.path().join("worktrees").join("feature-valid");
        manager
            .create_with_new_branch(&wt_path, "feature-valid-test")
            .expect("Create worktree");
        manager
            .lock("feature-valid-test", Some("test"))
            .expect("Lock worktree");
        manager.push_worktree_info_if_openable(&mut infos, "feature-valid-test");

        assert!(
            infos
                .iter()
                .any(|wt| wt.name == "feature-valid-test" && wt.is_locked)
        );
        assert_eq!(
            infos
                .iter()
                .filter(|wt| wt.name == "feature-unlocked-test" && !wt.is_locked)
                .count(),
            1
        );
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_bails_when_admin_path_is_file() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_file = admin_dir_root.join("file-admin");
        fs::write(&admin_file, "not-a-dir").expect("Write worktree admin marker");

        let err = manager
            .cleanup_orphaned_worktree_admin_dir("file-admin")
            .unwrap_err();
        assert!(err.to_string().contains("not a directory"));
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_removes_invalid_dir_missing_gitdir() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("missing-gitdir");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        fs::write(admin_dir.join("junk"), "payload").expect("Write worktree admin marker");

        with_tracing_dispatch(|| manager.cleanup_orphaned_worktree_admin_dir("missing-gitdir"))
            .expect("Prune orphaned admin dir");
        assert!(!admin_dir.exists());
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_removes_orphaned_empty_dir() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("orphan-empty");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        assert!(is_empty_dir(&admin_dir).expect("Check admin dir emptiness"));

        with_tracing_dispatch(|| manager.cleanup_orphaned_worktree_admin_dir("orphan-empty"))
            .expect("Prune orphaned admin dir");
        assert!(!admin_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_propagates_is_empty_dir_errors() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("perm-denied");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");

        let mut perms = fs::metadata(&admin_dir)
            .expect("Read metadata")
            .permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o000);
        fs::set_permissions(&admin_dir, perms).expect("Remove permissions");

        let err = manager
            .cleanup_orphaned_worktree_admin_dir("perm-denied")
            .unwrap_err();
        assert!(err.to_string().contains("Failed to read directory"));

        let mut restore = fs::metadata(&admin_dir)
            .expect("Read metadata")
            .permissions();
        restore.set_mode(original_mode);
        fs::set_permissions(&admin_dir, restore).expect("Restore permissions");
    }

    #[cfg(unix)]
    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_propagates_gitdir_read_errors() {
        use std::os::unix::fs::PermissionsExt;

        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("unreadable-gitdir");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        let gitdir_path = admin_dir.join("gitdir");
        fs::write(&gitdir_path, "real-gitdir").expect("Write gitdir file");

        let mut perms = fs::metadata(&gitdir_path)
            .expect("Read metadata")
            .permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o000);
        fs::set_permissions(&gitdir_path, perms).expect("Remove permissions");

        let err = manager
            .cleanup_orphaned_worktree_admin_dir("unreadable-gitdir")
            .unwrap_err();
        assert!(err.to_string().contains("Failed to read"));

        let mut restore = fs::metadata(&gitdir_path)
            .expect("Read metadata")
            .permissions();
        restore.set_mode(original_mode);
        fs::set_permissions(&gitdir_path, restore).expect("Restore permissions");
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_propagates_remove_dir_errors() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("forced-remove-error");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        fs::write(admin_dir.join("gitdir"), "\n").expect("Write empty gitdir file");

        with_forced_remove_dir_all_with_retries_error_for_tests(|| {
            let err = with_tracing_dispatch(|| {
                manager.cleanup_orphaned_worktree_admin_dir("forced-remove-error")
            })
            .unwrap_err();
            assert!(
                err.to_string()
                    .contains("Forced remove_dir_all_with_retries failure")
            );
        });
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_removes_invalid_dir_empty_gitdir() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("empty-gitdir");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        fs::write(admin_dir.join("gitdir"), "\n").expect("Write empty gitdir file");

        with_tracing_dispatch(|| manager.cleanup_orphaned_worktree_admin_dir("empty-gitdir"))
            .expect("Prune orphaned admin dir");
        assert!(!admin_dir.exists());
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_removes_stale_admin_dir_for_relative_gitdir() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("stale-relative");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        fs::write(admin_dir.join("gitdir"), "../missing").expect("Write stale gitdir file");

        with_tracing_dispatch(|| manager.cleanup_orphaned_worktree_admin_dir("stale-relative"))
            .expect("Prune orphaned admin dir");
        assert!(!admin_dir.exists());
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_removes_stale_admin_dir_for_absolute_gitdir() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).expect("Create worktree admin root");
        let admin_dir = admin_dir_root.join("stale-absolute");
        fs::create_dir_all(&admin_dir).expect("Create worktree admin dir");
        fs::write(admin_dir.join("gitdir"), "/definitely/missing/gitdir")
            .expect("Write stale gitdir file");

        with_tracing_dispatch(|| manager.cleanup_orphaned_worktree_admin_dir("stale-absolute"))
            .expect("Prune orphaned admin dir");
        assert!(!admin_dir.exists());
    }

    #[test]
    fn test_cleanup_orphaned_worktree_admin_dir_keeps_dir_when_gitdir_resolves() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir_root = repo.path().join("worktrees");
        fs::create_dir_all(&admin_dir_root).unwrap();
        let admin_dir = admin_dir_root.join("valid-gitdir");
        fs::create_dir_all(&admin_dir).unwrap();
        fs::create_dir_all(admin_dir.join("real-gitdir")).unwrap();
        fs::write(admin_dir.join("gitdir"), "real-gitdir").unwrap();

        with_tracing_dispatch(|| manager.cleanup_orphaned_worktree_admin_dir("valid-gitdir"))
            .unwrap();
        assert!(admin_dir.exists());
    }

    #[test]
    fn test_list_propagates_worktrees_errors() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        with_forced_repo_worktrees_error_for_tests(|| {
            let err = manager.list().unwrap_err();
            assert!(err.to_string().contains("Failed to list worktrees"));
        });
    }

    #[test]
    fn test_lock_reports_missing_worktrees() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = manager.lock("missing", None).unwrap_err();
        assert!(err.to_string().contains("Worktree not found"));
    }

    #[test]
    fn test_unlock_reports_missing_worktrees() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = manager.unlock("missing").unwrap_err();
        assert!(err.to_string().contains("Worktree not found"));
    }

    #[test]
    fn test_validate_reports_missing_worktrees() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = manager.validate("missing").unwrap_err();
        assert!(err.to_string().contains("Worktree not found"));
    }

    #[test]
    fn test_lock_propagates_lock_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-lock-error");
        manager
            .create_with_new_branch(&wt_path, "feature-lock-error-test")
            .expect("Create worktree");

        manager
            .lock("feature-lock-error-test", Some("first"))
            .expect("Lock worktree");

        let err = manager
            .lock("feature-lock-error-test", Some("second"))
            .unwrap_err();
        assert!(err.to_string().contains("Failed to lock worktree"));
    }

    #[cfg(unix)]
    #[test]
    fn test_unlock_propagates_unlock_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;

        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-unlock-error");
        manager
            .create_with_new_branch(&wt_path, "feature-unlock-error-test")
            .expect("Create worktree");
        manager
            .lock("feature-unlock-error-test", Some("reason"))
            .expect("Lock worktree");

        let admin_dir = repo
            .path()
            .join("worktrees")
            .join("feature-unlock-error-test");
        let mut perms = fs::metadata(&admin_dir)
            .expect("Read metadata")
            .permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o555);
        fs::set_permissions(&admin_dir, perms).expect("Drop write permissions");

        let err = manager.unlock("feature-unlock-error-test").unwrap_err();
        assert!(err.to_string().contains("Failed to unlock worktree"));

        let mut restore = fs::metadata(&admin_dir)
            .expect("Read metadata")
            .permissions();
        restore.set_mode(original_mode);
        fs::set_permissions(&admin_dir, restore).expect("Restore permissions");
    }

    #[test]
    fn test_validate_propagates_validate_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-invalid");
        manager
            .create_with_new_branch(&wt_path, "feature-invalid-test")
            .expect("Create worktree");
        fs::remove_dir_all(&wt_path).expect("Remove worktree directory");

        let err = manager.validate("feature-invalid-test").unwrap_err();
        assert!(
            err.to_string()
                .contains("Worktree 'feature-invalid-test' is invalid")
        );
    }

    #[test]
    fn test_head_info_reports_peel_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let ref_name = head.name().expect("Head ref has name");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");
        let tree_id = commit.tree_id();

        repo.reference(ref_name, tree_id, true, "Point branch at tree")
            .expect("Update HEAD reference");

        let err = manager.head_info().unwrap_err();
        assert!(err.to_string().contains("Failed to get HEAD commit"));
    }

    #[test]
    fn test_worktree_head_info_reports_head_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-no-head");
        manager
            .create_with_new_branch(&wt_path, "feature-no-head-test")
            .expect("Create worktree");

        with_forced_repo_head_error_for_tests(|| {
            let err = manager
                .worktree_head_info("feature-no-head-test")
                .unwrap_err();
            assert!(err.to_string().contains("Failed to get worktree HEAD"));
        });
    }

    #[test]
    fn test_worktree_head_info_reports_peel_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-peel-error");
        manager
            .create_with_new_branch(&wt_path, "feature-peel-error-test")
            .expect("Create worktree");

        let wt_repo = Repository::open(&wt_path).expect("Open worktree repository");
        let head = wt_repo.head().expect("Read worktree HEAD");
        let ref_name = head.name().expect("Worktree HEAD has name");
        let commit = head.peel_to_commit().expect("Peel to commit");
        let tree_id = commit.tree_id();
        wt_repo
            .reference(ref_name, tree_id, true, "Point branch at tree")
            .expect("Update HEAD reference");

        let err = manager
            .worktree_head_info("feature-peel-error-test")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to get worktree HEAD commit")
        );
    }

    #[test]
    fn test_symlink_ignored_files_into_worktree_propagates_check_ignore_errors() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let repo_root = repo.workdir().expect("Repo has workdir");
        let manager = Manager::new(&repo);

        fs::write(repo_root.join(".gitignore"), "ignored.txt\n").expect("Write gitignore");
        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");
        let tree_id = index.write_tree().expect("Write tree");
        let parent = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD");
        let tree = repo.find_tree(tree_id).expect("Lookup tree");
        repo.commit(Some("HEAD"), &sig, &sig, "Add gitignore", &tree, &[&parent])
            .expect("Commit gitignore");

        fs::write(repo_root.join("ignored.txt"), "payload").expect("Write ignored file");

        let err = with_dropped_git_check_ignore_stdin_for_tests(|| {
            manager.symlink_ignored_files_into_worktree(temp_dir.path())
        })
        .unwrap_err();
        assert!(err.to_string().contains("Failed to open git stdin"));
    }

    #[test]
    fn test_head_info_reports_detached_head() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");
        repo.set_head_detached(head.id())
            .expect("Detach repository HEAD");

        let (branch, _commit) = manager.head_info().expect("Read head info");
        assert_eq!(branch, "HEAD (detached)");
    }

    #[test]
    fn test_worktree_head_info_reports_open_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-open-error");
        manager
            .create_with_new_branch(&wt_path, "feature-open-error-test")
            .expect("Create worktree");
        fs::remove_dir_all(&wt_path).expect("Remove worktree directory");

        let err = manager
            .worktree_head_info("feature-open-error-test")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to open worktree repository at")
        );
    }

    #[test]
    fn test_worktree_head_info_reports_detached_head() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-detached");
        manager
            .create_with_new_branch(&wt_path, "feature-detached-test")
            .unwrap();

        let wt_repo = Repository::open(&wt_path).unwrap();
        let head_commit = wt_repo.head().unwrap().peel_to_commit().unwrap();
        wt_repo.set_head_detached(head_commit.id()).unwrap();

        let (branch_name, short_hash) =
            manager.worktree_head_info("feature-detached-test").unwrap();
        assert_eq!(branch_name, "HEAD (detached)");
        assert_eq!(short_hash, head_commit.id().to_string()[..7].to_string());
    }

    #[test]
    fn test_symlink_helpers_noop_for_bare_repo() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("bare.git");
        let repo = Repository::init_bare(&repo_path).expect("Init bare repository");
        let manager = Manager::new(&repo);

        manager
            .symlink_local_instruction_files_into_worktree(temp_dir.path())
            .expect("Symlink instruction files");
        manager
            .symlink_ignored_files_into_worktree(temp_dir.path())
            .expect("Symlink ignored files");
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_ignored_files_into_worktree_links_ignored_file() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let repo_root = repo.workdir().unwrap();
        let manager = Manager::new(&repo);

        fs::write(repo_root.join(".gitignore"), "ignored.txt\n").unwrap();
        let sig = Signature::now("Test", "test@test.com").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(".gitignore")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add gitignore", &tree, &[&parent])
            .unwrap();

        fs::write(repo_root.join("ignored.txt"), "payload").unwrap();

        let wt_path = temp_dir.path().join("worktrees").join("feature-ignored");
        manager
            .create_with_new_branch_with_options(
                &wt_path,
                "feature-ignored-test",
                CreateOptions::without_ignored_file_links(),
            )
            .unwrap();

        assert!(!wt_path.join("ignored.txt").exists());

        manager
            .symlink_ignored_files_into_worktree(&wt_path)
            .unwrap();

        let dst = wt_path.join("ignored.txt");
        let dst_meta = fs::symlink_metadata(&dst).unwrap();
        assert!(dst_meta.file_type().is_symlink());
        assert_eq!(fs::read_to_string(dst).unwrap(), "payload");
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_ignored_files_into_worktree_propagates_symlink_errors() {
        use std::os::unix::fs::PermissionsExt;

        let (temp_dir, repo) = init_test_repo_with_commit();
        let repo_root = repo.workdir().unwrap();
        let manager = Manager::new(&repo);

        fs::write(repo_root.join(".gitignore"), "ignored.txt\n").expect("Write gitignore");
        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");
        let tree_id = index.write_tree().expect("Write tree");
        let parent = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");
        let tree = repo.find_tree(tree_id).expect("Find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "Add gitignore", &tree, &[&parent])
            .expect("Commit gitignore");

        fs::write(repo_root.join("ignored.txt"), "payload").expect("Write ignored file");

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-ignored-symlink-error");
        manager
            .create_with_new_branch_with_options(
                &wt_path,
                "feature-ignored-symlink-error-test",
                CreateOptions::without_ignored_file_links(),
            )
            .unwrap();

        let mut perms = fs::metadata(&wt_path)
            .expect("Read worktree metadata")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&wt_path, perms).expect("Set worktree permissions");

        let err = manager
            .symlink_ignored_files_into_worktree(&wt_path)
            .unwrap_err();
        assert!(err.to_string().contains("Failed to symlink ignored path"));
    }

    #[test]
    fn test_symlink_ignored_path_skips_when_not_ignored_or_source_missing() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        let worktree_root = temp_dir.path().join("worktree");
        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(&worktree_root).unwrap();

        let rel_path = Path::new("ignored.txt");

        let ignored = std::collections::HashSet::new();
        Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored).unwrap();
        assert!(fs::symlink_metadata(worktree_root.join(rel_path)).is_err());

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored).unwrap();
        assert!(fs::symlink_metadata(worktree_root.join(rel_path)).is_err());
    }

    #[test]
    fn test_symlink_ignored_path_returns_ok_when_destination_parent_is_none() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let rel_path = Path::new("");
        let ignored = std::collections::HashSet::from([PathBuf::from("")]);
        Manager::symlink_ignored_path(&repo_root, Path::new(""), rel_path, &ignored).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_ignored_path_skips_when_source_is_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        let worktree_root = temp_dir.path().join("worktree");
        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(&worktree_root).unwrap();

        let rel_path = Path::new("ignored.txt");
        let src = repo_root.join(rel_path);
        symlink_path(Path::new("missing-target"), &src).unwrap();

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored).unwrap();
        assert!(fs::symlink_metadata(worktree_root.join(rel_path)).is_err());
    }

    #[test]
    fn test_symlink_ignored_path_skips_when_worktree_is_nested_under_source() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        fs::create_dir_all(&repo_root).unwrap();

        let rel_path = Path::new("ignored.txt");
        let src = repo_root.join(rel_path);
        fs::write(&src, "payload").unwrap();

        let worktree_root = src.join("nested-worktree");

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored).unwrap();
        assert!(fs::symlink_metadata(worktree_root.join(rel_path)).is_err());
    }

    #[test]
    fn test_symlink_ignored_path_skips_when_destination_exists() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        let worktree_root = temp_dir.path().join("worktree");
        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(&worktree_root).unwrap();

        let rel_path = Path::new("ignored.txt");
        fs::write(repo_root.join(rel_path), "payload").unwrap();
        fs::write(worktree_root.join(rel_path), "existing").unwrap();

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_ignored_path_skips_when_parent_is_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        let worktree_root = temp_dir.path().join("worktree");
        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(&worktree_root).unwrap();

        let rel_path = Path::new("symlink-parent/ignored.txt");
        let src = repo_root.join(rel_path);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, "payload").unwrap();

        let link_target = temp_dir.path().join("link-target");
        fs::create_dir_all(&link_target).unwrap();
        symlink_path(&link_target, &worktree_root.join("symlink-parent")).unwrap();

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored).unwrap();
        assert!(fs::symlink_metadata(worktree_root.join(rel_path)).is_err());
    }

    #[test]
    fn test_symlink_ignored_path_reports_parent_creation_errors_with_context() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        let worktree_root = temp_dir.path().join("worktree");
        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(&worktree_root).unwrap();

        let rel_path = Path::new("file-parent/ignored.txt");
        let src = repo_root.join(rel_path);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, "payload").unwrap();

        fs::write(worktree_root.join("file-parent"), "not-a-dir").unwrap();

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        let err = Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored)
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to create parent directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_ignored_path_reports_symlink_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo");
        let worktree_root = temp_dir.path().join("worktree");
        fs::create_dir_all(&repo_root).unwrap();
        fs::create_dir_all(&worktree_root).unwrap();

        let rel_path = Path::new("readonly-parent/ignored.txt");
        let src = repo_root.join(rel_path);
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, "payload").unwrap();

        let readonly_parent = worktree_root.join("readonly-parent");
        fs::create_dir_all(&readonly_parent).unwrap();
        let mut perms = fs::metadata(&readonly_parent).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&readonly_parent, perms).unwrap();

        let ignored = std::collections::HashSet::from([rel_path.to_path_buf()]);
        let err = Manager::symlink_ignored_path(&repo_root, &worktree_root, rel_path, &ignored)
            .unwrap_err();
        assert!(err.to_string().contains("Failed to symlink ignored path"));
    }

    #[cfg(unix)]
    #[test]
    fn test_finish_worktree_create_logs_warnings_when_symlink_helpers_error() {
        use std::os::unix::fs::PermissionsExt;

        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let repo_root = repo.workdir().unwrap();
        fs::write(repo_root.join("AGENTS.md"), "# agents\n").expect("Write AGENTS.md");

        let worktree_path = temp_dir.path().join("worktrees").join("warn-worktree");
        fs::create_dir_all(&worktree_path).expect("Create worktree directory");
        let mut perms = fs::metadata(&worktree_path)
            .expect("Read worktree metadata")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&worktree_path, perms).expect("Set worktree permissions");

        let fake_git = write_fake_git_script(&temp_dir, "#!/bin/sh\necho nope\nexit 1\n");
        with_git_program_override_for_tests(fake_git, || {
            with_tracing_dispatch(|| {
                manager.finish_worktree_create(&worktree_path, CreateOptions::default());
            });
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_local_instruction_file_skips_directory_sources() {
        let (temp_dir, repo) = init_test_repo_with_commit();

        let repo_root = repo.workdir().unwrap();
        let src = repo_root.join("AGENTS.md");
        fs::create_dir_all(&src).expect("Create AGENTS.md directory");

        let worktree_path = temp_dir.path().join("worktrees").join("instruction-skip");
        fs::create_dir_all(&worktree_path).expect("Create worktree directory");

        Manager::symlink_local_instruction_file(repo_root, &worktree_path, "AGENTS.md")
            .expect("Symlink local instruction file");
        assert!(fs::symlink_metadata(worktree_path.join("AGENTS.md")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_local_instruction_file_reports_clone_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;

        let (temp_dir, repo) = init_test_repo_with_commit();
        let repo_root = repo.workdir().unwrap();

        let src = repo_root.join("AGENTS.md");
        std::os::unix::fs::symlink("missing-target", &src).expect("Create broken symlink");

        let worktree_path = temp_dir.path().join("worktrees").join("instruction-clone");
        fs::create_dir_all(&worktree_path).expect("Create worktree directory");
        let mut perms = fs::metadata(&worktree_path)
            .expect("Read worktree metadata")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&worktree_path, perms).expect("Set worktree permissions");

        let err = Manager::symlink_local_instruction_file(repo_root, &worktree_path, "AGENTS.md")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to symlink local instruction file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_local_instruction_file_reports_symlink_errors_with_context() {
        use std::os::unix::fs::PermissionsExt;

        let (temp_dir, repo) = init_test_repo_with_commit();
        let repo_root = repo.workdir().unwrap();

        fs::write(repo_root.join("AGENTS.md"), "payload").expect("Write AGENTS.md");

        let worktree_path = temp_dir.path().join("worktrees").join("instruction-file");
        fs::create_dir_all(&worktree_path).expect("Create worktree directory");
        let mut perms = fs::metadata(&worktree_path)
            .expect("Read worktree metadata")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&worktree_path, perms).expect("Set worktree permissions");

        let err = Manager::symlink_local_instruction_file(repo_root, &worktree_path, "AGENTS.md")
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to symlink local instruction file")
        );
    }

    #[test]
    fn test_create_with_new_branch_removes_existing_worktree_dir_when_prune_leaves_it_behind() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-locked-recreate");
        manager
            .create_with_new_branch(&wt_path, "feature-locked-recreate-test")
            .expect("Create worktree");
        fs::write(wt_path.join("marker.txt"), "payload").expect("Write marker file");

        manager
            .lock("feature-locked-recreate-test", Some("test"))
            .expect("Lock worktree");
        manager
            .create_with_new_branch(&wt_path, "feature-locked-recreate-test")
            .expect("Recreate worktree");

        assert!(wt_path.exists());
        assert!(!wt_path.join("marker.txt").exists());
    }

    #[test]
    fn test_remove_worktree_only_propagates_remove_dir_failures() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-remove-dir-fails");
        manager
            .create_with_new_branch(&wt_path, "feature-remove-dir-fails-test")
            .expect("Create worktree");
        fs::remove_dir_all(&wt_path).expect("Remove worktree directory");
        fs::write(&wt_path, "not-a-dir").expect("Write file at worktree path");

        let err =
            with_tracing_dispatch(|| manager.remove_worktree_only("feature-remove-dir-fails-test"))
                .unwrap_err();
        assert!(err.to_string().contains("Failed to remove directory at"));
    }

    #[test]
    fn test_remove_worktree_only_returns_error_when_prune_fails_and_worktree_still_exists_in_git() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-worktree-only-prune-fails");
        manager
            .create_with_new_branch(&wt_path, "feature-worktree-only-prune-fails-test")
            .unwrap();
        manager
            .lock("feature-worktree-only-prune-fails-test", Some("test"))
            .unwrap();

        let err = with_tracing_dispatch(|| {
            manager.remove_worktree_only("feature-worktree-only-prune-fails-test")
        })
        .unwrap_err();
        assert!(err.to_string().contains("may still be in use"));
    }

    #[test]
    fn test_remove_returns_error_when_prune_fails_and_worktree_still_exists_in_git() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-remove-prune-fails");
        manager
            .create_with_new_branch(&wt_path, "feature-remove-prune-fails-test")
            .unwrap();
        manager
            .lock("feature-remove-prune-fails-test", Some("test"))
            .unwrap();

        let err = with_tracing_dispatch(|| manager.remove("feature-remove-prune-fails-test"))
            .unwrap_err();
        assert!(err.to_string().contains("may still be in use"));
    }

    #[test]
    fn test_remove_succeeds_when_prune_fails_and_worktree_is_no_longer_registered() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let branch = "feature-remove-prune-admin-removed-test";
        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-remove-prune-admin-removed");
        manager
            .create_with_new_branch(&wt_path, branch)
            .expect("Create worktree");
        manager.lock(branch, Some("test")).expect("Lock worktree");

        let worktree_name = branch.replace('/', "-");
        let admin_dir = repo.path().join("worktrees").join(worktree_name);
        assert!(admin_dir.exists());

        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(25));
            let _ = fs::remove_dir_all(&admin_dir);
        });

        with_tracing_dispatch(|| manager.remove(branch)).expect("Remove worktree");
        let _ = handle.join();
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_create_with_new_branch() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-test");
        manager
            .create_with_new_branch(&wt_path, "feature-test")
            .expect("Create worktree");

        assert!(wt_path.exists());
        assert!(manager.exists("feature-test"));
    }

    #[test]
    fn test_create_with_new_branch_removes_orphaned_admin_dir() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let admin_dir = repo.path().join("worktrees").join("agent-asdf");
        fs::create_dir_all(&admin_dir).expect("Create orphaned admin dir");
        assert!(is_empty_dir(&admin_dir).expect("Check orphaned admin dir"));

        let wt_path = temp_dir.path().join("worktrees").join("asdf");
        manager
            .create_with_new_branch(&wt_path, "agent/asdf")
            .expect("Create worktree");

        assert!(wt_path.exists());
        assert!(manager.exists("agent/asdf"));
        assert!(admin_dir.join("gitdir").exists());
    }

    #[test]
    fn test_create_with_new_branch_symlinks_ignored_files() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(
            temp_dir.path().join(".gitignore"),
            "ignored.txt\nignored_dir/\n",
        )
        .unwrap();

        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let parent_commit = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");

        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");

        let tree_id = index.write_tree().expect("Write tree");
        let tree = repo.find_tree(tree_id).expect("Find tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add gitignore",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

        let ignored_file = temp_dir.path().join("ignored.txt");
        fs::write(&ignored_file, "payload").expect("Write ignored file");

        let ignored_dir = temp_dir.path().join("ignored_dir");
        fs::create_dir_all(&ignored_dir).expect("Create ignored directory");
        fs::write(ignored_dir.join("nested.txt"), "nested").expect("Write nested marker");

        let wt_path = temp_dir.path().join("worktrees").join("feature-symlinks");
        manager
            .create_with_new_branch(&wt_path, "feature-symlinks-test")
            .expect("Create worktree");

        let linked_file = wt_path.join("ignored.txt");
        let linked_meta = fs::symlink_metadata(&linked_file).expect("Read linked file metadata");
        assert!(linked_meta.file_type().is_symlink());
        let linked_path = fs::canonicalize(&linked_file).expect("Resolve linked file path");
        let ignored_path = fs::canonicalize(&ignored_file).expect("Resolve ignored file path");
        assert_eq!(linked_path, ignored_path);

        assert!(fs::symlink_metadata(wt_path.join("ignored_dir")).is_err());
    }

    #[test]
    fn test_create_with_new_branch_can_skip_ignored_file_symlinks() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(temp_dir.path().join(".gitignore"), "ignored.txt\n").expect("Write gitignore");

        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let parent_commit = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");

        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");

        let tree_id = index.write_tree().expect("Write tree");
        let tree = repo.find_tree(tree_id).expect("Find tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add gitignore",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

        fs::write(temp_dir.path().join("ignored.txt"), "payload").expect("Write ignored file");
        let agents_path = temp_dir.path().join("AGENTS.md");
        fs::write(&agents_path, "# local instructions\n").expect("Write AGENTS.md");
        symlink_path(Path::new("AGENTS.md"), &temp_dir.path().join("CLAUDE.md"))
            .expect("Create CLAUDE.md symlink");

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-no-symlinks");
        manager
            .create_with_new_branch_with_options(
                &wt_path,
                "feature-no-symlinks-test",
                CreateOptions::without_ignored_file_links(),
            )
            .unwrap();

        assert!(fs::symlink_metadata(wt_path.join("ignored.txt")).is_err());
        let linked_agents = wt_path.join("AGENTS.md");
        let linked_agents_meta =
            fs::symlink_metadata(&linked_agents).expect("Read linked AGENTS.md metadata");
        assert!(linked_agents_meta.file_type().is_symlink());
        let linked_agents_path =
            fs::canonicalize(&linked_agents).expect("Resolve linked AGENTS.md path");
        let agents_path = fs::canonicalize(&agents_path).expect("Resolve AGENTS.md path");
        assert_eq!(linked_agents_path, agents_path);

        let linked_claude = wt_path.join("CLAUDE.md");
        let linked_claude_meta =
            fs::symlink_metadata(&linked_claude).expect("Read linked CLAUDE.md metadata");
        assert!(linked_claude_meta.file_type().is_symlink());
        assert_eq!(
            fs::read_link(&linked_claude).expect("Read CLAUDE.md symlink"),
            PathBuf::from("AGENTS.md")
        );
        let linked_claude_path =
            fs::canonicalize(&linked_claude).expect("Resolve linked CLAUDE.md path");
        let agents_path =
            fs::canonicalize(wt_path.join("AGENTS.md")).expect("Resolve linked AGENTS.md path");
        assert_eq!(linked_claude_path, agents_path);
    }

    #[test]
    fn test_symlink_local_instruction_file_skips_existing_dst() {
        let temp_dir = TempDir::new().unwrap();
        let repo_root = temp_dir.path().join("repo-root");
        let worktree_root = temp_dir.path().join("worktree-root");
        fs::create_dir_all(&repo_root).expect("Create repo root");
        fs::create_dir_all(&worktree_root).expect("Create worktree root");

        let src = repo_root.join("AGENTS.md");
        fs::write(&src, "src").expect("Write source file");

        let dst = worktree_root.join("AGENTS.md");
        fs::write(&dst, "dst").expect("Write destination file");

        Manager::symlink_local_instruction_file(&repo_root, &worktree_root, "AGENTS.md")
            .expect("Attempt symlink local instruction file");

        assert_eq!(
            fs::read_to_string(&dst).expect("Read destination file"),
            "dst"
        );
    }

    #[test]
    fn test_manager_debug_impl() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        assert_eq!(format!("{manager:?}"), "Manager { .. }");
    }

    #[test]
    fn test_create_with_new_branch_skips_dir_only_ignored_directory_symlinks() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(temp_dir.path().join(".gitignore"), ".ruff_cache/\n").expect("Write gitignore");

        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let parent_commit = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");

        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");

        let tree_id = index.write_tree().expect("Write tree");
        let tree = repo.find_tree(tree_id).expect("Find tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add gitignore",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

        let cache_dir = temp_dir.path().join(".ruff_cache");
        fs::create_dir_all(&cache_dir).expect("Create cache directory");
        fs::write(cache_dir.join("marker.txt"), "payload").expect("Write cache marker");

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-ignored-dir");
        manager
            .create_with_new_branch(&wt_path, "feature-ignored-dir-test")
            .expect("Create worktree");

        assert!(fs::symlink_metadata(wt_path.join(".ruff_cache")).is_err());
    }

    #[test]
    fn test_create_with_new_branch_symlinks_ignored_dir_when_ignored_as_file() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(temp_dir.path().join(".gitignore"), "ignored_dir\n").expect("Write gitignore");

        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let parent_commit = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");

        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");

        let tree_id = index.write_tree().expect("Write tree");
        let tree = repo.find_tree(tree_id).expect("Find tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add gitignore",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

        let ignored_dir = temp_dir.path().join("ignored_dir");
        fs::create_dir_all(&ignored_dir).expect("Create ignored directory");
        fs::write(ignored_dir.join("nested.txt"), "nested").expect("Write nested marker");

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-ignored-dir-no-slash");
        manager
            .create_with_new_branch(&wt_path, "feature-ignored-dir-no-slash-test")
            .expect("Create worktree");

        let linked_dir = wt_path.join("ignored_dir");
        let linked_dir_meta = fs::symlink_metadata(&linked_dir).expect("Read linked dir metadata");
        assert!(linked_dir_meta.file_type().is_symlink());
        let linked_dir_path = fs::canonicalize(&linked_dir).expect("Resolve linked dir path");
        let ignored_dir_path = fs::canonicalize(&ignored_dir).expect("Resolve ignored dir path");
        assert_eq!(linked_dir_path, ignored_dir_path);

        assert_eq!(
            fs::read_to_string(linked_dir.join("nested.txt")).expect("Read nested marker"),
            "nested"
        );

        let status = super::super::git_command()
            .args(["status", "--porcelain"])
            .current_dir(&wt_path)
            .output()
            .expect("Run git status");
        assert!(status.status.success());

        let stdout = String::from_utf8_lossy(&status.stdout);
        assert!(!stdout.contains("ignored_dir"));
    }

    #[test]
    fn test_create_with_new_branch_skips_ignored_symlink_sources() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(temp_dir.path().join(".gitignore"), "ignored-link\n").expect("Write gitignore");

        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let parent_commit = repo
            .head()
            .expect("Read repository HEAD")
            .peel_to_commit()
            .expect("Peel HEAD to commit");

        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(Path::new(".gitignore"))
            .expect("Add gitignore to index");
        index.write().expect("Write index");

        let tree_id = index.write_tree().expect("Write tree");
        let tree = repo.find_tree(tree_id).expect("Find tree");
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add gitignore",
            &tree,
            &[&parent_commit],
        )
        .unwrap();

        let real = temp_dir.path().join("real.txt");
        fs::write(&real, "payload").expect("Write real file");

        let link = temp_dir.path().join("ignored-link");
        symlink_path(&real, &link).expect("Create ignored symlink");

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-symlink-source");
        manager
            .create_with_new_branch(&wt_path, "feature-symlink-source-test")
            .expect("Create worktree");

        assert!(fs::symlink_metadata(wt_path.join("ignored-link")).is_err());
    }

    #[test]
    fn test_create_with_new_branch_skips_ignored_worktree_ancestor() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(temp_dir.path().join(".gitignore"), "worktrees/\n").expect("Write gitignore");

        let wt_path = temp_dir.path().join("worktrees").join("feature-loop");
        manager
            .create_with_new_branch(&wt_path, "feature-loop-test")
            .expect("Create worktree");

        assert!(fs::symlink_metadata(wt_path.join("worktrees")).is_err());
    }

    #[test]
    fn test_create_existing_branch() {
        let (temp_dir, repo) = init_test_repo_with_commit();

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");
        repo.branch("existing-branch", &commit, false)
            .expect("Create branch");

        let manager = Manager::new(&repo);
        let wt_path = temp_dir.path().join("worktrees").join("existing");
        manager
            .create(&wt_path, "existing-branch")
            .expect("Create worktree");

        assert!(wt_path.exists());
    }

    #[test]
    fn test_create_checked_out_branch_uses_git_force() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let branch = head
            .shorthand()
            .expect("Expected HEAD to be a branch")
            .to_string();

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join(branch.replace('/', "-"));
        manager.create(&wt_path, &branch).expect("Create worktree");

        assert!(wt_path.exists());
        assert!(manager.exists(&branch));
    }

    #[test]
    fn test_create_checked_out_branch_reports_git_force_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let branch = head
            .shorthand()
            .expect("Expected HEAD to be a branch")
            .to_string();

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join(branch.replace('/', "-"));

        let missing_git = PathBuf::from("/definitely-not-a-git-binary");
        with_git_program_override_for_tests(missing_git, || {
            let err = manager
                .create(&wt_path, &branch)
                .expect_err("Create worktree should fail when git cannot be spawned");
            let message = err.to_string();
            assert!(message.contains("Failed to create worktree at"));
            assert!(message.contains(&wt_path.display().to_string()));
        });
    }

    #[test]
    fn test_create_nonexistent_branch() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("test");
        let result = manager.create(&wt_path, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_worktree() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-remove");
        manager
            .create_with_new_branch(&wt_path, "feature-remove-test")
            .expect("Create worktree");
        assert!(manager.exists("feature-remove-test"));

        manager
            .remove("feature-remove-test")
            .expect("Remove worktree");
        assert!(!manager.exists("feature-remove-test"));
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_remove_propagates_remove_dir_failures() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-remove-dir-fails");
        manager
            .create_with_new_branch(&wt_path, "feature-remove-dir-fails-test")
            .unwrap();
        fs::remove_dir_all(&wt_path).unwrap();
        fs::write(&wt_path, "not-a-dir").unwrap();

        let err =
            with_tracing_dispatch(|| manager.remove("feature-remove-dir-fails-test")).unwrap_err();
        assert!(err.to_string().contains("Failed to remove directory at"));
    }

    #[test]
    fn test_remove_nonexistent() {
        // Removing a non-existent worktree/branch should succeed (idempotent cleanup)
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.remove("nonexistent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_worktree_only_succeeds_when_prune_fails_but_worktree_missing() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-prune-fail");
        manager
            .create_with_new_branch(&wt_path, "feature-prune-fail-test")
            .expect("Create worktree");

        with_forced_worktree_prune_error_for_tests(|| {
            with_forced_repo_find_worktree_verify_missing_for_tests(|| {
                manager
                    .remove_worktree_only("feature-prune-fail-test")
                    .expect("Remove worktree");
            });
        });

        assert!(!wt_path.exists());
    }

    #[test]
    fn test_remove_with_missing_worktree_but_existing_branch() {
        // When worktree is manually removed but branch exists, cleanup should still delete branch
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("orphan-branch");
        manager
            .create_with_new_branch(&wt_path, "orphan-branch-test")
            .expect("Create worktree");

        // Verify branch exists
        assert!(
            repo.find_branch("orphan-branch-test", git2::BranchType::Local)
                .is_ok()
        );

        // Manually remove the worktree directory (simulating manual cleanup)
        fs::remove_dir_all(&wt_path).expect("Remove worktree directory");

        // Prune the worktree reference so git doesn't track it
        let worktree_name = "orphan-branch-test";
        let wt = repo.find_worktree(worktree_name).expect("Find worktree");
        let _ = wt.prune(Some(
            git2::WorktreePruneOptions::new()
                .valid(true)
                .working_tree(true),
        ));

        // Now remove should still clean up the branch
        manager
            .remove("orphan-branch-test")
            .expect("Remove worktree");

        // Branch should be deleted
        assert!(
            repo.find_branch("orphan-branch-test", git2::BranchType::Local)
                .is_err()
        );
    }

    #[test]
    fn test_remove_dir_all_with_retries_noop_when_missing() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("missing-dir");

        remove_dir_all_with_retries(&missing).expect("Remove missing directory");

        assert!(!missing.exists());
    }

    #[test]
    fn test_remove_dir_all_with_retries_removes_existing_dir() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("to-remove");
        let nested = target.join("nested");
        fs::create_dir_all(&nested).expect("Create nested directory");
        fs::write(nested.join("file.txt"), "payload").expect("Write payload file");

        remove_dir_all_with_retries(&target).expect("Remove directory");

        assert!(!target.exists());
    }

    #[test]
    fn test_list_worktrees() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-list");
        manager
            .create_with_new_branch(&wt_path, "feature-list-test")
            .expect("Create worktree");

        let worktrees = manager.list().expect("List worktrees");
        assert!(worktrees.iter().any(|wt| wt.name == "feature-list-test"));
    }

    #[test]
    fn test_exists() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        assert!(!manager.exists("nonexistent"));

        let wt_path = temp_dir.path().join("worktrees").join("feature-exists");
        manager
            .create_with_new_branch(&wt_path, "feature-exists-test")
            .expect("Create worktree");

        assert!(manager.exists("feature-exists-test"));
    }

    #[test]
    fn test_lock_and_unlock() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-lock");
        manager
            .create_with_new_branch(&wt_path, "feature-lock-test")
            .expect("Create worktree");

        manager
            .lock("feature-lock-test", Some("Testing lock"))
            .expect("Lock worktree");

        let worktrees = manager.list().expect("List worktrees");
        let locked_wt = worktrees
            .iter()
            .find(|wt| wt.name == "feature-lock-test")
            .expect("Expected worktree");
        assert!(locked_wt.is_locked);

        manager
            .unlock("feature-lock-test")
            .expect("Unlock worktree");

        let worktrees = manager.list().expect("List worktrees");
        let unlocked_wt = worktrees
            .iter()
            .find(|wt| wt.name == "feature-lock-test")
            .expect("Expected worktree");
        assert!(!unlocked_wt.is_locked);
    }

    #[test]
    fn test_unlock_not_locked() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-unlock");
        manager
            .create_with_new_branch(&wt_path, "feature-unlock-test")
            .expect("Create worktree");

        let result = manager.unlock("feature-unlock-test");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-validate");
        manager
            .create_with_new_branch(&wt_path, "feature-validate-test")
            .expect("Create worktree");

        manager
            .validate("feature-validate-test")
            .expect("Validate worktree");
    }

    #[test]
    fn test_branch_name_with_slashes() {
        // Integration test: branch names with slashes (like "tenex/feature-name")
        // should work correctly. The worktree name internally replaces slashes with dashes.
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        // Use a branch name with a slash (like tenex generates)
        let branch_name = "tenex/my-feature";
        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("tenex")
            .join("my-feature");

        // Create worktree with slashed branch name
        manager
            .create_with_new_branch(&wt_path, branch_name)
            .expect("Create worktree");

        // Verify worktree directory exists
        assert!(wt_path.exists());

        // Verify worktree can be found using original branch name
        assert!(manager.exists(branch_name));

        // Verify the worktree is a valid git worktree (has .git file)
        assert!(wt_path.join(".git").exists());

        // Verify the branch was created in the repository
        assert!(
            repo.find_branch(branch_name, git2::BranchType::Local)
                .is_ok()
        );

        // Verify we can validate the worktree using the branch name
        manager.validate(branch_name).expect("Validate worktree");

        // Verify we can remove the worktree using the branch name
        manager.remove(branch_name).expect("Remove worktree");
        assert!(!manager.exists(branch_name));
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_worktree_info() {
        let info = Info {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/test"),
            is_locked: false,
        };

        assert_eq!(info.name, "test");
        assert_eq!(info.path, PathBuf::from("/tmp/test"));
        assert!(!info.is_locked);
    }

    #[test]
    fn test_create_with_new_branch_cleanup_existing() {
        // Test that create_with_new_branch cleans up existing worktree/branch
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-recreate");

        // Create initial worktree
        manager
            .create_with_new_branch(&wt_path, "feature-recreate-test")
            .expect("Create worktree");
        assert!(wt_path.exists());
        assert!(manager.exists("feature-recreate-test"));

        // Create a file in the worktree to verify it's a new worktree after recreation
        let marker_file = wt_path.join("marker.txt");
        fs::write(&marker_file, "original").expect("Write marker file");
        assert!(marker_file.exists());

        // Remove the worktree first (simulate user cleanup without branch deletion)
        manager
            .remove("feature-recreate-test")
            .expect("Remove worktree");

        // Re-create with the same name - should clean up existing branch and succeed
        manager
            .create_with_new_branch(&wt_path, "feature-recreate-test")
            .expect("Create worktree");

        assert!(wt_path.exists());
        assert!(manager.exists("feature-recreate-test"));
        // The marker file should not exist (fresh worktree)
        assert!(!marker_file.exists());
    }

    #[test]
    fn test_create_with_new_branch_errors_when_head_is_not_commit() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let blob = repo.blob(b"not a commit").expect("Create blob");
        repo.reference("refs/heads/master", blob, true, "test: point HEAD at blob")
            .expect("Update master ref");

        let wt_path = temp_dir
            .path()
            .join("worktrees")
            .join("feature-head-not-commit");
        let err = manager
            .create_with_new_branch(&wt_path, "feature-head-not-commit-test")
            .expect_err("Create worktree should fail when HEAD does not peel to commit");
        assert!(err.to_string().contains("Failed to get HEAD commit"));
    }

    #[test]
    fn test_create_with_new_branch_reports_invalid_worktree_admin_path() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let branch = "feature-admin-file-test";
        let admin_dir = repo.path().join("worktrees").join(branch);
        fs::create_dir_all(admin_dir.parent().expect("admin dir should have parent"))
            .expect("Create admin worktrees parent dir");
        fs::write(&admin_dir, "not a directory").expect("Create invalid admin dir file");

        let wt_path = temp_dir.path().join("worktrees").join(branch);
        let err = manager
            .create_with_new_branch(&wt_path, branch)
            .expect_err("Create worktree should fail when admin path is invalid");
        assert!(
            err.to_string()
                .contains("Worktree admin path exists but is not a directory")
        );
    }

    #[test]
    fn test_create_with_new_branch_reports_branch_create_errors_with_context() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("invalid-branch");
        let err = manager
            .create_with_new_branch(&wt_path, "invalid..branch")
            .expect_err("Create worktree should fail for invalid branch name");
        assert!(
            err.to_string()
                .contains("Failed to create branch 'invalid..branch'")
        );
    }

    #[test]
    fn test_create_with_new_branch_recreate_with_existing_worktree() {
        // Test that create_with_new_branch cleans up when worktree still exists
        // (exercises the cleanup path at lines 85-94)
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let wt_path = temp_dir.path().join("worktrees").join("feature-reuse");

        // Create initial worktree
        manager
            .create_with_new_branch(&wt_path, "feature-reuse-test")
            .expect("Create worktree");
        assert!(wt_path.exists());
        assert!(manager.exists("feature-reuse-test"));

        // Create a marker file
        let marker_file = wt_path.join("marker.txt");
        fs::write(&marker_file, "original").expect("Write marker file");

        // DO NOT remove the worktree - call create_with_new_branch again
        // This should prune the existing worktree and recreate it
        with_tracing_dispatch(|| {
            manager
                .create_with_new_branch(&wt_path, "feature-reuse-test")
                .expect("Recreate worktree");
        });

        assert!(wt_path.exists());
        assert!(manager.exists("feature-reuse-test"));
        // The marker file should not exist (fresh worktree)
        assert!(!marker_file.exists());
    }

    #[test]
    fn test_head_info() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let (branch, commit) = manager.head_info().expect("Read head info");

        assert_eq!(branch, "master");
        // Commit hash should be 7 characters
        assert_eq!(commit.len(), 7);
    }

    #[test]
    fn test_worktree_head_info() {
        let (temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        // Create a worktree first
        let wt_path = temp_dir.path().join("worktrees").join("feature-info");
        manager
            .create_with_new_branch(&wt_path, "feature-info-test")
            .expect("Create worktree");

        let (branch, commit) = manager
            .worktree_head_info("feature-info-test")
            .expect("Read worktree head info");

        // Should be on the feature branch
        assert_eq!(branch, "feature-info-test");
        // Commit hash should be 7 characters
        assert_eq!(commit.len(), 7);
    }

    #[test]
    fn test_worktree_head_info_not_found() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.worktree_head_info("nonexistent-worktree");
        assert!(result.is_err());
    }
}
