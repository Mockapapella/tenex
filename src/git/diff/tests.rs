use super::*;
use git2::Signature;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_parse_diff_model_trims_crlf_line_endings() {
    let patch = concat!(
        "diff --git a/foo.txt b/foo.txt\r\n",
        "index 0000000..0000000 100644\r\n",
        "--- a/foo.txt\r\n",
        "+++ b/foo.txt\r\n",
        "@@ -1,1 +1,1 @@\r\n",
        "-old\r\n",
        "+new\r\n",
    );
    let diff = git2::Diff::from_buffer(patch.as_bytes()).expect("Expected diff buffer");

    let mut saw_crlf = false;
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        if line.content().ends_with(b"\r\n") {
            saw_crlf = true;
        }
        true
    })
    .expect("Expected diff print");
    assert!(saw_crlf);

    let model = Generator::parse_diff_model(&diff).expect("Expected diff model parse");
    for file in &model.files {
        for meta in &file.meta {
            assert!(!meta.ends_with('\r'));
        }
        for hunk in &file.hunks {
            assert!(!hunk.header.ends_with('\r'));
            for line in &hunk.lines {
                assert!(!line.content.ends_with('\r'));
            }
        }
    }

    let digest = Generator::digest_diff(&diff).expect("Expected diff digest");
    assert!(digest.hash != 0);
}

#[test]
fn test_parse_diff_reports_print_failures_with_context() {
    let patch = concat!(
        "diff --git a/foo.txt b/foo.txt\n",
        "index 0000000..0000000 100644\n",
        "--- a/foo.txt\n",
        "+++ b/foo.txt\n",
        "@@ -1,1 +1,1 @@\n",
        "-old\n",
        "+new\n",
    );
    let diff = git2::Diff::from_buffer(patch.as_bytes()).expect("Expected diff buffer");

    with_forced_diff_print_error_for_tests(|| {
        let err = Generator::parse_diff(&diff).expect_err("Expected parse diff to fail");
        assert!(err.to_string().contains("Failed to parse diff"));

        let err =
            Generator::parse_diff_model(&diff).expect_err("Expected parse diff model to fail");
        assert!(err.to_string().contains("Failed to parse diff patch"));

        let err = Generator::digest_diff(&diff).expect_err("Expected digest diff to fail");
        assert!(err.to_string().contains("Failed to compute diff digest"));
    });
}

fn init_test_repo_with_commit() -> (TempDir, Repository) {
    let temp_dir = TempDir::new().expect("Expected temp dir");
    let repo = Repository::init(temp_dir.path()).expect("Expected repository");

    let sig = Signature::now("Test", "test@test.com").expect("Expected signature");
    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Test\n").expect("Expected README write");

    let mut index = repo.index().expect("Expected repo index");
    index
        .add_path(std::path::Path::new("README.md"))
        .expect("Expected index add");
    index.write().expect("Expected index write");

    let tree_id = index.write_tree().expect("Expected tree write");

    {
        let tree = repo.find_tree(tree_id).expect("Expected tree");
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .expect("Expected commit");
    }

    (temp_dir, repo)
}

fn init_bare_repo() -> (TempDir, Repository) {
    let temp_dir = TempDir::new().expect("Expected temp dir");
    let repo = Repository::init_bare(temp_dir.path()).expect("Expected bare repository");
    (temp_dir, repo)
}

#[test]
fn test_no_changes() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let uncommitted = generator.uncommitted().expect("Expected uncommitted diff");
    assert!(uncommitted.is_empty());
    assert!(!generator.has_changes().expect("Expected has_changes"));
}

#[test]
fn test_unstaged_changes() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Test\n\nNew content\n").expect("Expected README write");

    let unstaged = generator.unstaged().expect("Expected unstaged diff");
    assert_eq!(unstaged.len(), 1);
    assert_eq!(unstaged[0].status, FileStatus::Modified);
    assert!(generator.has_changes().expect("Expected has_changes"));
}

#[test]
fn test_staged_changes() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let new_file = temp_dir.path().join("new.txt");
    fs::write(&new_file, "New file content\n").expect("Expected new file write");

    let mut index = repo.index().expect("Expected repo index");
    index
        .add_path(std::path::Path::new("new.txt"))
        .expect("Expected index add");
    index.write().expect("Expected index write");

    let staged = generator.staged().expect("Expected staged diff");
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].status, FileStatus::Added);
}

