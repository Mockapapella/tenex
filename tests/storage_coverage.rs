//! Coverage tests for storage behavior reached through the public API.
#![expect(
    clippy::expect_used,
    reason = "coverage tests assert fixture setup directly"
)]

use chrono::Duration;
use std::path::PathBuf;
use std::sync::Mutex;
use tenex::agent::{Agent, AgentRuntime, ChildConfig, Status, Storage, WorkspaceKind};
use uuid::Uuid;

static CWD_LOCK: Mutex<()> = Mutex::new(());

fn init_tracing_for_coverage() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(std::io::sink)
        .try_init();
}

#[cfg(unix)]
#[test]
fn test_storage_load_from_resolves_symlinked_state_paths() {
    use std::os::unix::fs::symlink;

    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let real_path = temp_dir.path().join("real-state.json");
    let mut storage = Storage::new();
    storage.save_to(&real_path).expect("save real state");

    let relative_link = temp_dir.path().join("relative-state.json");
    symlink("real-state.json", &relative_link).expect("create relative symlink");
    Storage::load_from(&relative_link).expect("load through relative symlink");

    let absolute_link = temp_dir.path().join("absolute-state.json");
    symlink(&real_path, &absolute_link).expect("create absolute symlink");
    Storage::load_from(&absolute_link).expect("load through absolute symlink");
}

#[test]
fn test_storage_ensure_instance_id_normalizes_public_state() {
    let mut storage = Storage::new();
    storage.instance_id = Some("DEADBEEF".to_string());
    assert_eq!(storage.ensure_instance_id(), "deadbeef");
    assert_eq!(storage.instance_id.as_deref(), Some("deadbeef"));

    storage.instance_id = Some("short".to_string());
    assert_eq!(storage.ensure_instance_id().len(), 8);

    storage.instance_id = Some("zzzzzzzz".to_string());
    assert_eq!(storage.ensure_instance_id().len(), 8);
}

#[test]
fn test_storage_public_tree_paths_cover_children_and_missing_ids() {
    let mut storage = Storage::new();
    let root = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp/root"),
    );
    let root_id = root.id;
    let child = Agent::new_child(
        "Agent 1".to_string(),
        "codex".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp/root"),
        ChildConfig {
            parent_id: root_id,
            mux_session: root.mux_session.clone(),
            window_index: 2,
            repo_root: Some(PathBuf::from("/tmp/root")),
        },
    );
    let child_id = child.id;

    storage.add(root);
    storage.add(child);
    assert!(storage.set_collapsed(root_id, false));
    assert!(!storage.set_collapsed(Uuid::new_v4(), true));

    assert!(storage.root_ancestor(Uuid::new_v4()).is_none());
    assert_eq!(
        storage.root_ancestor(child_id).map(|agent| agent.id),
        Some(root_id)
    );
    assert_eq!(storage.depth(child_id), 1);

    let visible = storage.visible_agents_with_info();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].agent.id, root_id);
    assert!(visible[0].has_children);
    assert_eq!(visible[0].child_count, 1);
    assert_eq!(visible[1].agent.id, child_id);
    assert_eq!(visible[1].depth, 1);

    assert!(storage.remove_with_descendants(Uuid::new_v4()).is_empty());
}

#[test]
fn test_storage_public_tree_paths_cover_expanded_leaf_info() {
    let mut storage = Storage::new();
    let mut leaf = Agent::new(
        "leaf".to_string(),
        "claude".to_string(),
        "tenex/leaf".to_string(),
        PathBuf::from("/tmp/leaf"),
    );
    leaf.collapsed = false;
    let leaf_id = leaf.id;
    storage.add(leaf);

    let visible = storage.visible_agents_with_info();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].agent.id, leaf_id);
    assert!(!visible[0].has_children);
    assert_eq!(visible[0].child_count, 0);
}

