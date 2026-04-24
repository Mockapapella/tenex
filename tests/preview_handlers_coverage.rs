//! Integration coverage for preview handlers.
//!
//! This test intentionally exercises a small set of handler paths from an
//! integration test target so they are covered in non-test crate instantiations.

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};
use interprocess::local_socket::traits::ListenerExt as _;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use git2::{Repository, RepositoryInitOptions, Signature};
use tenex::agent::{Agent, Storage};
use tenex::app::{Actions, App, Settings, Tab};
use tenex::config::Config;

#[derive(Debug, Default)]
struct MockMuxConfig {
    observed_requests: Vec<String>,
}

fn make_mock_mux_socket(
    dir: &TempDir,
) -> Result<(String, interprocess::local_socket::Name<'static>)> {
    #[cfg(windows)]
    {
        use interprocess::local_socket::GenericNamespaced;
        use interprocess::local_socket::prelude::*;

        let display = format!("tenex-mux-preview-integration-{}", uuid::Uuid::new_v4());
        let name = display
            .clone()
            .to_ns_name::<GenericNamespaced>()
            .context("namespaced socket name")?
            .into_owned();
        return Ok((display, name));
    }

    #[cfg(not(windows))]
    {
        use interprocess::local_socket::GenericFilePath;
        use interprocess::local_socket::prelude::*;

        let socket_path = dir.path().join("mux.sock");
        let display = socket_path.to_string_lossy().into_owned();
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .context("filesystem socket name")?
            .into_owned();
        Ok((display, name))
    }
}

fn read_len_prefixed_json<R: Read>(reader: &mut R) -> std::io::Result<serde_json::Value> {
    let mut len_bytes = [0u8; 4];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    serde_json::from_slice(&buf)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

fn write_len_prefixed_json<W: Write>(
    writer: &mut W,
    value: &serde_json::Value,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(value)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let len = u32::try_from(payload.len())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "payload too large"))?;

    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

fn response_read_output(
    payload: &serde_json::Value,
    read_output_calls_by_target: &mut HashMap<String, usize>,
) -> serde_json::Value {
    let target = payload
        .get("target")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    if target != "stream-session" {
        return serde_json::json!({ "Err": { "message": "mock read_output failure" } });
    }

    let index = read_output_calls_by_target
        .entry(target.to_string())
        .or_insert(0);
    let max_bytes = payload
        .get("max_bytes")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let response = match *index {
        0 => {
            let checkpoint_b64 = general_purpose::STANDARD.encode(b"reset-checkpoint\n");
            serde_json::json!({
                "OutputReset": { "start": 0, "checkpoint_b64": checkpoint_b64 }
            })
        }
        1 => serde_json::json!({
            "OutputReset": { "start": 0, "checkpoint_b64": "" }
        }),
        2 => {
            let data_b64 = general_purpose::STANDARD.encode(b"hello\n");
            serde_json::json!({
                "OutputChunk": { "start": 0, "end": 10, "data_b64": data_b64 }
            })
        }
        3 => serde_json::json!({
            "OutputChunk": { "start": 10, "end": 10, "data_b64": "" }
        }),
        4 => {
            let len = usize::try_from(max_bytes).unwrap_or(0).min(1024);
            let data = vec![b'a'; len];
            let data_b64 = general_purpose::STANDARD.encode(&data);
            serde_json::json!({
                "OutputChunk": { "start": 10, "end": 20, "data_b64": data_b64 }
            })
        }
        _ => serde_json::json!({
            "OutputChunk": { "start": 0, "end": 10, "data_b64": "" }
        }),
    };

    *index = index.saturating_add(1);
    response
}