#[test]
fn test_staged_includes_context_when_index_read_fails() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let index_path = repo.path().join("index");
    fs::remove_file(&index_path).ok();
    fs::create_dir(&index_path).expect("Expected index path to be a directory");

    let err = generator
        .staged()
        .expect_err("expected staged diff to fail when index path is invalid");
    assert!(err.to_string().contains("Failed to get staged diff"));
}

#[test]
fn test_worktree_file_sample_hash_falls_back_when_reads_fail() {
    let temp_dir = TempDir::new().expect("Expected temp dir");
    let dir_path = temp_dir.path();

    for len in [1_u64, 5000_u64] {
        let mut expected = DefaultHasher::new();
        expected.write_u64(len);
        let expected = expected.finish();

        assert_eq!(worktree_file_sample_hash(dir_path, len), expected);
    }
}

#[test]
fn test_staged_skips_index_guard_when_index_is_missing() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let index_path = repo.path().join("index");
    fs::remove_file(&index_path).expect("Expected index file removal");
    assert!(!index_path.exists());

    with_forced_repo_diff_tree_to_index_error_for_tests(|| {
        let err = generator
            .staged()
            .expect_err("expected staged diff to fail when index is missing");
        assert!(err.to_string().contains("Failed to get staged diff"));
    });
}

#[test]
fn test_staged_includes_context_when_diff_tree_to_index_errors() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    with_forced_repo_diff_tree_to_index_error_for_tests(|| {
        let err = generator
            .staged()
            .expect_err("expected staged diff to fail when diff_tree_to_index fails");
        assert!(err.to_string().contains("Failed to get staged diff"));
    });
}

#[test]
fn test_uncommitted_changes() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Modified\n").expect("Expected README write");

    let uncommitted = generator.uncommitted().expect("Expected uncommitted diff");
    assert!(!uncommitted.is_empty());
}

#[test]
fn test_untracked_file_included_in_uncommitted() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let untracked_path = temp_dir.path().join("untracked.txt");
    fs::write(&untracked_path, "hello\n").expect("Expected untracked write");

    let files = generator.uncommitted().expect("Expected uncommitted diff");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].status, FileStatus::Untracked);
    assert_eq!(files[0].path.as_path(), Path::new("untracked.txt"));
    assert!(files[0].additions > 0);
    assert!(generator.has_changes().expect("Expected has_changes"));
}

#[test]
fn test_untracked_file_in_directory_included_in_uncommitted() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let dir = temp_dir.path().join("newdir");
    fs::create_dir_all(&dir).expect("Expected dir create");
    fs::write(dir.join("nested.txt"), "nested\n").expect("Expected nested write");

    let files = generator.uncommitted().expect("Expected uncommitted diff");
    assert!(
        files
            .iter()
            .any(|file| file.path.as_path() == Path::new("newdir/nested.txt"))
    );
}

#[test]
fn test_uncommitted_change_marker_zero_when_clean() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    assert_eq!(
        generator
            .uncommitted_change_marker()
            .expect("Expected change marker"),
        0
    );
}

#[test]
fn test_diff_methods_include_context_for_bare_repo() {
    let (_temp_dir, repo) = init_bare_repo();
    let generator = Generator::new(&repo);

    let unstaged_err = generator
        .unstaged()
        .expect_err("expected unstaged diff to fail");
    assert!(
        unstaged_err
            .to_string()
            .contains("Failed to get unstaged diff")
    );

    let uncommitted_err = generator
        .uncommitted()
        .expect_err("expected uncommitted diff to fail");
    assert!(
        uncommitted_err
            .to_string()
            .contains("Failed to get uncommitted diff")
    );

    let digest_err = generator
        .uncommitted_digest()
        .expect_err("expected uncommitted digest to fail");
    assert!(
        digest_err
            .to_string()
            .contains("Failed to get uncommitted diff")
    );

    let marker_err = generator
        .uncommitted_change_marker()
        .expect_err("expected uncommitted change marker to fail");
    assert!(
        marker_err
            .to_string()
            .contains("Failed to get repository status for diff marker")
    );
}