#[test]
fn test_storage_public_tree_paths_cover_collapsed_missing_and_broken_parent_cases() {
    let mut storage = Storage::new();
    let mut root = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp/root"),
    );
    root.collapsed = true;
    let root_id = root.id;
    let child = Agent::new_child(
        "Agent 1".to_string(),
        "codex".to_string(),
        "tenex/root".to_string(),
        PathBuf::from("/tmp/root"),
        ChildConfig {
            parent_id: root_id,
            mux_session: root.mux_session.clone(),
            window_index: 2,
            repo_root: Some(PathBuf::from("/tmp/root")),
        },
    );
    let mut orphan = Agent::new_child(
        "orphan".to_string(),
        "codex".to_string(),
        "tenex/orphan".to_string(),
        PathBuf::from("/tmp/orphan"),
        ChildConfig {
            parent_id: Uuid::new_v4(),
            mux_session: "missing".to_string(),
            window_index: 3,
            repo_root: None,
        },
    );
    orphan.collapsed = false;
    let orphan_id = orphan.id;
    let leaf = Agent::new(
        "leaf".to_string(),
        "claude".to_string(),
        "tenex/leaf".to_string(),
        PathBuf::from("/tmp/leaf"),
    );

    storage.add(root);
    storage.add(child);
    storage.add(orphan);
    storage.add(leaf);

    assert_eq!(storage.depth(Uuid::new_v4()), 0);
    assert_eq!(storage.depth(orphan_id), 1);
    assert_eq!(
        storage.root_ancestor(orphan_id).map(|agent| agent.id),
        Some(orphan_id)
    );
    assert_eq!(storage.visible_agents_with_info().len(), 2);
}

#[test]
fn test_storage_load_at_returns_empty_when_state_and_backup_are_missing() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("missing-state.json");

    let storage = Storage::load_at(&state_path).expect("load missing state");
    assert!(storage.is_empty());
}

#[test]
fn test_storage_load_at_recovers_missing_state_from_backup() {
    init_tracing_for_coverage();

    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");
    let backup_path = temp_dir.path().join("state.json.bak");

    let mut backup = Storage::new();
    backup.save_to(&backup_path).expect("write backup state");

    let storage = Storage::load_at(&state_path).expect("load backup");
    assert!(storage.is_empty());
    assert!(state_path.exists());
    assert!(!backup_path.exists());
}

#[test]
fn test_storage_load_at_reports_corrupt_state_without_backup() {
    init_tracing_for_coverage();

    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");
    std::fs::write(&state_path, "not json").expect("write corrupt state");

    let err = Storage::load_at(&state_path).expect_err("corrupt state should fail");
    assert!(format!("{err:#}").contains("Failed to parse state"));
}

