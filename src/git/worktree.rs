//! Git worktree management

use anyhow::{Context, Result, bail};
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tracing::{debug, info, warn};

const LOCAL_INSTRUCTION_FILE_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

fn remove_dir_all_with_retries(path: &Path) -> Result<()> {
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
        let mut stdin = child.stdin.take().context("Failed to open git stdin")?;
        for rel_path in rel_paths {
            let rel_path_bytes = git_path_bytes(rel_path);
            stdin.write_all(&rel_path_bytes).with_context(|| {
                format!(
                    "Failed to write path {} to git check-ignore stdin",
                    rel_path.display()
                )
            })?;

            stdin
                .write_all(b"\0")
                .context("Failed to write NUL delimiter to git check-ignore stdin")?;
        }
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
                match worktree.prune(Some(&mut opts)) {
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
                match worktree.prune(Some(&mut opts)) {
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
            if !prune_succeeded && self.repo.find_worktree(&worktree_name).is_ok() {
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
        let worktrees = self.repo.worktrees().context("Failed to list worktrees")?;

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

        let head = wt_repo.head().context("Failed to get worktree HEAD")?;
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

        let parent_is_symlink =
            fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink());
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