#[test]
fn test_summary_includes_context_for_bare_repo() {
    let (_temp_dir, repo) = init_bare_repo();
    let generator = Generator::new(&repo);

    let err = generator
        .summary()
        .expect_err("expected summary to fail for bare repo");
    assert!(err.to_string().contains("Failed to get uncommitted diff"));
}

#[test]
fn test_has_changes_includes_context_for_bare_repo() {
    let (_temp_dir, repo) = init_bare_repo();
    let generator = Generator::new(&repo);

    let err = generator
        .has_changes()
        .expect_err("expected has_changes to fail for bare repo");
    assert!(err.to_string().contains("Failed to get repository status"));
}

#[test]
fn test_uncommitted_change_marker_changes_when_worktree_file_changes() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Modified\n").expect("Expected README write");
    let first = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    fs::write(&file_path, "# Modified again with more bytes\nextra\n")
        .expect("Expected README write");
    let second = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    assert_ne!(first, 0);
    assert_ne!(first, second);
}

#[test]
fn test_uncommitted_change_marker_changes_when_worktree_file_changes_same_size() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "hello world\n").expect("Expected README write");
    let first = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    fs::write(&file_path, "hello again\n").expect("Expected README write");
    let second = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    assert_ne!(first, 0);
    assert_ne!(first, second);
}

#[test]
fn test_uncommitted_change_marker_changes_when_staged_blob_changes() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# First staged version\n").expect("Expected README write");
    let mut index = repo.index().expect("Expected repo index");
    index
        .add_path(Path::new("README.md"))
        .expect("Expected add path");
    index.write().expect("Expected index write");
    let first = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    fs::write(&file_path, "# Second staged version with more bytes\n")
        .expect("Expected README write");
    let mut index = repo.index().expect("Expected repo index");
    index
        .add_path(Path::new("README.md"))
        .expect("Expected add path");
    index.write().expect("Expected index write");
    let second = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    assert_ne!(first, 0);
    assert_ne!(first, second);
}

#[test]
fn test_uncommitted_change_marker_changes_when_untracked_nested_file_changes() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let dir = temp_dir.path().join("repro");
    fs::create_dir_all(&dir).expect("Expected dir create");
    let nested = dir.join("nested.bin");
    fs::write(&nested, b"first payload").expect("Expected nested write");
    let first = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&nested, b"second payload with different size").expect("Expected nested write");
    let second = generator
        .uncommitted_change_marker()
        .expect("Expected change marker");

    assert_ne!(first, 0);
    assert_ne!(first, second);
}

#[test]
fn test_summary() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Modified\nNew line\n").expect("Expected README write");

    let summary = generator.summary().expect("Expected diff summary");
    assert_eq!(summary.files_changed, 1);
    assert!((summary.additions > 0) | (summary.deletions > 0));
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
fn test_between_commits() {
    let (temp_dir, repo) = init_test_repo_with_commit();

    let sig = Signature::now("Test", "test@test.com").expect("Expected signature");
    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# Modified\n").expect("Expected README write");

    let mut index = repo.index().expect("Expected repo index");
    index
        .add_path(std::path::Path::new("README.md"))
        .expect("Expected index add");
    index.write().expect("Expected index write");

    let tree_id = index.write_tree().expect("Expected tree");
    let tree = repo.find_tree(tree_id).expect("Expected tree");
    let head = repo
        .head()
        .expect("Expected head")
        .peel_to_commit()
        .expect("Expected commit");

    repo.commit(Some("HEAD"), &sig, &sig, "Second commit", &tree, &[&head])
        .expect("Expected commit");

    let generator = Generator::new(&repo);
    let diff = generator
        .between_commits("HEAD~1", "HEAD")
        .expect("Expected diff");
    assert_eq!(diff.len(), 1);
}

#[test]
fn test_between_commits_reports_tree_and_diff_errors_with_context() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    with_forced_commit_tree_error_on_call_for_tests(1, || {
        let err = generator
            .between_commits("HEAD", "HEAD")
            .expect_err("expected between_commits to fail when old tree lookup fails");
        assert!(err.to_string().contains("Could not get old tree"));
    });

    with_forced_commit_tree_error_on_call_for_tests(2, || {
        let err = generator
            .between_commits("HEAD", "HEAD")
            .expect_err("expected between_commits to fail when new tree lookup fails");
        assert!(err.to_string().contains("Could not get new tree"));
    });

    with_forced_repo_diff_tree_to_tree_error_for_tests(|| {
        let err = generator
            .between_commits("HEAD", "HEAD")
            .expect_err("expected between_commits to fail when tree diff fails");
        assert!(err.to_string().contains("Failed to diff trees"));
    });
}

