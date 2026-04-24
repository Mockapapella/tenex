//! Git branch management

use anyhow::{Context, Result, bail};
use git2::{BranchType, Repository};
#[cfg(any(test, coverage))]
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(test)]
type ListForSelectorOverrideFn = Box<dyn Fn(&Repository) -> Result<Vec<BranchInfo>>>;

#[cfg(test)]
thread_local! {
    static LIST_FOR_SELECTOR_OVERRIDE: RefCell<Option<ListForSelectorOverrideFn>> = const {
        RefCell::new(None)
    };
}

#[cfg(any(test, coverage))]
thread_local! {
    static FORCE_REVWALK_NEW_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_REVWALK_PUSH_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_LIST_BRANCH_RESULT_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_SELECTOR_LOCAL_BRANCH_RESULT_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_SELECTOR_REMOTE_BRANCH_RESULT_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_REMOTE_BRANCHES_LIST_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_CHECKOUT_TREE_ERROR: Cell<bool> = const { Cell::new(false) };
    static FORCE_SET_HEAD_ERROR: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
struct ListForSelectorOverrideGuard {
    previous: Option<ListForSelectorOverrideFn>,
}

#[cfg(test)]
impl Drop for ListForSelectorOverrideGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        LIST_FOR_SELECTOR_OVERRIDE.with(|cell| {
            *cell.borrow_mut() = previous;
        });
    }
}

#[cfg(test)]
pub fn with_list_for_selector_override_for_tests<T>(
    override_fn: impl Fn(&Repository) -> Result<Vec<BranchInfo>> + 'static,
    f: impl FnOnce() -> T,
) -> T {
    let previous =
        LIST_FOR_SELECTOR_OVERRIDE.with(|cell| (*cell.borrow_mut()).replace(Box::new(override_fn)));
    let _guard = ListForSelectorOverrideGuard { previous };
    f()
}

fn revwalk_new(repo: &Repository) -> std::result::Result<git2::Revwalk<'_>, git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_REVWALK_NEW_ERROR.with(Cell::get) {
        return Err(git2::Error::from_str(
            "forced git revwalk_new failure for tests",
        ));
    }

    repo.revwalk()
}

fn revwalk_push(
    revwalk: &mut git2::Revwalk<'_>,
    oid: git2::Oid,
) -> std::result::Result<(), git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_REVWALK_PUSH_ERROR.with(Cell::get) {
        return Err(git2::Error::from_str(
            "forced git revwalk_push failure for tests",
        ));
    }

    revwalk.push(oid)
}

