//! Integration coverage for `tenex::tui::test_support`.

#![cfg(feature = "test-support")]

use tenex::agent::{Agent, Storage};
use tenex::app::Settings;
use tenex::tui::test_support;
use tenex::{App, Config};

struct FailingWriter;

impl std::io::Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("boom"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("boom"))
    }
}

fn test_config(prefix: &str) -> Config {
    Config {
        worktree_dir: std::env::temp_dir().join(prefix),
        branch_prefix: format!("{prefix}/"),
        ..Config::default()
    }
}

#[test]
fn test_flush_pending_clipboard_test_support_exercises_all_outcomes() {
    let mut app = App::new(
        test_config("tenex-test-support-clipboard"),
        Storage::new(),
        Settings::default(),
        false,
    );

    let mut out = Vec::new();
    test_support::flush_pending_clipboard(&mut out, &mut app);
    assert!(out.is_empty());
    assert!(app.data.ui.pending_clipboard.is_none());

    app.data.ui.pending_clipboard = Some(String::new());
    test_support::flush_pending_clipboard(&mut out, &mut app);
    assert!(out.is_empty());
    assert!(app.data.ui.pending_clipboard.is_none());

    app.data.ui.pending_clipboard = Some("x".repeat(200_000));
    test_support::flush_pending_clipboard(&mut out, &mut app);
    assert!(out.is_empty());
    assert!(
        app.data
            .ui
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("Selection too large to copy")
    );

    app.data.ui.pending_clipboard = Some("hello".to_string());
    let mut out = Vec::new();
    test_support::flush_pending_clipboard(&mut out, &mut app);
    let output = String::from_utf8(out).expect("Expected utf8 OSC52 output");
    assert!(output.starts_with("\u{1b}]52;c;"));
    assert!(output.ends_with('\u{7}'));
    assert_eq!(app.data.ui.status_message.as_deref(), Some("Copied 1 line"));

    app.data.ui.pending_clipboard = Some("a\nb".to_string());
    let mut out = Vec::new();
    test_support::flush_pending_clipboard(&mut out, &mut app);
    assert_eq!(app.data.ui.status_message.as_deref(), Some("Copied 2 lines"));

    app.data.ui.pending_clipboard = Some("hello".to_string());
    let mut failing = FailingWriter;
    test_support::flush_pending_clipboard(&mut failing, &mut app);
    assert!(
        app.data
            .ui
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("Copy failed")
    );
}

fn agent_in_repo(title: &str, repo_root: &std::path::Path) -> Agent {
    let mut agent = Agent::new(
        title.to_string(),
        "echo".to_string(),
        format!("branch/{title}"),
        repo_root.join("worktree"),
    );
    agent.repo_root = Some(repo_root.to_path_buf());
    agent
}

#[test]
fn test_state_file_tracker_test_support_covers_interval_and_stamp_checks() {
    use tempfile::TempDir;

    let dir = TempDir::new().expect("Expected temp dir");
    let state_path = dir.path().join("state.json");

    let mut storage = Storage::with_path(state_path.clone());
    storage.save_to(&state_path).expect("save initial state");

    let config = test_config("tenex-test-support-state-tracker");
    let mut app = App::new(config, storage, Settings::default(), false);
    let mut tracker = test_support::StateFileTracker::new(&app);
    assert_eq!(format!("{tracker:?}"), "StateFileTracker");

    // Interval has not elapsed yet.
    assert!(!tracker.maybe_reload_state(&mut app));

    // Interval has elapsed, but stamp is unchanged.
    tracker.force_due();
    assert!(!tracker.maybe_reload_state(&mut app));
}