#[test]
fn test_between_commits_invalid() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let result = generator.between_commits("invalid", "HEAD");
    assert!(result.is_err());
}

#[test]
fn test_between_commits_invalid_new_reference_reports_error() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let result = generator.between_commits("HEAD", "invalid");
    assert!(result.is_err());
}

#[test]
fn test_between_commits_old_reference_not_commit_reports_error() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let result = generator.between_commits("HEAD^{tree}", "HEAD");
    assert!(result.is_err());
}

#[test]
fn test_between_commits_new_reference_not_commit_reports_error() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    let result = generator.between_commits("HEAD", "HEAD^{tree}");
    assert!(result.is_err());
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

#[test]
fn test_file_status_rank_covers_all_variants() {
    assert_eq!(file_status_rank(FileStatus::Added), 1);
    assert_eq!(file_status_rank(FileStatus::Deleted), 2);
    assert_eq!(file_status_rank(FileStatus::Modified), 3);
    assert_eq!(file_status_rank(FileStatus::Renamed), 4);
    assert_eq!(file_status_rank(FileStatus::Copied), 5);
    assert_eq!(file_status_rank(FileStatus::TypeChange), 6);
    assert_eq!(file_status_rank(FileStatus::Untracked), 7);
    assert_eq!(file_status_rank(FileStatus::Unknown), 8);
}

#[test]
fn test_generator_debug_is_non_exhaustive() {
    let (_temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);
    let rendered = format!("{generator:?}");
    assert!(rendered.contains("Generator"));
}

#[test]
fn test_worktree_file_sample_hash_handles_missing_and_zero_length() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let missing = tmp.path().join("missing.bin");
    let hash = worktree_file_sample_hash(&missing, 123);
    assert_ne!(hash, 0);

    let empty = tmp.path().join("empty.bin");
    fs::write(&empty, b"").expect("Expected empty write");
    let empty_hash = worktree_file_sample_hash(&empty, 0);
    assert_ne!(empty_hash, 0);
}

#[test]
fn test_worktree_file_sample_hash_reads_tail_for_large_files() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let large = tmp.path().join("large.bin");
    let payload = vec![b'x'; 9000];
    fs::write(&large, payload).expect("Expected large write");
    let hash = worktree_file_sample_hash(&large, 9000);
    assert_ne!(hash, 0);
}

#[test]
fn test_worktree_file_sample_hash_handles_len_conversion_overflow() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let path = tmp.path().join("small.bin");
    fs::write(&path, b"hello").expect("Expected small write");
    let hash = worktree_file_sample_hash(&path, u64::MAX);
    assert_ne!(hash, 0);
}

#[test]
fn test_worktree_meta_marker_handles_missing_inputs() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let root = tmp.path();

    assert!(worktree_meta_marker(Some(root), None, false).is_none());
    assert!(worktree_meta_marker(None, Some(Path::new("relative.txt")), false).is_none());

    let missing = root.join("missing.txt");
    assert!(worktree_meta_marker(Some(root), Some(&missing), false).is_none());
}

#[test]
fn test_worktree_meta_marker_returns_none_when_metadata_modified_errors() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let file = tmp.path().join("file.txt");
    fs::write(&file, b"hello").expect("Expected file write");

    with_forced_metadata_modified_error_for_tests(|| {
        assert!(worktree_meta_marker(None, Some(&file), true).is_none());
    });
}

#[test]
fn test_worktree_meta_marker_returns_none_when_modified_time_is_pre_epoch() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let file = tmp.path().join("file.txt");
    fs::write(&file, b"hello").expect("Expected file write");

    with_forced_metadata_modified_pre_epoch_for_tests(|| {
        assert!(worktree_meta_marker(None, Some(&file), true).is_none());
    });
}