#[test]
fn test_storage_save_to_merges_public_agent_field_updates() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");

    let mut initial = Storage::new();
    let agent = Agent::new(
        "initial".to_string(),
        "claude".to_string(),
        "tenex/initial".to_string(),
        temp_dir.path().join("initial"),
    );
    let agent_id = agent.id;
    initial.add(agent);
    initial.save_to(&state_path).expect("save initial state");

    let mut client = Storage::load_from(&state_path).expect("load client");
    client.version = 2;
    client.instance_id = Some("deadbeef".to_string());
    client.mux_socket = Some("mux-socket".to_string());

    let agent = client.get_mut(agent_id).expect("missing agent");
    let created_at = agent.created_at;
    let updated_at = agent.updated_at;
    agent.title = "updated".to_string();
    agent.program = "codex".to_string();
    agent.conversation_id = Some("conversation".to_string());
    agent.status = Status::Running;
    agent.branch = "tenex/updated".to_string();
    agent.worktree_path = temp_dir.path().join("updated-worktree");
    agent.repo_root = Some(temp_dir.path().join("updated-repo"));
    agent.workspace_kind = WorkspaceKind::PlainDir;
    agent.runtime = AgentRuntime::Docker;
    agent.runtime_scope = "scope".to_string();
    agent.mux_session = "updated-session".to_string();
    agent.created_at = created_at + Duration::seconds(1);
    agent.updated_at = updated_at + Duration::seconds(2);
    agent.parent_id = Some(Uuid::new_v4());
    agent.window_index = Some(7);
    agent.is_terminal = true;

    client.save_to(&state_path).expect("save merged state");

    let merged = Storage::load_from(&state_path).expect("load merged state");
    assert_eq!(merged.version, 2);
    assert_eq!(merged.instance_id.as_deref(), Some("deadbeef"));
    assert_eq!(merged.mux_socket.as_deref(), Some("mux-socket"));

    let merged_agent = merged.get(agent_id).expect("missing merged agent");
    assert_eq!(merged_agent.title, "updated");
    assert_eq!(merged_agent.program, "codex");
    assert_eq!(
        merged_agent.conversation_id.as_deref(),
        Some("conversation")
    );
    assert_eq!(merged_agent.status, Status::Running);
    assert_eq!(merged_agent.branch, "tenex/updated");
    assert_eq!(
        merged_agent.worktree_path,
        temp_dir.path().join("updated-worktree")
    );
    assert_eq!(
        merged_agent.repo_root.as_deref(),
        Some(temp_dir.path().join("updated-repo").as_path())
    );
    assert_eq!(merged_agent.workspace_kind, WorkspaceKind::PlainDir);
    assert_eq!(merged_agent.runtime, AgentRuntime::Docker);
    assert_eq!(merged_agent.runtime_scope, "scope");
    assert_eq!(merged_agent.mux_session, "updated-session");
    assert_eq!(merged_agent.created_at, created_at + Duration::seconds(1));
    assert_eq!(merged_agent.updated_at, updated_at + Duration::seconds(2));
    assert!(merged_agent.parent_id.is_some());
    assert_eq!(merged_agent.window_index, Some(7));
    assert!(merged_agent.is_terminal);
}

#[test]
fn test_storage_save_to_merges_disk_deleted_and_shared_new_agents() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");

    let mut initial = Storage::new();
    let original = Agent::new(
        "original".to_string(),
        "claude".to_string(),
        "tenex/original".to_string(),
        temp_dir.path().join("original"),
    );
    let original_id = original.id;
    initial.add(original);
    initial.save_to(&state_path).expect("save initial state");

    let mut client = Storage::load_from(&state_path).expect("load client");
    client.get_mut(original_id).expect("missing original").title = "client update".to_string();

    let shared = Agent::new(
        "shared".to_string(),
        "codex".to_string(),
        "tenex/shared".to_string(),
        temp_dir.path().join("shared"),
    );
    let shared_id = shared.id;

    let mut disk = Storage::load_from(&state_path).expect("load disk");
    assert!(disk.remove(original_id).is_some());
    disk.add(shared.clone());
    disk.save_to(&state_path).expect("save disk state");

    client.add(shared);
    client.save_to(&state_path).expect("save merged state");

    let merged = Storage::load_from(&state_path).expect("load merged state");
    assert!(merged.get(original_id).is_none());
    let shared_count = merged.iter().filter(|agent| agent.id == shared_id).count();
    assert_eq!(shared_count, 1);
}