fn response_capture(payload: &serde_json::Value) -> serde_json::Value {
    let kind = payload.get("kind").unwrap_or(&serde_json::Value::Null);
    let text = match kind {
        serde_json::Value::String(name) if name == "Visible" => "capture-visible\n",
        serde_json::Value::String(name) if name == "FullHistory" => "capture-full-history\n",
        serde_json::Value::Object(map) if map.contains_key("History") => "capture-history\n",
        _ => "capture-unknown\n",
    };
    serde_json::json!({ "Text": { "text": text } })
}

fn mux_response_for_request(
    request: &serde_json::Value,
    config: &Arc<Mutex<MockMuxConfig>>,
    read_output_calls_by_target: &mut HashMap<String, usize>,
) -> serde_json::Value {
    let (kind, payload) = request
        .as_object()
        .and_then(|obj| obj.iter().next())
        .map_or(("", &serde_json::Value::Null), |(k, v)| (k.as_str(), v));

    {
        let mut guard = config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.observed_requests.push(kind.to_string());
    }

    match kind {
        "Ping" => serde_json::json!({ "Pong": { "version": "test" } }),
        "SessionExists" => {
            let name = payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            serde_json::json!({ "Bool": { "value": name != "missing-session" } })
        }
        "ReadOutput" => response_read_output(payload, read_output_calls_by_target),
        "Capture" => response_capture(payload),
        "PaneSize" => serde_json::json!({ "Size": { "cols": 80, "rows": 24 } }),
        "CursorPosition" => serde_json::json!({ "Position": { "x": 0, "y": 0, "hidden": false } }),
        _ => serde_json::json!({ "Err": { "message": "mock: unsupported request" } }),
    }
}

fn spawn_mock_mux_server(
    name: interprocess::local_socket::Name<'static>,
    config: Arc<Mutex<MockMuxConfig>>,
) -> Result<std::thread::JoinHandle<()>> {
    use interprocess::local_socket::ListenerOptions;

    let listener = ListenerOptions::new()
        .name(name)
        .create_sync()
        .context("create mock mux listener")?;

    Ok(std::thread::spawn(move || {
        let mut read_output_calls_by_target: HashMap<String, usize> = HashMap::new();

        for mut stream in listener.incoming().flatten() {
            while let Ok(request) = read_len_prefixed_json(&mut stream) {
                let response =
                    mux_response_for_request(&request, &config, &mut read_output_calls_by_target);
                if write_len_prefixed_json(&mut stream, &response).is_err() {
                    break;
                }
            }
        }
    }))
}

fn create_test_app() -> App {
    App::new(
        Config::default(),
        Storage::default(),
        Settings::default(),
        false,
    )
}

fn init_repo(repo_dir: &TempDir) -> Result<Repository> {
    let mut init_opts = RepositoryInitOptions::new();
    init_opts.initial_head("master");
    let repo = Repository::init_opts(repo_dir.path(), &init_opts).context("init repository")?;
    repo.set_head("refs/heads/master").context("set HEAD")?;
    Ok(repo)
}

fn commit_file(
    repo: &Repository,
    sig: &Signature<'_>,
    file_path: &Path,
    content: &str,
    message: &str,
) -> Result<()> {
    std::fs::write(file_path, content).context("write file")?;
    let mut index = repo.index().context("open index")?;
    index.add_path(Path::new("file.txt")).context("add file")?;
    index.write().context("write index")?;

    let tree_id = index.write_tree().context("write tree")?;
    let tree = repo.find_tree(tree_id).context("find tree")?;

    if let Ok(head) = repo.head()
        && let Some(oid) = head.target()
        && let Ok(parent) = repo.find_commit(oid)
    {
        repo.commit(Some("HEAD"), sig, sig, message, &tree, &[&parent])
            .context("commit")?;
        return Ok(());
    }

    repo.commit(Some("HEAD"), sig, sig, message, &tree, &[])
        .context("commit")?;
    Ok(())
}

fn observed_requests(config: &Arc<Mutex<MockMuxConfig>>) -> Vec<String> {
    config
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .observed_requests
        .clone()
}