#[cfg(not(any(test, coverage)))]
const fn maybe_force_list_branch_result(
    branch_result: std::result::Result<(git2::Branch<'_>, BranchType), git2::Error>,
) -> std::result::Result<(git2::Branch<'_>, BranchType), git2::Error> {
    branch_result
}

#[cfg(any(test, coverage))]
fn maybe_force_list_branch_result(
    branch_result: std::result::Result<(git2::Branch<'_>, BranchType), git2::Error>,
) -> std::result::Result<(git2::Branch<'_>, BranchType), git2::Error> {
    if FORCE_LIST_BRANCH_RESULT_ERROR.with(|slot| slot.replace(false)) {
        return Err(git2::Error::from_str(
            "forced git branch iterator failure for tests",
        ));
    }

    branch_result
}

#[cfg(not(any(test, coverage)))]
const fn maybe_force_selector_local_branch_result(
    branch_result: std::result::Result<(git2::Branch<'_>, BranchType), git2::Error>,
) -> std::result::Result<(git2::Branch<'_>, BranchType), git2::Error> {
    branch_result
}

#[cfg(any(test, coverage))]
fn maybe_force_selector_local_branch_result(
    branch_result: std::result::Result<(git2::Branch<'_>, BranchType), git2::Error>,
) -> std::result::Result<(git2::Branch<'_>, BranchType), git2::Error> {
    if FORCE_SELECTOR_LOCAL_BRANCH_RESULT_ERROR.with(|slot| slot.replace(false)) {
        return Err(git2::Error::from_str(
            "forced git local branch iterator failure for tests",
        ));
    }

    branch_result
}

#[cfg(not(any(test, coverage)))]
const fn maybe_force_selector_remote_branch_result(
    branch_result: std::result::Result<(git2::Branch<'_>, BranchType), git2::Error>,
) -> std::result::Result<(git2::Branch<'_>, BranchType), git2::Error> {
    branch_result
}

#[cfg(any(test, coverage))]
fn maybe_force_selector_remote_branch_result(
    branch_result: std::result::Result<(git2::Branch<'_>, BranchType), git2::Error>,
) -> std::result::Result<(git2::Branch<'_>, BranchType), git2::Error> {
    if FORCE_SELECTOR_REMOTE_BRANCH_RESULT_ERROR.with(|slot| slot.replace(false)) {
        return Err(git2::Error::from_str(
            "forced git remote branch iterator failure for tests",
        ));
    }

    branch_result
}

fn remote_branches(repo: &Repository) -> std::result::Result<git2::Branches<'_>, git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_REMOTE_BRANCHES_LIST_ERROR.with(|slot| slot.replace(false)) {
        return Err(git2::Error::from_str(
            "forced git remote branches list failure for tests",
        ));
    }

    repo.branches(Some(BranchType::Remote))
}

fn checkout_tree(
    repo: &Repository,
    obj: &git2::Object<'_>,
) -> std::result::Result<(), git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_CHECKOUT_TREE_ERROR.with(|slot| slot.replace(false)) {
        return Err(git2::Error::from_str(
            "forced git checkout_tree failure for tests",
        ));
    }

    repo.checkout_tree(obj, None)
}

fn set_head(repo: &Repository, refname: &str) -> std::result::Result<(), git2::Error> {
    #[cfg(any(test, coverage))]
    if FORCE_SET_HEAD_ERROR.with(|slot| slot.replace(false)) {
        return Err(git2::Error::from_str(
            "forced git set_head failure for tests",
        ));
    }

    repo.set_head(refname)
}

/// Information about a git branch for the branch selector
#[derive(Debug, Clone)]
pub struct BranchInfo {
    /// Branch name (without remote prefix for remote branches)
    pub name: String,
    /// Full reference name (e.g., "refs/remotes/origin/main")
    pub full_name: String,
    /// Whether this is a remote branch
    pub is_remote: bool,
    /// Remote name (e.g., "origin") for remote branches
    pub remote: Option<String>,
    /// Last commit time (for sorting)
    pub last_commit_time: Option<SystemTime>,
}

/// Manager for git branch operations
pub struct Manager<'a> {
    repo: &'a Repository,
}

impl std::fmt::Debug for Manager<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manager").finish_non_exhaustive()
    }
}

impl<'a> Manager<'a> {
    /// Create a new branch manager for the given repository
    #[must_use]
    pub const fn new(repo: &'a Repository) -> Self {
        Self { repo }
    }

    /// Create a new branch from HEAD
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be created
    pub fn create(&self, name: &str) -> Result<()> {
        let head = self.repo.head().context("Failed to get HEAD reference")?;
        let commit = head.peel_to_commit().context("Failed to get HEAD commit")?;

        self.repo
            .branch(name, &commit, false)
            .with_context(|| format!("Failed to create branch '{name}'"))?;

        Ok(())
    }

    /// Create a new branch from a specific commit
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be created
    pub fn create_from_commit(&self, name: &str, commit_id: &str) -> Result<()> {
        let oid = git2::Oid::from_str(commit_id)
            .with_context(|| format!("Invalid commit ID: {commit_id}"))?;
        let commit = self
            .repo
            .find_commit(oid)
            .with_context(|| format!("Commit not found: {commit_id}"))?;

        self.repo
            .branch(name, &commit, false)
            .with_context(|| format!("Failed to create branch '{name}'"))?;

        Ok(())
    }

    /// Delete a local branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be deleted
    pub fn delete(&self, name: &str) -> Result<()> {
        let mut branch = self
            .repo
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("Branch not found: {name}"))?;