#[test]
fn test_storage_backfills_workspace_kinds_child_titles_and_repo_roots() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let plain_dir = temp_dir.path().join("plain");
    std::fs::create_dir(&plain_dir).expect("create plain dir");
    let git_dir = temp_dir.path().join("git");
    git2::Repository::init(&git_dir).expect("create git repo");
    let missing_dir = temp_dir.path().join("missing");

    let mut storage = Storage::new();
    let mut already_plain = Agent::new(
        "plain".to_string(),
        "claude".to_string(),
        "tenex/plain".to_string(),
        plain_dir.clone(),
    );
    already_plain.workspace_kind = WorkspaceKind::PlainDir;
    let git_agent = Agent::new(
        "git".to_string(),
        "claude".to_string(),
        "tenex/git".to_string(),
        git_dir,
    );
    let missing_agent = Agent::new(
        "missing".to_string(),
        "claude".to_string(),
        "tenex/missing".to_string(),
        missing_dir.clone(),
    );
    let plain_git_default = Agent::new(
        "default".to_string(),
        "claude".to_string(),
        "tenex/default".to_string(),
        plain_dir,
    );
    let mut root = Agent::new(
        "root".to_string(),
        "claude".to_string(),
        "tenex/root".to_string(),
        missing_dir,
    );
    root.repo_root = Some(PathBuf::from("/already-set"));
    let root_id = root.id;
    let mut agent_child = Agent::new_child(
        "Agent 2".to_string(),
        "codex".to_string(),
        "tenex/root".to_string(),
        root.worktree_path.clone(),
        ChildConfig {
            parent_id: root_id,
            mux_session: root.mux_session.clone(),
            window_index: 2,
            repo_root: None,
        },
    );
    agent_child.title = format!("Agent 2 ({})", agent_child.short_id());
    let mut planner_child = agent_child.clone();
    planner_child.id = Uuid::new_v4();
    planner_child.title = format!("Planner 1 ({})", planner_child.short_id());
    let mut reviewer_child = agent_child.clone();
    reviewer_child.id = Uuid::new_v4();
    reviewer_child.title = format!("Reviewer 1 ({})", reviewer_child.short_id());
    let mut custom_child = agent_child.clone();
    custom_child.id = Uuid::new_v4();
    custom_child.title = format!("Custom ({})", custom_child.short_id());
    let mut child_without_suffix = agent_child.clone();
    child_without_suffix.id = Uuid::new_v4();
    child_without_suffix.title = "Agent 3".to_string();

    storage.add(already_plain);
    storage.add(git_agent);
    storage.add(missing_agent);
    storage.add(plain_git_default);
    storage.add(root);
    storage.add(agent_child);
    storage.add(planner_child);
    storage.add(reviewer_child);
    storage.add(custom_child);
    storage.add(child_without_suffix);

    assert!(storage.backfill_workspace_kinds());
    assert!(storage.backfill_child_titles());
    assert!(storage.backfill_repo_roots());
}

#[test]
fn test_storage_backfill_repo_roots_uses_missing_worktree_path() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let missing = temp_dir.path().join("missing-worktree");
    let mut storage = Storage::new();
    let mut agent = Agent::new(
        "missing".to_string(),
        "claude".to_string(),
        "tenex/missing".to_string(),
        missing.clone(),
    );
    agent.repo_root = None;
    let agent_id = agent.id;
    storage.add(agent);

    assert!(storage.backfill_repo_roots());
    let loaded = storage.get(agent_id).expect("missing agent");
    assert_eq!(loaded.repo_root.as_deref(), Some(missing.as_path()));
}

#[test]
fn test_storage_backfill_conversation_ids_covers_existing_and_blank_values() {
    let mut storage = Storage::new();

    let mut existing = Agent::new(
        "existing".to_string(),
        "claude".to_string(),
        "tenex/existing".to_string(),
        PathBuf::from("/tmp/existing"),
    );
    existing.conversation_id = Some("known-conversation".to_string());

    let mut blank = Agent::new(
        "blank".to_string(),
        "claude".to_string(),
        "tenex/blank".to_string(),
        PathBuf::from("/tmp/blank"),
    );
    let blank_id = blank.id;
    blank.conversation_id = Some("   ".to_string());

    let other = Agent::new(
        "other".to_string(),
        "not-claude".to_string(),
        "tenex/other".to_string(),
        PathBuf::from("/tmp/other"),
    );
    let mut terminal = Agent::new(
        "terminal".to_string(),
        "bash".to_string(),
        "tenex/terminal".to_string(),
        PathBuf::from("/tmp/terminal"),
    );
    terminal.is_terminal = true;
    let terminal_id = terminal.id;

    storage.add(existing);
    storage.add(blank);
    storage.add(other);
    storage.add(terminal);

    assert!(storage.backfill_conversation_ids());
    let expected_blank = blank_id.to_string();
    assert_eq!(
        storage
            .get(blank_id)
            .and_then(|agent| agent.conversation_id.as_deref()),
        Some(expected_blank.as_str())
    );
    assert!(
        storage
            .get(terminal_id)
            .and_then(|agent| agent.conversation_id.as_deref())
            .is_none()
    );
}