fn exercise_preview_no_agent_and_missing_session(
    handler: Actions,
    socket_dir: &TempDir,
) -> Result<()> {
    let mut app = create_test_app();
    handler.update_preview(&mut app)?;
    assert!(app.data.ui.preview_content.contains("No agent selected"));
    app.data.ui.preview_dimensions = Some((80, 10));

    let mut missing_agent = Agent::new(
        "missing".to_string(),
        "claude".to_string(),
        "main".to_string(),
        socket_dir.path().to_path_buf(),
    );
    missing_agent.mux_session = "missing-session".to_string();
    app.data.storage.add(missing_agent);

    handler.update_preview(&mut app)?;
    assert!(app.data.ui.preview_content.contains("Session not running"));
    Ok(())
}

fn exercise_preview_capture_respects_follow_mode(
    handler: Actions,
    socket_dir: &TempDir,
) -> Result<()> {
    let mut app = create_test_app();
    app.data.ui.preview_dimensions = Some((80, 10));

    let mut preview_agent = Agent::new(
        "preview".to_string(),
        "claude".to_string(),
        "main".to_string(),
        socket_dir.path().to_path_buf(),
    );
    preview_agent.mux_session = "ok-session".to_string();
    app.data.storage.add(preview_agent);

    app.data.ui.preview_follow = true;
    handler.update_preview(&mut app)?;
    assert_eq!(app.data.ui.preview_content, "capture-history\n");

    app.data.ui.preview_follow = false;
    handler.update_preview(&mut app)?;
    assert_eq!(app.data.ui.preview_content, "capture-full-history\n");

    Ok(())
}

fn exercise_preview_streams_output_when_following(
    handler: Actions,
    socket_dir: &TempDir,
) -> Result<()> {
    let mut app = create_test_app();
    app.data.ui.preview_dimensions = Some((80, 10));
    app.data.ui.preview_follow = true;

    let mut stream_agent = Agent::new(
        "stream".to_string(),
        "claude".to_string(),
        "main".to_string(),
        socket_dir.path().to_path_buf(),
    );
    stream_agent.mux_session = "stream-session".to_string();
    app.data.storage.add(stream_agent);

    handler.update_preview(&mut app)?;
    assert!(app.data.ui.preview_content.contains("reset-checkpoint"));

    handler.update_preview(&mut app)?;
    handler.update_preview(&mut app)?;
    assert!(app.data.ui.preview_content.contains("hello"));

    app.data.ui.preview_dimensions = Some((81, 10));
    handler.update_preview(&mut app)?;
    handler.update_preview(&mut app)?;
    Ok(())
}