        branch
            .delete()
            .context(format!("Failed to delete branch '{name}'"))?;

        Ok(())
    }

    /// Check if a branch exists
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        self.repo.find_branch(name, BranchType::Local).is_ok()
    }

    /// Get the current branch name
    ///
    /// # Errors
    ///
    /// Returns an error if HEAD is not a branch
    pub fn current(&self) -> Result<String> {
        let head = self.repo.head().context("Failed to get HEAD")?;

        if head.is_branch() {
            let name = head.shorthand().context("Branch name is not valid UTF-8")?;
            Ok(name.to_string())
        } else {
            bail!("HEAD is not a branch (detached HEAD state)")
        }
    }

    /// List all local branches
    ///
    /// # Errors
    ///
    /// Returns an error if branches cannot be listed
    pub fn list(&self) -> Result<Vec<String>> {
        let branches = self
            .repo
            .branches(Some(BranchType::Local))
            .context("Failed to list branches")?;

        let mut names = Vec::new();
        for branch_result in branches {
            let (branch, _) =
                maybe_force_list_branch_result(branch_result).context("Failed to read branch")?;
            if let Some(name) = branch.name().ok().flatten() {
                names.push(name.to_string());
            }
        }

        Ok(names)
    }

    /// Checkout a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch cannot be checked out
    pub fn checkout(&self, name: &str) -> Result<()> {
        let refname = format!("refs/heads/{name}");
        let obj = self
            .repo
            .revparse_single(&refname)
            .with_context(|| format!("Branch not found: {name}"))?;

        checkout_tree(self.repo, &obj)
            .context(format!("Failed to checkout tree for branch '{name}'"))?;

        set_head(self.repo, &refname).context(format!("Failed to set HEAD to branch '{name}'"))?;

        Ok(())
    }

    /// Get the commit count on a branch
    ///
    /// # Errors
    ///
    /// Returns an error if the branch or commits cannot be read
    pub fn commit_count(&self, name: &str) -> Result<usize> {
        let branch = self
            .repo
            .find_branch(name, BranchType::Local)
            .with_context(|| format!("Branch not found: {name}"))?;

        let commit = branch
            .get()
            .peel_to_commit()
            .context("Failed to get branch commit")?;

        let mut revwalk = revwalk_new(self.repo).context("Failed to create revwalk")?;
        revwalk_push(&mut revwalk, commit.id()).context("Failed to push commit to revwalk")?;

        Ok(revwalk.count())
    }

    /// List all branches for the branch selector
    ///
    /// Returns branches sorted with:
    /// - "main" and "master" at the top (if they exist)
    /// - Local branches before remote branches
    /// - Within each section, sorted by most recent commit
    ///
    /// # Errors
    ///
    /// Returns an error if branches cannot be listed
    pub fn list_for_selector(&self) -> Result<Vec<BranchInfo>> {
        #[cfg(test)]
        {
            if let Some(result) = LIST_FOR_SELECTOR_OVERRIDE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .map(|override_fn| override_fn(self.repo))
            }) {
                return result;
            }
        }

        let mut local_branches = Vec::new();
        let mut remote_branch_infos = Vec::new();

        // Get local branches
        let branches = self
            .repo
            .branches(Some(BranchType::Local))
            .context("Failed to list local branches")?;

        for branch_result in branches {
            let (branch, _) = maybe_force_selector_local_branch_result(branch_result)
                .context("Failed to read branch")?;
            let Some(name) = branch.name().ok().flatten() else {
                continue;
            };

            let commit_time = branch
                .get()
                .peel_to_commit()
                .ok()
                .map(|c| c.time())
                .and_then(|t| {
                    u64::try_from(t.seconds())
                        .ok()
                        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
                });

            local_branches.push(BranchInfo {
                name: name.to_string(),
                full_name: format!("refs/heads/{name}"),
                is_remote: false,
                remote: None,
                last_commit_time: commit_time,
            });
        }

        // Get remote branches
        let branches = remote_branches(self.repo).context("Failed to list remote branches")?;

        for branch_result in branches {
            let (branch, _) = maybe_force_selector_remote_branch_result(branch_result)
                .context("Failed to read branch")?;
            let Some(full_name) = branch.name().ok().flatten() else {
                continue;
            };

            // Skip HEAD references like "origin/HEAD"
            if full_name.ends_with("/HEAD") {
                continue;
            }

            // Parse remote name and branch name (e.g., "origin/main" -> ("origin", "main"))
            let parts: Vec<&str> = full_name.splitn(2, '/').collect();
            let (remote_name, branch_name) = if parts.len() == 2 {
                (Some(parts[0].to_string()), parts[1].to_string())
            } else {
                (None, full_name.to_string())
            };

            let commit_time = branch
                .get()
                .peel_to_commit()
                .ok()
                .map(|c| c.time())
                .and_then(|t| {
                    u64::try_from(t.seconds())
                        .ok()
                        .and_then(|secs| UNIX_EPOCH.checked_add(Duration::from_secs(secs)))
                });

            remote_branch_infos.push(BranchInfo {
                name: branch_name,
                full_name: format!("refs/remotes/{full_name}"),
                is_remote: true,
                remote: remote_name,
                last_commit_time: commit_time,
            });
        }

        // Sort local branches: main/master first, then by most recent commit
        local_branches.sort_by(|a, b| {
            let a_priority = Self::branch_priority(&a.name);
            let b_priority = Self::branch_priority(&b.name);

            // Higher priority first (main/master)
            match b_priority.cmp(&a_priority) {
                std::cmp::Ordering::Equal => {
                    // Then by most recent commit
                    b.last_commit_time.cmp(&a.last_commit_time)
                }
                other => other,
            }
        });

        // Sort remote branches: main/master first, then by most recent commit
        remote_branch_infos.sort_by(|a, b| {
            let a_priority = Self::branch_priority(&a.name);
            let b_priority = Self::branch_priority(&b.name);

            match b_priority.cmp(&a_priority) {
                std::cmp::Ordering::Equal => b.last_commit_time.cmp(&a.last_commit_time),
                other => other,
            }
        });

        // Combine: local branches first, then remote
        let mut result = local_branches;
        result.extend(remote_branch_infos);
        Ok(result)
    }

    /// Get priority for branch name sorting (higher = comes first)
    fn branch_priority(name: &str) -> u8 {
        match name {
            "main" => 2,
            "master" => 1,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{RepositoryInitOptions, Signature};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn init_test_repo_with_commit() -> (TempDir, Repository) {
        let temp_dir = TempDir::new().expect("Create temp dir");
        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).expect("Init repository");
        repo.set_head("refs/heads/master").expect("Set HEAD");

        let sig = Signature::now("Test", "test@test.com").expect("Create signature");
        let file_path = temp_dir.path().join("README.md");
        fs::write(&file_path, "# Test").expect("Write README.md");

        let mut index = repo.index().expect("Open repository index");
        index
            .add_path(std::path::Path::new("README.md"))
            .expect("Add README.md");
        index.write().expect("Write index");

        let tree_id = index.write_tree().expect("Write tree");

        {
            let tree = repo.find_tree(tree_id).expect("Find tree");
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .expect("Commit README.md");
        }

        (temp_dir, repo)
    }

    #[test]
    fn test_create_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").expect("Create branch");
        assert!(manager.exists("feature/test"));
    }

    #[test]
    fn test_create_duplicate_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").expect("Create branch");
        let result = manager.create("feature/test");
        assert!(result.is_err());
    }

    #[test]
    fn test_delete_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").expect("Create branch");
        assert!(manager.exists("feature/test"));

        manager.delete("feature/test").expect("Delete branch");
        assert!(!manager.exists("feature/test"));
    }

    #[test]
    fn test_delete_nonexistent_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.delete("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_current_branch() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let current = manager.current().expect("Read current branch");
        assert_eq!(current, "master");
    }

    #[test]
    fn test_current_detached_head_reports_error() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");
        repo.set_head_detached(commit.id())
            .expect("Detach repository HEAD");

        let err = manager.current().unwrap_err();
        assert!(err.to_string().contains("detached HEAD"));
    }

    #[test]
    fn test_create_branch_errors_when_head_commit_is_missing() {
        let temp_dir = TempDir::new().expect("Create temp dir");
        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).expect("Init repository");
        repo.set_head("refs/heads/master").expect("Set HEAD");
        let blob = repo.blob(b"not-a-commit").expect("Create blob");
        repo.reference(
            "refs/heads/master",
            blob,
            true,
            "tenex-tests: create invalid master ref",
        )
        .expect("Create invalid refs/heads/master");
        let manager = Manager::new(&repo);

        let err = manager.create("feature/unborn").unwrap_err();
        assert!(err.to_string().contains("Failed to get HEAD commit"));
    }

    #[cfg(unix)]
    #[test]
    fn test_delete_branch_reports_delete_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager
            .create("feature/delete-permission-error")
            .expect("Create branch");

        let heads_dir = repo.path().join("refs").join("heads");
        let prev = fs::metadata(&heads_dir)
            .expect("Read refs/heads metadata")
            .permissions();
        let _guard = DirPermissionsGuard::new(&heads_dir, prev);

        let mut perms = fs::metadata(&heads_dir)
            .expect("Read refs/heads metadata")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&heads_dir, perms).expect("Set refs/heads read-only");

        let err = manager
            .delete("feature/delete-permission-error")
            .unwrap_err();
        assert!(err.to_string().contains("Failed to delete branch"));
    }

    #[test]
    fn test_current_reports_error_when_head_unreadable() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::remove_file(repo.path().join("HEAD")).expect("Remove HEAD");

        let err = manager.current().unwrap_err();
        assert!(err.to_string().contains("Failed to get HEAD"));
    }

    #[cfg(unix)]
    #[test]
    fn test_current_reports_error_when_branch_name_is_not_utf8() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");

        // macOS filesystems reject non-UTF8 filenames, so we simulate a non-UTF8 refname
        // via packed-refs content instead of creating a ref file on disk.
        let mut packed = Vec::new();
        packed.extend_from_slice(commit.id().to_string().as_bytes());
        packed.push(b' ');
        packed.extend_from_slice(b"refs/heads/bad-branch-");
        packed.push(0xff);
        packed.push(b'\n');
        fs::write(repo.path().join("packed-refs"), packed).expect("write packed-refs");

        let mut head_bytes = b"ref: refs/heads/bad-branch-".to_vec();
        head_bytes.push(0xff);
        head_bytes.push(b'\n');
        fs::write(repo.path().join("HEAD"), head_bytes).expect("write HEAD");

        let err = manager.current().unwrap_err();
        assert!(err.to_string().contains("Branch name is not valid UTF-8"));
    }

    #[test]
    fn test_list_reports_error_when_branches_cannot_be_listed() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        fs::write(
            repo.path().join("packed-refs"),
            "this-is-not-a-packed-refs-file\n",
        )
        .expect("Write invalid packed-refs");

        let err = manager.list().unwrap_err();
        assert!(err.to_string().contains("Failed to list branches"));
    }

    #[test]
    fn test_list_propagates_branch_iterator_errors() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = with_forced_list_branch_result_error(|| manager.list()).unwrap_err();
        assert!(err.to_string().contains("Failed to read branch"));
    }

    #[cfg(unix)]
    #[test]
    fn test_list_skips_local_branch_with_non_utf8_name() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");

        let mut packed = Vec::new();
        packed.extend_from_slice(commit.id().to_string().as_bytes());
        packed.push(b' ');
        packed.extend_from_slice(b"refs/heads/bad-branch-");
        packed.push(0xff);
        packed.push(b'\n');
        fs::write(repo.path().join("packed-refs"), packed).expect("write packed-refs");

        let branches = manager.list().expect("List branches");
        assert!(!branches.is_empty());
    }

    #[test]
    fn test_checkout_reports_checkout_tree_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/checkout-permission-error").unwrap();

        let err = with_forced_checkout_tree_error(|| {
            manager.checkout("feature/checkout-permission-error")
        })
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to checkout tree for branch")
        );
    }

    #[test]
    fn test_checkout_reports_set_head_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/head-permission-error").unwrap();

        let err = with_forced_set_head_error(|| manager.checkout("feature/head-permission-error"))
            .unwrap_err();
        assert!(err.to_string().contains("Failed to set HEAD to branch"));
    }

    #[test]
    fn test_commit_count_reports_peel_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read HEAD");
        let commit = head.peel_to_commit().expect("Peel commit");
        let tree = commit.tree().expect("Read tree");

        repo.reference(
            "refs/heads/not-a-commit",
            tree.id(),
            true,
            "tenex-tests: create invalid branch target",
        )
        .expect("Create tree ref");

        let err = manager.commit_count("not-a-commit").unwrap_err();
        assert!(err.to_string().contains("Failed to get branch commit"));
    }

    #[cfg(any(test, coverage))]
    fn with_forced_revwalk_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_REVWALK_NEW_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_revwalk_push_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_REVWALK_PUSH_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_list_branch_result_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_LIST_BRANCH_RESULT_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_selector_local_branch_result_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_SELECTOR_LOCAL_BRANCH_RESULT_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_selector_remote_branch_result_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_SELECTOR_REMOTE_BRANCH_RESULT_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_remote_branches_list_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_REMOTE_BRANCHES_LIST_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_checkout_tree_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_CHECKOUT_TREE_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[cfg(any(test, coverage))]
    fn with_forced_set_head_error<T>(f: impl FnOnce() -> T) -> T {
        FORCE_SET_HEAD_ERROR.with(|slot| {
            let prev = slot.replace(true);
            let out = f();
            slot.set(prev);
            out
        })
    }

    #[test]
    fn test_commit_count_reports_revwalk_create_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = with_forced_revwalk_error(|| manager.commit_count("master")).unwrap_err();
        assert!(err.to_string().contains("Failed to create revwalk"));
    }

    #[test]
    fn test_commit_count_reports_revwalk_push_errors_with_context() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = with_forced_revwalk_push_error(|| manager.commit_count("master")).unwrap_err();
        assert!(err.to_string().contains("Failed to push commit to revwalk"));
    }

    #[test]
    fn test_list_for_selector_propagates_local_branch_iteration_errors() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err = with_forced_selector_local_branch_result_error(|| manager.list_for_selector())
            .unwrap_err();
        assert!(err.to_string().contains("Failed to read branch"));
    }

    #[test]
    fn test_list_for_selector_reports_error_when_remote_refs_are_unreadable() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let err =
            with_forced_remote_branches_list_error(|| manager.list_for_selector()).unwrap_err();
        assert!(err.to_string().contains("Failed to list remote branches"));
    }

    #[test]
    fn test_list_for_selector_propagates_remote_branch_iteration_errors() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let remote_dir = repo.path().join("refs").join("remotes").join("origin");
        fs::create_dir_all(&remote_dir).expect("Create refs/remotes/origin");
        let commit_id = repo
            .head()
            .expect("Read HEAD")
            .peel_to_commit()
            .expect("Peel commit")
            .id();
        repo.reference(
            "refs/remotes/origin/master",
            commit_id,
            true,
            "tenex-tests: create remote tracking ref",
        )
        .expect("Create remote tracking ref");

        let err = with_forced_selector_remote_branch_result_error(|| manager.list_for_selector())
            .unwrap_err();
        assert!(err.to_string().contains("Failed to read branch"));
    }

    #[cfg(unix)]
    struct DirPermissionsGuard {
        path: std::path::PathBuf,
        previous: std::fs::Permissions,
    }

    #[cfg(unix)]
    impl DirPermissionsGuard {
        fn new(path: &std::path::Path, previous: std::fs::Permissions) -> Self {
            Self {
                path: path.to_path_buf(),
                previous,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for DirPermissionsGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.path, self.previous.clone());
        }
    }

    #[test]
    fn test_list_branches() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/a").expect("Create branch");
        manager.create("feature/b").expect("Create branch");

        let branches = manager.list().expect("List branches");
        assert!(branches.len() >= 3);
        assert!(branches.iter().any(|b| b == "feature/a"));
        assert!(branches.iter().any(|b| b == "feature/b"));
    }

    #[test]
    fn test_exists() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        assert!(!manager.exists("nonexistent"));
        manager.create("new-branch").expect("Create branch");
        assert!(manager.exists("new-branch"));
    }

    #[test]
    fn test_checkout() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        manager.create("feature/test").expect("Create branch");
        manager.checkout("feature/test").expect("Checkout branch");

        assert_eq!(
            manager.current().expect("Read current branch"),
            "feature/test"
        );
    }

    #[test]
    fn test_checkout_nonexistent() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.checkout("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_commit_count() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let current = manager.current().expect("Read current branch");
        let count = manager
            .commit_count(&current)
            .expect("Count commits on branch");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_create_from_commit() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");
        let commit_id = commit.id().to_string();

        manager
            .create_from_commit("from-commit", &commit_id)
            .expect("Create branch from commit");
        assert!(manager.exists("from-commit"));
    }

    #[test]
    fn test_create_from_invalid_commit() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let result = manager.create_from_commit("test", "invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_debug() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let debug = format!("{manager:?}");
        assert!(debug.contains("Manager"));
    }

    #[test]
    fn test_branch_priority() {
        // main gets highest priority
        assert_eq!(Manager::branch_priority("main"), 2);
        // master gets second priority
        assert_eq!(Manager::branch_priority("master"), 1);
        // Other branches get zero priority
        assert_eq!(Manager::branch_priority("feature"), 0);
        assert_eq!(Manager::branch_priority("develop"), 0);
        assert_eq!(Manager::branch_priority("main-feature"), 0);
    }

    #[test]
    fn test_list_for_selector() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        // Create some test branches
        manager.create("feature/a").expect("Create branch");
        manager.create("main").expect("Create branch");
        manager.create("develop").expect("Create branch");

        let branches = manager
            .list_for_selector()
            .expect("List branches for selector");

        // Should have branches
        assert!(!branches.is_empty());

        // Find the main branch index
        let main_idx = branches
            .iter()
            .position(|b| b.name == "main")
            .expect("main branch should be present");
        let feature_idx = branches
            .iter()
            .position(|b| b.name == "feature/a")
            .expect("feature branch should be present");

        assert!(main_idx < feature_idx);
    }

    #[test]
    fn test_list_for_selector_local_before_remote() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let remote_dir = TempDir::new().expect("Create remote dir");
        Repository::init_bare(remote_dir.path()).expect("Init bare remote");

        let remote_path = remote_dir
            .path()
            .to_str()
            .expect("Remote path should be valid UTF-8");
        repo.remote("origin", remote_path)
            .expect("Add origin remote");

        let current = manager.current().expect("Read current branch");
        let push_ref = format!("refs/heads/{current}:refs/heads/{current}");
        {
            let mut remote = repo.find_remote("origin").expect("Find origin remote");
            remote
                .push(&[push_ref.as_str()], None)
                .expect("Push branch");
        }

        let fetch_ref = format!("refs/heads/{current}:refs/remotes/origin/{current}");
        {
            let mut remote = repo.find_remote("origin").expect("Find origin remote");
            remote
                .fetch(&[fetch_ref.as_str()], None, None)
                .expect("Fetch branch");
        }

        let branches = manager
            .list_for_selector()
            .expect("List branches for selector");

        // Find first remote branch (if any)
        let first_remote_idx = branches.iter().position(|b| b.is_remote);

        let remote_idx = first_remote_idx.expect("Expected at least one remote branch");
        for branch in &branches[..remote_idx] {
            assert!(!branch.is_remote);
        }
    }

    #[test]
    fn test_list_for_selector_includes_remote_branches() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let remote_dir = TempDir::new().expect("Create remote dir");
        Repository::init_bare(remote_dir.path()).expect("Init bare remote");

        let remote_path = remote_dir
            .path()
            .to_str()
            .expect("Remote path should be valid UTF-8");
        repo.remote("origin", remote_path)
            .expect("Add origin remote");

        let current = manager.current().expect("Read current branch");
        let push_ref = format!("refs/heads/{current}:refs/heads/{current}");
        {
            let mut remote = repo.find_remote("origin").expect("Find origin remote");
            remote
                .push(&[push_ref.as_str()], None)
                .expect("Push branch");
        }

        let fetch_ref = format!("refs/heads/{current}:refs/remotes/origin/{current}");
        {
            let mut remote = repo.find_remote("origin").expect("Find origin remote");
            remote
                .fetch(&[fetch_ref.as_str()], None, None)
                .expect("Fetch branch");
        }

        // Ensure we have an origin/HEAD reference so we deterministically exercise the
        // code path that skips it in list_for_selector (git/libgit2 behavior varies).
        let origin_head_target = format!("refs/remotes/origin/{current}");
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            &origin_head_target,
            true,
            "tenex-tests: set origin/HEAD",
        )
        .expect("Set origin/HEAD symbolic reference");

        let branches = manager
            .list_for_selector()
            .expect("List branches for selector");
        let remote_branch = branches
            .iter()
            .find(|branch| branch.is_remote && branch.name == current)
            .expect("Expected remote branch");
        assert_eq!(remote_branch.remote.as_deref(), Some("origin"));
        assert!(remote_branch.full_name.starts_with("refs/remotes/origin/"));
    }

    #[test]
    fn test_list_for_selector_parses_remote_without_slash() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");
        repo.reference(
            "refs/remotes/weird",
            commit.id(),
            true,
            "tenex-tests: set refs/remotes/weird",
        )
        .expect("Create remote ref without slash");

        let branches = manager
            .list_for_selector()
            .expect("List branches for selector");
        let remote_branch = branches
            .iter()
            .find(|branch| branch.is_remote && branch.full_name == "refs/remotes/weird")
            .expect("Expected weird remote ref");
        assert_eq!(remote_branch.name, "weird");
        assert!(remote_branch.remote.is_none());
    }

    #[test]
    #[cfg(unix)]
    fn test_list_for_selector_skips_local_branch_with_non_utf8_name() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");

        let mut packed = Vec::new();
        packed.extend_from_slice(commit.id().to_string().as_bytes());
        packed.push(b' ');
        packed.extend_from_slice(b"refs/heads/bad-branch-");
        packed.push(0xff);
        packed.push(b'\n');
        fs::write(repo.path().join("packed-refs"), packed).expect("write packed-refs");

        let branches = manager
            .list_for_selector()
            .expect("List branches for selector");
        assert!(!branches.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn test_list_for_selector_skips_remote_branch_with_non_utf8_name() {
        let (_temp_dir, repo) = init_test_repo_with_commit();
        let manager = Manager::new(&repo);

        let head = repo.head().expect("Read repository HEAD");
        let commit = head.peel_to_commit().expect("Peel HEAD to commit");

        let mut packed = Vec::new();
        packed.extend_from_slice(commit.id().to_string().as_bytes());
        packed.push(b' ');
        packed.extend_from_slice(b"refs/remotes/origin/bad-remote-");
        packed.push(0xff);
        packed.push(b'\n');
        fs::write(repo.path().join("packed-refs"), packed).expect("write packed-refs");

        let branches = manager
            .list_for_selector()
            .expect("List branches for selector");
        assert!(!branches.is_empty());
    }

    #[test]
    fn test_branch_info_debug() {
        let info = BranchInfo {
            name: "test".to_string(),
            full_name: "refs/heads/test".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("test"));
        assert!(debug.contains("BranchInfo"));
    }

    #[test]
    fn test_branch_info_clone() {
        let info = BranchInfo {
            name: "test".to_string(),
            full_name: "refs/heads/test".to_string(),
            is_remote: false,
            remote: Some("origin".to_string()),
            last_commit_time: Some(SystemTime::now()),
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, info.name);
        assert_eq!(cloned.full_name, info.full_name);
        assert_eq!(cloned.is_remote, info.is_remote);
        assert_eq!(cloned.remote, info.remote);
    }
}
