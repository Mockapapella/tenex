//! Tests for git worktree operations

use crate::common::TestFixture;

#[test]
fn test_git_worktree_create_and_remove() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("worktree")?;
    let repo = tenex::git::open_repository(&fixture.repo_path)?;
    let manager = tenex::git::WorktreeManager::new(&repo);

    let worktree_path = fixture.worktree_dir.path().join("test-worktree");
    let branch_name = "test-branch";

    // Create worktree with new branch
    let result = manager.create_with_new_branch(&worktree_path, branch_name);
    assert!(result.is_ok(), "Failed to create worktree: {result:?}");

    // Verify worktree exists
    assert!(worktree_path.exists());
    assert!(worktree_path.join(".git").exists());

    // Remove worktree
    let result = manager.remove(branch_name);
    assert!(result.is_ok(), "Failed to remove worktree: {result:?}");

    Ok(())
}

#[test]
fn test_git_exclude_tenex_directory() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = TestFixture::new("git_exclude")?;

    // Call ensure_tenex_excluded
    let result = tenex::git::ensure_tenex_excluded(&fixture.repo_path);
    assert!(result.is_ok());

    // Check that .git/info/exclude contains .tenex/
    let exclude_path = fixture.repo_path.join(".git/info/exclude");
    assert!(exclude_path.exists());

    let contents = std::fs::read_to_string(&exclude_path)?;
    assert!(
        contents.contains(".tenex/"),
        "Exclude file should contain .tenex/"
    );

    // Call again - should be idempotent
    let result = tenex::git::ensure_tenex_excluded(&fixture.repo_path);
    assert!(result.is_ok());

    // Should still only have one .tenex/ entry
    let contents = std::fs::read_to_string(&exclude_path)?;
    let count = contents.matches(".tenex/").count();
    assert_eq!(count, 1, "Should only have one .tenex/ entry");

    Ok(())
}