fn exercise_diff_and_commits_track_unseen_changes(handler: Actions) -> Result<()> {
    let repo_dir = TempDir::new().context("create repo dir")?;
    let repo = init_repo(&repo_dir)?;
    let sig = Signature::now("Test", "test@test.com").context("create signature")?;
    let file_path = repo_dir.path().join("file.txt");

    commit_file(&repo, &sig, &file_path, "initial\n", "Initial\n\nBody")?;
    let head = repo.head().context("read HEAD")?;
    let first = repo
        .find_commit(head.target().context("missing head target")?)
        .context("find head commit")?;
    repo.branch("tenex/test", &first, false)
        .context("create branch")?;
    repo.set_head("refs/heads/tenex/test")
        .context("set branch HEAD")?;

    let long_body = (0..42usize)
        .map(|idx| {
            if idx == 1 {
                String::new()
            } else {
                format!("line {idx}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    commit_file(
        &repo,
        &sig,
        &file_path,
        "changed\n",
        &format!("Change\n\n{long_body}"),
    )?;
    std::fs::write(&file_path, "uncommitted change\n").context("write worktree change")?;

    let mut app = create_test_app();
    app.data.storage.add(Agent::new(
        "git".to_string(),
        "claude".to_string(),
        "tenex/test".to_string(),
        repo_dir.path().to_path_buf(),
    ));

    app.data.active_tab = Tab::Preview;
    handler.update_diff(&mut app)?;
    assert!(app.data.ui.diff_has_unseen_changes);

    app.data.active_tab = Tab::Diff;
    handler.update_diff(&mut app)?;
    assert!(!app.data.ui.diff_has_unseen_changes);

    app.data.active_tab = Tab::Preview;
    handler.update_commits(&mut app)?;
    assert!(app.data.ui.commits_content.contains("Branch: tenex/test"));
    assert!(app.data.ui.commits_has_unseen_changes);

    app.data.active_tab = Tab::Commits;
    handler.update_commits(&mut app)?;
    assert!(!app.data.ui.commits_has_unseen_changes);

    handler.update_commits_digest(&mut app)?;
    handler.update_diff_digest(&mut app)?;
    Ok(())
}

fn exercise_diff_and_commits_report_missing_agent_worktree_and_repo(
    handler: Actions,
) -> Result<()> {
    let mut empty_app = create_test_app();
    handler.update_commits(&mut empty_app)?;
    assert!(
        empty_app
            .data
            .ui
            .commits_content
            .contains("No agent selected")
    );
    handler.update_diff(&mut empty_app)?;
    assert!(empty_app.data.ui.diff_content.contains("No agent selected"));
    handler.update_diff_digest(&mut empty_app)?;

    let repo_dir = TempDir::new().context("create repo dir")?;
    let missing_path = repo_dir.path().join("nope");
    let mut missing_worktree_app = create_test_app();
    missing_worktree_app.data.storage.add(Agent::new(
        "missing-worktree".to_string(),
        "claude".to_string(),
        "feature/missing".to_string(),
        missing_path,
    ));
    handler.update_commits(&mut missing_worktree_app)?;
    assert!(
        missing_worktree_app
            .data
            .ui
            .commits_content
            .contains("Worktree not found")
    );
    handler.update_diff(&mut missing_worktree_app)?;
    assert!(
        missing_worktree_app
            .data
            .ui
            .diff_content
            .contains("Worktree not found")
    );
    handler.update_diff_digest(&mut missing_worktree_app)?;
    handler.update_commits_digest(&mut missing_worktree_app)?;
    assert_eq!(missing_worktree_app.data.ui.commits_hash, 0);
    assert!(!missing_worktree_app.data.ui.commits_has_unseen_changes);

    let not_git_dir = TempDir::new().context("create temp dir")?;
    let mut not_git_app = create_test_app();
    not_git_app.data.storage.add(Agent::new(
        "not-git".to_string(),
        "claude".to_string(),
        "feature/not-git".to_string(),
        not_git_dir.path().to_path_buf(),
    ));
    handler.update_commits(&mut not_git_app)?;
    assert!(
        not_git_app
            .data
            .ui
            .commits_content
            .contains("Not a git repository")
    );
    handler.update_diff(&mut not_git_app)?;
    assert!(
        not_git_app
            .data
            .ui
            .diff_content
            .contains("Not a git repository")
    );
    handler.update_commits_digest(&mut not_git_app)?;
    assert_eq!(not_git_app.data.ui.commits_hash, 0);
    assert!(!not_git_app.data.ui.commits_has_unseen_changes);
    Ok(())
}

fn exercise_commits_report_no_commits_and_empty_repo(handler: Actions) -> Result<()> {
    let base_repo_dir = TempDir::new().context("create repo dir")?;
    let base_repo = init_repo(&base_repo_dir)?;
    let sig = Signature::now("Test", "test@test.com").context("create signature")?;
    let file_path = base_repo_dir.path().join("file.txt");
    commit_file(&base_repo, &sig, &file_path, "base\n", "Base")?;

    let mut empty_range_app = create_test_app();
    empty_range_app.data.storage.add(Agent::new(
        "empty-range".to_string(),
        "claude".to_string(),
        "master".to_string(),
        base_repo_dir.path().to_path_buf(),
    ));
    handler.update_commits(&mut empty_range_app)?;
    assert!(
        empty_range_app
            .data
            .ui
            .commits_content
            .contains("(No commits)")
    );
    handler.update_commits_digest(&mut empty_range_app)?;
    assert_eq!(empty_range_app.data.ui.commits_hash, 0);
    assert!(!empty_range_app.data.ui.commits_has_unseen_changes);

    let empty_repo_dir = TempDir::new().context("create repo dir")?;
    let empty_repo = init_repo(&empty_repo_dir)?;
    let _ = empty_repo;
    let mut empty_repo_app = create_test_app();
    empty_repo_app.data.storage.add(Agent::new(
        "empty-repo".to_string(),
        "claude".to_string(),
        "master".to_string(),
        empty_repo_dir.path().to_path_buf(),
    ));

    let Err(_) = handler.update_commits(&mut empty_repo_app) else {
        anyhow::bail!("update_commits unexpectedly succeeded for empty repository");
    };
    let Err(_) = handler.update_commits_digest(&mut empty_repo_app) else {
        anyhow::bail!("update_commits_digest unexpectedly succeeded for empty repository");
    };
    Ok(())
}

fn exercise_commits_truncates_long_history() -> Result<()> {
    let repo_dir = TempDir::new().context("create repo dir")?;
    let repo = init_repo(&repo_dir)?;
    let sig = Signature::now("Test", "test@test.com").context("create signature")?;
    let file_path = repo_dir.path().join("file.txt");

    commit_file(&repo, &sig, &file_path, "initial\n", "Initial")?;
    let head = repo.head().context("read HEAD")?;
    let base_commit = repo
        .find_commit(head.target().context("missing head target")?)
        .context("find base commit")?;
    repo.branch("tenex/truncated", &base_commit, false)
        .context("create branch")?;
    repo.set_head("refs/heads/tenex/truncated")
        .context("set branch HEAD")?;

    for idx in 0..201usize {
        commit_file(
            &repo,
            &sig,
            &file_path,
            &format!("commit {idx}\n"),
            &format!("Commit {idx}"),
        )?;
    }

    let handler = Actions::new();
    let mut app = create_test_app();
    app.data.storage.add(Agent::new(
        "truncated".to_string(),
        "claude".to_string(),
        "tenex/truncated".to_string(),
        repo_dir.path().to_path_buf(),
    ));
    handler.update_commits(&mut app)?;
    assert!(app.data.ui.commits_content.contains("(truncated)"));
    handler.update_commits_digest(&mut app)?;
    Ok(())
}

#[test]
fn test_preview_handlers_cover_integration_instantiations() -> Result<()> {
    let socket_dir = TempDir::new().context("create socket dir")?;
    let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir)?;
    let config = Arc::new(Mutex::new(MockMuxConfig::default()));
    let _server = spawn_mock_mux_server(socket_name, Arc::clone(&config))?;

    tenex::mux::set_socket_override(&socket_display)?;

    let handler = Actions::new();
    exercise_preview_no_agent_and_missing_session(handler, &socket_dir)?;
    exercise_preview_capture_respects_follow_mode(handler, &socket_dir)?;
    exercise_preview_streams_output_when_following(handler, &socket_dir)?;

    let observed = observed_requests(&config);
    assert!(observed.iter().any(|req| req == "SessionExists"));
    assert!(observed.iter().any(|req| req == "ReadOutput"));
    assert!(observed.iter().any(|req| req == "Capture"));

    exercise_diff_and_commits_track_unseen_changes(handler)?;
    exercise_diff_and_commits_report_missing_agent_worktree_and_repo(handler)?;
    exercise_commits_report_no_commits_and_empty_repo(handler)?;
    exercise_commits_truncates_long_history()?;
    Ok(())
}