#[test]
fn test_storage_load_at_recovers_invalid_state_from_backup() {
    init_tracing_for_coverage();

    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");
    std::fs::write(&state_path, "{").expect("write invalid state");

    let mut backup = Storage::new();
    backup
        .save_to(&state_path.with_file_name("state.json.bak"))
        .expect("write backup");

    Storage::load_at(&state_path).expect("load from backup");
}

#[test]
fn test_storage_save_to_uses_backup_when_existing_state_is_invalid() {
    init_tracing_for_coverage();

    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");
    let backup_path = state_path.with_file_name("state.json.bak");

    let mut backup = Storage::new();
    backup.save_to(&backup_path).expect("write backup state");
    std::fs::write(&state_path, "{").expect("write invalid state");

    let mut storage = Storage::new();
    storage.add(Agent::new(
        "saved".to_string(),
        "claude".to_string(),
        "tenex/saved".to_string(),
        temp_dir.path().join("worktree"),
    ));
    storage.save_to(&state_path).expect("save using backup");

    let loaded = Storage::load_from(&state_path).expect("load saved state");
    assert_eq!(loaded.len(), 1);
}

#[cfg(coverage)]
#[test]
fn test_storage_save_to_forced_error_runs_in_non_test_build() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");
    let mut storage = Storage::new();

    let err = Storage::with_forced_save_error_after_successes_for_tests(0, || {
        storage.save_to(&state_path)
    })
    .expect_err("forced save error should fail");

    assert!(err.to_string().contains("forced storage save error"));

    let delayed_state_path = temp_dir.path().join("delayed-state.json");
    let mut delayed_storage = Storage::new();
    let delayed_err = Storage::with_forced_save_error_after_successes_for_tests(1, || {
        delayed_storage
            .save_to(&delayed_state_path)
            .expect("first delayed save succeeds");
        delayed_storage.save_to(&delayed_state_path)
    })
    .expect_err("second delayed save should fail");

    assert!(
        delayed_err
            .to_string()
            .contains("forced storage save error")
    );
}

#[cfg(coverage)]
#[test]
fn test_storage_private_save_boundary_runs_in_non_test_build() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");

    Storage::exercise_private_save_boundary_for_coverage(&state_path);
}

#[cfg(unix)]
#[test]
fn test_storage_save_to_reports_existing_lock_directory() {
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let state_path = temp_dir.path().join("state.json");
    std::fs::create_dir(state_path.with_file_name("state.json.lock"))
        .expect("create lock directory");

    let mut storage = Storage::new();
    let err = storage
        .save_to(&state_path)
        .expect_err("lock directory should reject save");

    assert!(err.to_string().contains("Failed to open state lock"));
}

#[test]
fn test_storage_save_to_path_without_parent_errors_cleanly() {
    struct CurrentDirGuard {
        original: std::path::PathBuf,
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }

    let _cwd_guard = CWD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let temp_dir = tempfile::TempDir::new().expect("create temp dir");
    let original = std::env::current_dir().expect("current dir");
    std::env::set_current_dir(temp_dir.path()).expect("set current dir");
    let _guard = CurrentDirGuard { original };

    let mut storage = Storage::new();
    storage.add(Agent::new(
        "agent".to_string(),
        "claude".to_string(),
        "tenex/agent".to_string(),
        temp_dir.path().join("worktree"),
    ));

    let err = storage
        .save_to(std::path::Path::new(""))
        .expect_err("empty path should fail");
    assert!(format!("{err:#}").contains("Failed to replace state file"));
}