#[test]
fn test_state_file_tracker_test_support_returns_false_when_state_file_missing() {
    use tempfile::TempDir;

    let dir = TempDir::new().expect("Expected temp dir");
    let state_path = dir.path().join("missing-state.json");

    let mut storage = Storage::with_path(state_path.clone());
    storage.save_to(&state_path).expect("save initial state");

    let config = test_config("tenex-test-support-state-missing");
    let mut app = App::new(config, storage, Settings::default(), false);
    let mut tracker = test_support::StateFileTracker::new(&app);

    std::fs::remove_file(&state_path).expect("remove state file");
    tracker.force_due();
    assert!(!tracker.maybe_reload_state(&mut app));
}

#[test]
fn test_state_file_tracker_test_support_returns_false_when_state_file_is_corrupt() {
    use tempfile::TempDir;

    let dir = TempDir::new().expect("Expected temp dir");
    let state_path = dir.path().join("corrupt-state.json");

    let mut storage = Storage::with_path(state_path.clone());
    storage.save_to(&state_path).expect("save initial state");

    let config = test_config("tenex-test-support-state-corrupt");
    let mut app = App::new(config, storage, Settings::default(), false);
    let mut tracker = test_support::StateFileTracker::new(&app);

    std::fs::write(&state_path, "{ not valid json").expect("write corrupt state");
    tracker.force_due();
    assert!(!tracker.maybe_reload_state(&mut app));
}

#[test]
fn test_state_file_tracker_test_support_reload_restores_sidebar_selection() {
    use tempfile::TempDir;

    let dir = TempDir::new().expect("Expected temp dir");
    let state_path = dir.path().join("state.json");
    let repo_a = dir.path().join("repo-a");
    let repo_b = dir.path().join("repo-b");

    std::fs::create_dir_all(&repo_a).expect("create repo-a");
    std::fs::create_dir_all(&repo_b).expect("create repo-b");

    let mut storage = Storage::with_path(state_path.clone());
    let agent_a = agent_in_repo("agent-a", &repo_a);
    let agent_b = agent_in_repo("agent-b", &repo_b);
    let selected_agent_id = agent_b.id;
    storage.add(agent_a);
    storage.add(agent_b);
    storage.save_to(&state_path).expect("save initial state");

    let config = test_config("tenex-test-support-state-reload");
    let mut app = App::new(config, storage, Settings::default(), false);

    test_support::restore_sidebar_selection(
        &mut app,
        Some(test_support::SelectedSidebarKey::Agent(selected_agent_id)),
    );
    assert_eq!(
        test_support::selected_sidebar_key(&app),
        Some(test_support::SelectedSidebarKey::Agent(selected_agent_id))
    );

    let mut tracker = test_support::StateFileTracker::new(&app);

    let mut disk = Storage::load_from(&state_path).expect("load state from disk");
    let agent_c = agent_in_repo("agent-c", &repo_a);
    let added_agent_id = agent_c.id;
    disk.add(agent_c);
    disk.save_to(&state_path).expect("save updated state");

    tracker.force_due();
    assert!(tracker.maybe_reload_state(&mut app));
    assert!(app.data.storage.get(added_agent_id).is_some());
    assert_eq!(
        app.data.storage.state_path.as_deref(),
        Some(state_path.as_path())
    );
    assert_eq!(
        test_support::selected_sidebar_key(&app),
        Some(test_support::SelectedSidebarKey::Agent(selected_agent_id))
    );

    test_support::restore_sidebar_selection(
        &mut app,
        Some(test_support::SelectedSidebarKey::Project(repo_b.clone())),
    );
    assert_eq!(
        test_support::selected_sidebar_key(&app),
        Some(test_support::SelectedSidebarKey::Project(repo_b.clone()))
    );

    let mut disk = Storage::load_from(&state_path).expect("load state from disk");
    disk.add(agent_in_repo("agent-d", &repo_b));
    disk.save_to(&state_path).expect("save updated state");

    tracker.force_due();
    assert!(tracker.maybe_reload_state(&mut app));
    assert_eq!(
        test_support::selected_sidebar_key(&app),
        Some(test_support::SelectedSidebarKey::Project(repo_b))
    );
}