#[test]
fn test_worktree_meta_marker_reports_expected_kinds() {
    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let root = tmp.path();

    let file = root.join("file.txt");
    fs::write(&file, b"hello").expect("Expected file write");
    let file_marker = worktree_meta_marker(None, Some(&file), true).expect("file marker");
    assert_eq!(file_marker.kind, 1);
    assert_ne!(file_marker.sample_hash, 0);

    let dir = root.join("dir");
    fs::create_dir_all(&dir).expect("Expected dir create");
    let dir_marker = worktree_meta_marker(None, Some(&dir), true).expect("dir marker");
    assert_eq!(dir_marker.kind, 2);
    assert_eq!(dir_marker.sample_hash, 0);

    #[cfg(unix)]
    {
        let link = root.join("link.txt");
        std::os::unix::fs::symlink(&file, &link).expect("Expected symlink");
        let link_marker = worktree_meta_marker(None, Some(&link), true).expect("link marker");
        assert_eq!(link_marker.kind, 3);
        assert_eq!(link_marker.sample_hash, 0);
    }
}

#[cfg(unix)]
#[test]
fn test_worktree_meta_marker_kind_for_unix_socket() {
    use std::os::unix::net::UnixListener;

    let tmp = tempfile::TempDir::new().expect("Expected temp dir");
    let socket_path = tmp.path().join("sock");
    let _listener = UnixListener::bind(&socket_path).expect("Expected socket bind");
    let marker = worktree_meta_marker(None, Some(&socket_path), true).expect("socket marker");
    assert_eq!(marker.kind, 4);
}

#[test]
fn test_push_model_hunk_line_infers_default_hunk() {
    let mut file = DiffFile {
        path: PathBuf::from("file.txt"),
        status: FileStatus::Modified,
        meta: Vec::new(),
        hunks: Vec::new(),
        additions: 0,
        deletions: 0,
    };

    push_model_hunk_line(&mut file, '+', "hello".to_string(), None, Some(1));
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunks[0].header, "@@ -0,0 +0,0 @@");
    assert_eq!(file.hunks[0].lines.len(), 1);
}

#[test]
fn test_push_model_hunk_line_appends_to_existing_hunk() {
    let mut file = DiffFile {
        path: PathBuf::from("file.txt"),
        status: FileStatus::Modified,
        meta: Vec::new(),
        hunks: vec![DiffHunk {
            header: "@@ -1 +1 @@".to_string(),
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: Vec::new(),
        }],
        additions: 0,
        deletions: 0,
    };

    push_model_hunk_line(&mut file, '-', "goodbye".to_string(), Some(1), None);
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunks[0].lines.len(), 1);
}

#[test]
fn test_uncommitted_digest_reports_hash_and_summary() {
    let (temp_dir, repo) = init_test_repo_with_commit();
    let generator = Generator::new(&repo);

    assert_eq!(
        generator
            .uncommitted_digest()
            .expect("Expected digest")
            .hash,
        0
    );

    fs::write(temp_dir.path().join("README.md"), "hello\r\nworld\r\n")
        .expect("Expected README write");
    let digest = generator.uncommitted_digest().expect("Expected digest");
    assert_ne!(digest.hash, 0);
    assert_ne!(digest.summary.files_changed, 0);
}

#[test]
fn test_branch_diff_reports_changes_against_head() {
    let (temp_dir, repo) = init_test_repo_with_commit();

    let sig = Signature::now("Test", "test@test.com").expect("Expected signature");
    let file_path = temp_dir.path().join("README.md");
    fs::write(&file_path, "# feature\n").expect("Expected README write");

    let mut index = repo.index().expect("Expected repo index");
    index
        .add_path(std::path::Path::new("README.md"))
        .expect("Expected index add");
    index.write().expect("Expected index write");
    let tree_id = index.write_tree().expect("Expected tree");
    let tree = repo.find_tree(tree_id).expect("Expected tree");
    let head = repo
        .head()
        .expect("Expected head")
        .peel_to_commit()
        .expect("Expected commit");

    repo.branch("feature", &head, false)
        .expect("Expected branch");
    repo.set_head("refs/heads/feature")
        .expect("Expected set head");
    repo.checkout_head(None).expect("Expected checkout");
    repo.commit(Some("HEAD"), &sig, &sig, "Feature commit", &tree, &[&head])
        .expect("Expected commit");

    repo.set_head("refs/heads/master")
        .expect("Expected set head");
    repo.checkout_head(None).expect("Expected checkout");

    let generator = Generator::new(&repo);
    let diff = generator
        .branch_diff("feature")
        .expect("Expected branch diff");
    assert!(!diff.is_empty());
}
