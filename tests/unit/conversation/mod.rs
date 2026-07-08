use anyhow::Result;
use chrono::Local;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;
use tenex::conversation::test_support as conversation_support;
use tenex::conversation::{
    AgentCli, build_resume_argv, build_spawn_argv, detect_agent_cli, try_detect_codex_session_id,
};
use tenex::test_support::lock_env_test_environment;

fn codex_session_meta_line(id: &str, cwd: &Path) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": id,
                "cwd": cwd.to_string_lossy().into_owned(),
            }
        })
    )
}

fn set_modified(path: &Path, modified: SystemTime) -> std::io::Result<()> {
    let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    file.set_times(std::fs::FileTimes::new().set_modified(modified))
}

fn write_session(path: &Path, id: &str, workdir: &Path, modified: SystemTime) -> Result<()> {
    fs::write(path, codex_session_meta_line(id, workdir))?;
    set_modified(path, modified)?;
    Ok(())
}

#[test]
fn test_detect_agent_cli() {
    assert_eq!(detect_agent_cli("claude"), AgentCli::Claude);
    assert_eq!(detect_agent_cli("codex"), AgentCli::Codex);
    assert_eq!(detect_agent_cli("sh -c 'echo hi'"), AgentCli::Other);
    assert_eq!(detect_agent_cli("   "), AgentCli::Other);
    assert_eq!(detect_agent_cli("sh -c 'unterminated"), AgentCli::Other);
}

#[test]
fn test_detect_agent_cli_uses_executable_basename() {
    assert_eq!(detect_agent_cli("/usr/bin/echo hello"), AgentCli::Other);
}

#[test]
fn test_build_spawn_argv_claude_adds_session_id() -> Result<()> {
    let argv = build_spawn_argv("claude --debug", Some("hello"), Some("abc"))?;
    assert_eq!(
        argv,
        vec!["claude", "--debug", "--session-id", "abc", "hello"]
    );
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_without_session_id_only_appends_prompt() -> Result<()> {
    let argv = build_spawn_argv("claude --debug", Some("hello"), None)?;
    assert_eq!(argv, vec!["claude", "--debug", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_preserves_resume_flags_without_prompt() -> Result<()> {
    let cases: &[(&str, &[&str])] = &[
        (
            "claude --session-id existing",
            &["claude", "--session-id", "existing"],
        ),
        (
            "claude --session-id=existing",
            &["claude", "--session-id=existing"],
        ),
        (
            "claude --resume existing",
            &["claude", "--resume", "existing"],
        ),
        ("claude -r existing", &["claude", "-r", "existing"]),
        (
            "claude --continue existing",
            &["claude", "--continue", "existing"],
        ),
        ("claude -c existing", &["claude", "-c", "existing"]),
    ];

    for (program, expected) in cases {
        let argv = build_spawn_argv(program, None, Some("abc"))?;
        let actual = argv.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(actual, *expected);
    }
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_does_not_duplicate_session_id() -> Result<()> {
    let argv = build_spawn_argv("claude --session-id existing", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["claude", "--session-id", "existing", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_does_not_duplicate_session_id_with_equals() -> Result<()> {
    let argv = build_spawn_argv("claude --session-id=existing", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["claude", "--session-id=existing", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_does_not_inject_when_resume_present() -> Result<()> {
    let argv = build_spawn_argv("claude --resume existing", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["claude", "--resume", "existing", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_does_not_inject_when_short_resume_present() -> Result<()> {
    let argv = build_spawn_argv("claude -r existing", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["claude", "-r", "existing", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_does_not_inject_when_continue_present() -> Result<()> {
    let argv = build_spawn_argv("claude --continue existing", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["claude", "--continue", "existing", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_claude_does_not_inject_when_short_continue_present() -> Result<()> {
    let argv = build_spawn_argv("claude -c existing", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["claude", "-c", "existing", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_non_claude_does_not_inject_session_id() -> Result<()> {
    let argv = build_spawn_argv("codex", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["codex", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_other_command_does_not_inject_session_id() -> Result<()> {
    let argv = build_spawn_argv("sh -c 'echo hi'", Some("hello"), Some("abc"))?;
    assert_eq!(argv, vec!["sh", "-c", "echo hi", "hello"]);
    Ok(())
}

#[test]
fn test_build_spawn_argv_reports_parse_errors() {
    assert!(build_spawn_argv("sh -c 'unterminated", None, None).is_err());
}

#[test]
fn test_build_resume_argv_codex() -> Result<()> {
    let argv = build_resume_argv("codex --search", "id")?;
    assert_eq!(argv, vec!["codex", "--search", "resume", "id"]);
    Ok(())
}

#[test]
fn test_build_resume_argv_claude() -> Result<()> {
    let argv = build_resume_argv("claude --debug", "id")?;
    assert_eq!(argv, vec!["claude", "--debug", "--resume", "id"]);
    Ok(())
}

#[test]
fn test_build_resume_argv_other() -> Result<()> {
    let argv = build_resume_argv("echo hello", "id")?;
    assert_eq!(argv, vec!["echo", "hello"]);
    Ok(())
}

#[test]
fn test_build_resume_argv_minimal_and_shell_forms() -> Result<()> {
    let codex = build_resume_argv("codex", "conversation")?;
    assert_eq!(codex, vec!["codex", "resume", "conversation"]);

    let claude = build_resume_argv("claude", "conversation")?;
    assert_eq!(claude, vec!["claude", "--resume", "conversation"]);

    let shell = build_resume_argv("sh -c 'echo hi'", "conversation")?;
    assert_eq!(shell, vec!["sh", "-c", "echo hi"]);
    Ok(())
}

#[test]
fn test_try_detect_codex_session_id_from_fake_store() -> Result<()> {
    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;

    let sessions_root = temp.path().join("sessions");
    let date_dir = conversation_support::codex_date_dir(&sessions_root, Local::now().date_naive());
    fs::create_dir_all(&date_dir)?;

    let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
    fs::write(&session_path, codex_session_meta_line("deadbeef", &workdir))?;

    let id = conversation_support::detect_codex_session_id_once_in_root(
        &sessions_root,
        &workdir,
        SystemTime::UNIX_EPOCH,
        &HashSet::new(),
    );
    assert_eq!(id.as_deref(), Some("deadbeef"));
    Ok(())
}

#[test]
fn test_try_detect_codex_session_id_with_retry_returns_none_on_overflow() {
    let mut detect_once = || None;
    let mut now = || SystemTime::UNIX_EPOCH;
    let mut sleep = std::mem::drop::<Duration>;
    let id = conversation_support::try_detect_codex_session_id_with_retry(
        Duration::from_secs(u64::MAX),
        &mut detect_once,
        &mut now,
        &mut sleep,
    );
    assert!(id.is_none());
}

#[test]
fn test_try_detect_codex_session_id_with_retry_times_out() {
    let mut detect_calls = 0;
    let mut now_calls = 0;
    let mut slept = false;
    let id = {
        let mut detect_once = || {
            detect_calls += 1;
            None
        };
        let mut now = || {
            now_calls += 1;
            SystemTime::UNIX_EPOCH
        };
        let mut sleep = |_| slept = true;
        conversation_support::try_detect_codex_session_id_with_retry(
            Duration::from_millis(0),
            &mut detect_once,
            &mut now,
            &mut sleep,
        )
    };
    assert!(id.is_none());
    assert_eq!(detect_calls, 1);
    assert_eq!(now_calls, 2);
    assert!(!slept);
}

#[test]
fn test_try_detect_codex_session_id_returns_none_when_no_match() -> Result<()> {
    let _guard = lock_env_test_environment();
    let temp = TempDir::new()?;
    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;
    let sessions_root = temp.path().join("sessions");
    fs::create_dir_all(&sessions_root)?;

    let exclude_ids = HashSet::new();
    let id = conversation_support::with_codex_sessions_root_override(Some(sessions_root), || {
        try_detect_codex_session_id(
            &workdir,
            SystemTime::UNIX_EPOCH,
            &exclude_ids,
            Duration::from_millis(0),
        )
    });
    assert!(id.is_none());
    Ok(())
}

#[test]
fn test_codex_sessions_root() {
    let _guard = lock_env_test_environment();
    let root = conversation_support::with_codex_sessions_root_unset(
        conversation_support::codex_sessions_root,
    );
    if let Some(root) = root {
        assert_eq!(
            root.file_name().and_then(|name| name.to_str()),
            Some("sessions")
        );
    } else {
        assert!(std::env::var_os("CODEX_HOME").is_none());
        assert!(tenex::paths::home_dir().is_none());
    }
}

#[test]
fn test_read_codex_session_meta_returns_none_for_empty_file() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("empty.jsonl");
    fs::write(&path, "")?;
    assert!(!conversation_support::read_codex_session_meta_is_some(
        &path
    ));
    Ok(())
}

#[test]
fn test_read_codex_session_meta_returns_none_for_invalid_json() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("invalid.jsonl");
    fs::write(&path, "{not-json}\n")?;
    assert!(!conversation_support::read_codex_session_meta_is_some(
        &path
    ));
    Ok(())
}

#[test]
fn test_read_codex_session_meta_returns_none_for_non_session_meta_kind() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("wrong-kind.jsonl");
    fs::write(
        &path,
        serde_json::json!({
            "type": "not_session_meta",
            "payload": { "id": "deadbeef", "cwd": "/tmp" },
        })
        .to_string()
            + "\n",
    )?;
    assert!(!conversation_support::read_codex_session_meta_is_some(
        &path
    ));
    Ok(())
}

#[test]
fn test_read_codex_session_meta_returns_none_when_file_missing() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("missing.jsonl");
    assert!(!conversation_support::read_codex_session_meta_is_some(
        &path
    ));
    Ok(())
}

#[test]
fn test_normalize_path_falls_back_when_canonicalize_fails() -> Result<()> {
    let temp = TempDir::new()?;
    let path = temp.path().join("missing");
    assert_eq!(conversation_support::normalize_path(&path), path);
    Ok(())
}

#[test]
fn test_normalize_path_prefers_canonicalize_when_it_succeeds() -> Result<()> {
    let temp = TempDir::new()?;
    let dir = temp.path().join("dir");
    fs::create_dir_all(&dir)?;
    let expected = dir.canonicalize()?;
    assert_eq!(conversation_support::normalize_path(&dir), expected);
    Ok(())
}

#[test]
fn test_try_detect_codex_session_id_returns_none_when_sessions_root_missing() -> Result<()> {
    let _guard = lock_env_test_environment();
    let temp = TempDir::new()?;
    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;

    let exclude_ids = HashSet::new();
    let id = conversation_support::with_codex_sessions_root_override(None, || {
        try_detect_codex_session_id(
            &workdir,
            SystemTime::UNIX_EPOCH,
            &exclude_ids,
            Duration::from_millis(0),
        )
    });
    assert!(id.is_none());
    Ok(())
}

#[test]
fn test_codex_sessions_root_override_handles_poisoned_mutex() {
    let _guard = lock_env_test_environment();
    conversation_support::poison_codex_sessions_root_override_mutex();

    let tmp_root = std::env::temp_dir().join("tenex-codex-sessions");
    conversation_support::with_codex_sessions_root_override(Some(tmp_root.clone()), || {
        assert_eq!(conversation_support::codex_sessions_root(), Some(tmp_root));
    });

    conversation_support::with_codex_sessions_root_override(None, || {
        assert_eq!(conversation_support::codex_sessions_root(), None);
    });
}

#[test]
fn test_detect_codex_session_id_once_in_root_filters_since() -> Result<()> {
    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;

    let sessions_root = temp.path().join("sessions");
    let date_dir = conversation_support::codex_date_dir(&sessions_root, Local::now().date_naive());
    fs::create_dir_all(&date_dir)?;

    let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
    fs::write(&session_path, codex_session_meta_line("deadbeef", &workdir))?;

    let id = conversation_support::detect_codex_session_id_once_in_root(
        &sessions_root,
        &workdir,
        SystemTime::now() + Duration::from_secs(60),
        &HashSet::new(),
    );
    assert!(id.is_none());
    Ok(())
}

#[test]
fn test_detect_codex_session_id_once_in_root_excludes_ids() -> Result<()> {
    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;

    let sessions_root = temp.path().join("sessions");
    let date_dir = conversation_support::codex_date_dir(&sessions_root, Local::now().date_naive());
    fs::create_dir_all(&date_dir)?;

    let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
    fs::write(&session_path, codex_session_meta_line("deadbeef", &workdir))?;

    let exclude_ids: HashSet<String> = std::iter::once("deadbeef".to_string()).collect();
    let id = conversation_support::detect_codex_session_id_once_in_root(
        &sessions_root,
        &workdir,
        SystemTime::UNIX_EPOCH,
        &exclude_ids,
    );
    assert!(id.is_none());
    Ok(())
}

#[test]
fn test_detect_codex_session_id_once_in_dirs_skips_read_dir_errors() -> Result<()> {
    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;
    let not_a_dir = temp.path().join("not-a-dir");
    fs::write(&not_a_dir, "not a directory")?;

    let id = conversation_support::detect_codex_session_id_once_in_dirs(
        &[not_a_dir],
        &workdir,
        SystemTime::UNIX_EPOCH,
        &HashSet::new(),
    );
    assert!(id.is_none());
    Ok(())
}

#[test]
fn test_detect_codex_session_id_once_in_dirs_prefers_newest_modified() -> Result<()> {
    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;

    let sessions_root = temp.path().join("sessions");
    let date_dir = conversation_support::codex_date_dir(&sessions_root, Local::now().date_naive());
    fs::create_dir_all(&date_dir)?;

    let first_path = date_dir.join("rollout-2026-02-01T00-00-00-aaaa.jsonl");
    write_session(
        &first_path,
        "aaaa",
        &workdir,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    )?;

    let second_path = date_dir.join("rollout-2026-02-01T00-00-00-bbbb.jsonl");
    write_session(
        &second_path,
        "bbbb",
        &workdir,
        SystemTime::UNIX_EPOCH + Duration::from_secs(2),
    )?;

    let id = conversation_support::detect_codex_session_id_once_in_dirs(
        &[date_dir],
        &workdir,
        SystemTime::UNIX_EPOCH,
        &HashSet::new(),
    );
    assert_eq!(id.as_deref(), Some("bbbb"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn test_detect_codex_session_id_once_in_root_skips_metadata_errors() -> Result<()> {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;

    let sessions_root = temp.path().join("sessions");
    let date_dir = conversation_support::codex_date_dir(&sessions_root, Local::now().date_naive());
    fs::create_dir_all(&date_dir)?;

    let broken_target = date_dir.join("missing-target");
    let broken_link = date_dir.join("broken.jsonl");
    symlink(broken_target, broken_link)?;

    let id = conversation_support::detect_codex_session_id_once_in_root(
        &sessions_root,
        &workdir,
        SystemTime::UNIX_EPOCH,
        &HashSet::new(),
    );
    assert!(id.is_none());
    Ok(())
}

#[test]
fn test_detect_codex_session_id_once_in_root_prefers_newest_matching() -> Result<()> {
    let temp = TempDir::new()?;

    let workdir = temp.path().join("worktree");
    fs::create_dir_all(&workdir)?;
    let other_workdir = temp.path().join("other");
    fs::create_dir_all(&other_workdir)?;

    let sessions_root = temp.path().join("sessions");
    let date_dir = conversation_support::codex_date_dir(&sessions_root, Local::now().date_naive());
    fs::create_dir_all(&date_dir)?;

    fs::write(date_dir.join("ignore.txt"), "hello")?;
    fs::create_dir_all(date_dir.join("dir.jsonl"))?;

    fs::write(date_dir.join("empty.jsonl"), "")?;
    let wrong_kind = "{\"type\":\"other\",\"payload\":{\"id\":\"other\",\"cwd\":\"/tmp\"}}\n";
    fs::write(date_dir.join("wrong_kind.jsonl"), wrong_kind)?;
    fs::write(date_dir.join("invalid.jsonl"), "{")?;
    let wrong_cwd = codex_session_meta_line("wrongcwd", &other_workdir);
    fs::write(date_dir.join("wrong_cwd.jsonl"), wrong_cwd)?;

    let first_path = date_dir.join("first.jsonl");
    write_session(
        &first_path,
        "first",
        &workdir,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1),
    )?;

    let third_path = date_dir.join("third.jsonl");
    write_session(
        &third_path,
        "third",
        &workdir,
        SystemTime::UNIX_EPOCH + Duration::from_secs(2),
    )?;

    let second_path = date_dir.join("second.jsonl");
    write_session(
        &second_path,
        "second",
        &workdir,
        SystemTime::UNIX_EPOCH + Duration::from_secs(3),
    )?;

    let id = conversation_support::detect_codex_session_id_once_in_root(
        &sessions_root,
        &workdir,
        SystemTime::UNIX_EPOCH,
        &HashSet::new(),
    );
    assert_eq!(id.as_deref(), Some("second"));
    Ok(())
}

#[test]
fn test_try_detect_codex_session_id_with_retry_waits_for_session() {
    let mut detect_calls = 0;
    let mut now_calls = 0;
    let mut slept = Vec::new();
    let id = {
        let mut detect_once = || {
            detect_calls += 1;
            if detect_calls == 1 {
                None
            } else {
                Some("deadbeef".to_string())
            }
        };
        let mut now = || {
            now_calls += 1;
            SystemTime::UNIX_EPOCH
        };
        let mut sleep = |duration| slept.push(duration);
        conversation_support::try_detect_codex_session_id_with_retry(
            Duration::from_millis(200),
            &mut detect_once,
            &mut now,
            &mut sleep,
        )
    };
    assert_eq!(id.as_deref(), Some("deadbeef"));
    assert_eq!(detect_calls, 2);
    assert_eq!(now_calls, 2);
    assert_eq!(slept, vec![Duration::from_millis(25)]);
}

#[test]
fn test_normalize_path_returns_input_when_canonicalize_fails() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("missing");
    assert!(!missing.exists());
    assert_eq!(conversation_support::normalize_path(&missing), missing);
    Ok(())
}
