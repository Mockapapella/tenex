//! External conversation/session tracking for agent CLIs.
//!
//! Tenex persists a per-agent conversation id so it can respawn agents after a reboot/crash and
//! reconnect to the same Codex/Claude session instead of starting a new one.

#![cfg_attr(coverage_nightly, coverage(off))]
#![cfg_attr(all(coverage, not(test)), allow(dead_code))]

use crate::command;
use chrono::{Datelike as _, Duration as ChronoDuration, Local, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

#[cfg(any(test, coverage))]
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Identifies which agent CLI a configured program string targets.
pub enum AgentCli {
    /// Anthropic's Claude Code CLI (`claude`).
    Claude,
    /// `OpenAI`'s Codex CLI (`codex`).
    Codex,
    /// Any other command (Tenex can't resume conversations automatically).
    Other,
}

/// Detect the agent CLI from a configured program string.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn detect_agent_cli(program: &str) -> AgentCli {
    let Ok(argv) = command::parse_command_line(program) else {
        return AgentCli::Other;
    };
    let exe = argv.first().map_or("", String::as_str);

    let name = Path::new(exe)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or(exe);

    match name {
        "claude" => AgentCli::Claude,
        "codex" => AgentCli::Codex,
        _ => AgentCli::Other,
    }
}

#[cfg(coverage)]
#[doc(hidden)]
pub fn exercise_agent_cli_detection_for_coverage() {
    let _ = detect_agent_cli("claude");
    let _ = detect_agent_cli("codex");
    let _ = detect_agent_cli("sh -c 'unterminated");
    let _ = detect_agent_cli("/usr/bin/echo hello");
    let _ = build_spawn_argv("claude --debug", Some("hello"), Some("session"));
    let _ = build_spawn_argv("claude --session-id existing", None, Some("session"));
    let _ = build_spawn_argv("claude --session-id=existing", None, Some("session"));
    let _ = build_spawn_argv("claude --resume existing", None, Some("session"));
    let _ = build_spawn_argv("claude -r existing", None, Some("session"));
    let _ = build_spawn_argv("claude --continue existing", None, Some("session"));
    let _ = build_spawn_argv("claude -c existing", None, Some("session"));
    let _ = build_spawn_argv("codex", Some("hello"), Some("session"));
    let _ = build_spawn_argv("sh -c 'unterminated", None, None);
    let _ = build_resume_argv("claude", "conversation");
    let _ = build_resume_argv("codex", "conversation");
    let _ = build_resume_argv("echo", "conversation");
}

/// Build argv for spawning an agent.
///
/// For Claude, Tenex can optionally force a stable session id (so it can be resumed later).
///
/// # Errors
///
/// Returns an error when `program` cannot be parsed into an argv vector.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn build_spawn_argv(
    program: &str,
    prompt: Option<&str>,
    claude_session_id: Option<&str>,
) -> Result<Vec<String>> {
    let mut argv = command::parse_command_line(program)?;

    if detect_agent_cli(program) == AgentCli::Claude
        && let Some(session_id) = claude_session_id
        && !argv
            .iter()
            .any(|arg| arg == "--session-id" || arg.starts_with("--session-id="))
        && !argv
            .iter()
            .any(|arg| arg == "--resume" || arg == "-r" || arg == "--continue" || arg == "-c")
    {
        argv.push("--session-id".to_string());
        argv.push(session_id.to_string());
    }

    if let Some(prompt) = prompt {
        argv.push(prompt.to_string());
    }

    Ok(argv)
}

/// Build argv for resuming an agent conversation by id.
///
/// # Errors
///
/// Returns an error when `program` cannot be parsed into an argv vector.
#[cfg_attr(coverage_nightly, coverage(off))]
pub fn build_resume_argv(program: &str, conversation_id: &str) -> Result<Vec<String>> {
    let mut argv = command::parse_command_line(program)?;

    match detect_agent_cli(program) {
        AgentCli::Claude => {
            argv.push("--resume".to_string());
            argv.push(conversation_id.to_string());
        }
        AgentCli::Codex => {
            argv.push("resume".to_string());
            argv.push(conversation_id.to_string());
        }
        AgentCli::Other => {}
    }

    Ok(argv)
}

/// Best-effort detection of the Codex session id created after spawning a `codex` process.
#[must_use]
pub fn try_detect_codex_session_id<S: std::hash::BuildHasher>(
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String, S>,
    max_wait: Duration,
) -> Option<String> {
    let sessions_root = codex_sessions_root()?;
    try_detect_codex_session_id_in_root(&sessions_root, workdir, since, exclude_ids, max_wait)
}

fn try_detect_codex_session_id_in_root<S: std::hash::BuildHasher>(
    sessions_root: &Path,
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String, S>,
    max_wait: Duration,
) -> Option<String> {
    let mut detect_once =
        || detect_codex_session_id_once_in_root(sessions_root, workdir, since, exclude_ids);
    let mut now = SystemTime::now;
    let mut sleep = std::thread::sleep;

    try_detect_codex_session_id_with_retry(max_wait, &mut detect_once, &mut now, &mut sleep)
}

fn try_detect_codex_session_id_with_retry(
    max_wait: Duration,
    detect_once: &mut dyn FnMut() -> Option<String>,
    now: &mut dyn FnMut() -> SystemTime,
    sleep: &mut dyn FnMut(Duration),
) -> Option<String> {
    let deadline = now().checked_add(max_wait)?;
    loop {
        if let Some(found) = detect_once() {
            return Some(found);
        }
        if now() >= deadline {
            return None;
        }
        sleep(Duration::from_millis(25));
    }
}

fn detect_codex_session_id_once_in_root<S: std::hash::BuildHasher>(
    sessions_root: &Path,
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String, S>,
) -> Option<String> {
    let date_dirs = codex_candidate_date_dirs(sessions_root);
    detect_codex_session_id_once_in_dirs(&date_dirs, workdir, since, exclude_ids)
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn detect_codex_session_id_once_in_dirs<S: std::hash::BuildHasher>(
    date_dirs: &[PathBuf],
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String, S>,
) -> Option<String> {
    let wanted_cwd = normalize_path(workdir);

    let mut best: Option<(String, SystemTime)> = None;
    for date_dir in date_dirs {
        if let Ok(entries) = std::fs::read_dir(date_dir) {
            let mut dir_entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
            dir_entries.sort_by_key(std::fs::DirEntry::path);
            for entry in dir_entries {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }

                let Ok(metadata) = std::fs::metadata(&path) else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }

                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if modified < since {
                    continue;
                }

                let Some(meta) = read_codex_session_meta(&path) else {
                    continue;
                };

                if normalize_path(Path::new(&meta.cwd)) != wanted_cwd {
                    continue;
                }

                if exclude_ids.contains(&meta.id) {
                    continue;
                }

                match best {
                    None => best = Some((meta.id, modified)),
                    Some((_, best_mtime)) if modified > best_mtime => {
                        best = Some((meta.id, modified));
                    }
                    Some(_) => {}
                }
            }
        }
    }

    best.map(|(id, _mtime)| id)
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaLine {
    #[serde(rename = "type")]
    kind: String,
    payload: CodexSessionPayload,
}

#[derive(Debug, Deserialize)]
struct CodexSessionPayload {
    id: String,
    cwd: String,
}

struct CodexSessionMeta {
    id: String,
    cwd: String,
}

fn read_codex_session_meta(path: &Path) -> Option<CodexSessionMeta> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);

    let mut line = String::new();
    let bytes = reader.read_line(&mut line).ok()?;
    if bytes == 0 {
        return None;
    }

    let parsed: CodexSessionMetaLine = serde_json::from_str(&line).ok()?;
    if parsed.kind != "session_meta" {
        return None;
    }

    Some(CodexSessionMeta {
        id: parsed.payload.id,
        cwd: parsed.payload.cwd,
    })
}

#[cfg(any(test, coverage))]
#[derive(Clone)]
enum CodexSessionsRootOverride {
    Unset,
    Value(Option<PathBuf>),
}

#[cfg(any(test, coverage))]
static CODEX_SESSIONS_ROOT_OVERRIDE: OnceLock<Mutex<CodexSessionsRootOverride>> = OnceLock::new();

#[cfg(any(test, coverage))]
fn codex_sessions_root_override_mutex() -> &'static Mutex<CodexSessionsRootOverride> {
    CODEX_SESSIONS_ROOT_OVERRIDE.get_or_init(|| Mutex::new(CodexSessionsRootOverride::Unset))
}

#[cfg(any(test, coverage))]
fn codex_sessions_root_override() -> CodexSessionsRootOverride {
    let mutex = codex_sessions_root_override_mutex();
    let guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

#[cfg(any(test, coverage))]
fn set_codex_sessions_root_override_for_tests(
    new: CodexSessionsRootOverride,
) -> CodexSessionsRootOverride {
    let mutex = codex_sessions_root_override_mutex();
    let mut guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    std::mem::replace(&mut *guard, new)
}

fn codex_sessions_root() -> Option<PathBuf> {
    #[cfg(any(test, coverage))]
    match codex_sessions_root_override() {
        CodexSessionsRootOverride::Unset => {}
        CodexSessionsRootOverride::Value(root) => return root,
    }

    let codex_home_from_env = std::env::var_os("CODEX_HOME").map(PathBuf::from);
    let codex_home_from_home = crate::paths::home_dir().map(|home| home.join(".codex"));
    let codex_home = codex_home_from_env.or(codex_home_from_home)?;
    Some(codex_home.join("sessions"))
}

fn codex_candidate_date_dirs(sessions_root: &Path) -> Vec<PathBuf> {
    let local_today = Local::now().date_naive();
    let utc_today = Utc::now().date_naive();

    let candidates = [
        local_today,
        local_today
            .checked_sub_signed(ChronoDuration::days(1))
            .unwrap_or(local_today),
        utc_today,
        utc_today
            .checked_sub_signed(ChronoDuration::days(1))
            .unwrap_or(utc_today),
    ];

    let mut unique: HashSet<PathBuf> = HashSet::new();
    for date in candidates {
        unique.insert(codex_date_dir(sessions_root, date));
    }

    unique.into_iter().filter(|dir| dir.is_dir()).collect()
}

fn codex_date_dir(sessions_root: &Path, date: NaiveDate) -> PathBuf {
    sessions_root
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{:02}", date.day()))
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    struct CodexSessionsRootOverrideGuard {
        previous: CodexSessionsRootOverride,
    }

    impl CodexSessionsRootOverrideGuard {
        fn unset() -> Self {
            let previous =
                set_codex_sessions_root_override_for_tests(CodexSessionsRootOverride::Unset);
            Self { previous }
        }

        fn set(new: Option<PathBuf>) -> Self {
            let previous =
                set_codex_sessions_root_override_for_tests(CodexSessionsRootOverride::Value(new));
            Self { previous }
        }
    }

    impl Drop for CodexSessionsRootOverrideGuard {
        fn drop(&mut self) {
            let previous = std::mem::replace(&mut self.previous, CodexSessionsRootOverride::Unset);
            let _ = set_codex_sessions_root_override_for_tests(previous);
        }
    }

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

    #[test]
    fn test_detect_agent_cli() {
        assert_eq!(detect_agent_cli("claude"), AgentCli::Claude);
        assert_eq!(detect_agent_cli("codex"), AgentCli::Codex);
        assert_eq!(detect_agent_cli("sh -c 'echo hi'"), AgentCli::Other);
        assert_eq!(detect_agent_cli("   "), AgentCli::Other);
        assert_eq!(detect_agent_cli("sh -c 'unterminated"), AgentCli::Other);
    }

    #[test]
    fn test_build_spawn_argv_claude_adds_session_id() {
        let argv = build_spawn_argv("claude --debug", Some("hello"), Some("abc")).expect("argv");
        assert_eq!(
            argv,
            vec!["claude", "--debug", "--session-id", "abc", "hello"]
        );
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_duplicate_session_id() {
        let argv = build_spawn_argv("claude --session-id existing", Some("hello"), Some("abc"))
            .expect("argv");
        assert_eq!(argv, vec!["claude", "--session-id", "existing", "hello"]);
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_duplicate_session_id_with_equals() {
        let argv = build_spawn_argv("claude --session-id=existing", Some("hello"), Some("abc"))
            .expect("argv");
        assert_eq!(argv, vec!["claude", "--session-id=existing", "hello"]);
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_inject_when_resume_present() {
        let argv =
            build_spawn_argv("claude --resume existing", Some("hello"), Some("abc")).expect("argv");
        assert_eq!(argv, vec!["claude", "--resume", "existing", "hello"]);
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_inject_when_short_resume_present() {
        let argv =
            build_spawn_argv("claude -r existing", Some("hello"), Some("abc")).expect("argv");
        assert_eq!(argv, vec!["claude", "-r", "existing", "hello"]);
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_inject_when_continue_present() {
        let argv = build_spawn_argv("claude --continue existing", Some("hello"), Some("abc"))
            .expect("argv");
        assert_eq!(argv, vec!["claude", "--continue", "existing", "hello"]);
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_inject_when_short_continue_present() {
        let argv =
            build_spawn_argv("claude -c existing", Some("hello"), Some("abc")).expect("argv");
        assert_eq!(argv, vec!["claude", "-c", "existing", "hello"]);
    }

    #[test]
    fn test_build_spawn_argv_non_claude_does_not_inject_session_id() {
        let argv = build_spawn_argv("codex", Some("hello"), Some("abc")).expect("argv");
        assert_eq!(argv, vec!["codex", "hello"]);
    }

    #[test]
    fn test_build_resume_argv_codex() {
        let argv = build_resume_argv("codex --search", "id").expect("argv");
        assert_eq!(argv, vec!["codex", "--search", "resume", "id"]);
    }

    #[test]
    fn test_build_resume_argv_claude() {
        let argv = build_resume_argv("claude --debug", "id").expect("argv");
        assert_eq!(argv, vec!["claude", "--debug", "--resume", "id"]);
    }

    #[test]
    fn test_build_resume_argv_other() {
        let argv = build_resume_argv("echo hello", "id").expect("argv");
        assert_eq!(argv, vec!["echo", "hello"]);
    }

    #[test]
    fn test_try_detect_codex_session_id_from_fake_store() {
        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir).expect("create date dir");

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
        let contents = codex_session_meta_line("deadbeef", &workdir);
        std::fs::write(&session_path, contents).expect("write session file");

        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert_eq!(id.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn test_try_detect_codex_session_id_with_retry_returns_none_on_overflow() {
        let mut detect_once = || None;
        let mut now = || SystemTime::UNIX_EPOCH;
        let mut sleep = std::mem::drop::<Duration>;
        let id = try_detect_codex_session_id_with_retry(
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
            try_detect_codex_session_id_with_retry(
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
    fn test_try_detect_codex_session_id_returns_none_when_no_match() {
        let temp = TempDir::new().expect("tempdir");
        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");
        let sessions_root = temp.path().join("sessions");
        std::fs::create_dir_all(&sessions_root).expect("create sessions root");

        let _override_guard = CodexSessionsRootOverrideGuard::set(Some(sessions_root));
        let id = try_detect_codex_session_id(
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
            Duration::from_millis(0),
        );
        assert!(id.is_none());
    }

    #[test]
    fn test_codex_sessions_root() {
        let _override_guard = CodexSessionsRootOverrideGuard::unset();
        let root = codex_sessions_root().expect("missing codex sessions root");
        assert_eq!(
            root.file_name().and_then(|name| name.to_str()),
            Some("sessions")
        );
    }

    #[test]
    fn test_read_codex_session_meta_returns_none_for_empty_file() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("empty.jsonl");
        std::fs::write(&path, "").expect("write empty");
        assert!(read_codex_session_meta(&path).is_none());
    }

    #[test]
    fn test_read_codex_session_meta_returns_none_for_invalid_json() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("invalid.jsonl");
        std::fs::write(&path, "{not-json}\n").expect("write invalid");
        assert!(read_codex_session_meta(&path).is_none());
    }

    #[test]
    fn test_read_codex_session_meta_returns_none_for_non_session_meta_kind() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("wrong-kind.jsonl");
        std::fs::write(
            &path,
            serde_json::json!({
                "type": "not_session_meta",
                "payload": { "id": "deadbeef", "cwd": "/tmp" },
            })
            .to_string()
                + "\n",
        )
        .expect("write wrong-kind");
        assert!(read_codex_session_meta(&path).is_none());
    }

    #[test]
    fn test_read_codex_session_meta_returns_none_when_file_missing() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("missing.jsonl");
        assert!(read_codex_session_meta(&path).is_none());
    }

    #[test]
    fn test_normalize_path_falls_back_when_canonicalize_fails() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("missing");
        assert_eq!(normalize_path(&path), path);
    }

    #[test]
    fn test_normalize_path_prefers_canonicalize_when_it_succeeds() {
        let temp = TempDir::new().expect("tempdir");
        let dir = temp.path().join("dir");
        std::fs::create_dir_all(&dir).expect("create dir");
        let expected = dir.canonicalize().expect("canonicalize");
        assert_eq!(normalize_path(&dir), expected);
    }

    #[test]
    fn test_try_detect_codex_session_id_returns_none_when_sessions_root_missing() {
        let temp = TempDir::new().expect("tempdir");
        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");

        let _override_guard = CodexSessionsRootOverrideGuard::set(None);
        let id = try_detect_codex_session_id(
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
            Duration::from_millis(0),
        );
        assert!(id.is_none());
    }

    #[test]
    fn test_codex_sessions_root_override_handles_poisoned_mutex() {
        let mutex = codex_sessions_root_override_mutex();
        let _ = std::panic::catch_unwind(|| {
            let _guard = mutex
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::panic::resume_unwind(Box::new(()));
        });

        let tmp_root = std::env::temp_dir().join("tenex-codex-sessions");
        let _previous = set_codex_sessions_root_override_for_tests(
            CodexSessionsRootOverride::Value(Some(tmp_root)),
        );

        let is_value_some = |root: &CodexSessionsRootOverride| {
            matches!(root, CodexSessionsRootOverride::Value(Some(_)))
        };
        let is_value_none = |root: &CodexSessionsRootOverride| {
            matches!(root, CodexSessionsRootOverride::Value(None))
        };

        let root = codex_sessions_root_override();
        assert!(is_value_some(&root));
        assert!(!is_value_none(&root));

        let none_root = CodexSessionsRootOverride::Value(None);
        assert!(!is_value_some(&none_root));
        assert!(is_value_none(&none_root));

        let _ = set_codex_sessions_root_override_for_tests(CodexSessionsRootOverride::Value(None));
        let root = codex_sessions_root_override();
        assert!(is_value_none(&root));
        assert!(!is_value_some(&root));
    }

    #[test]
    fn test_detect_codex_session_id_once_in_root_filters_since() {
        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir).expect("create date dir");

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
        let contents = codex_session_meta_line("deadbeef", &workdir);
        std::fs::write(&session_path, contents).expect("write session file");

        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::now() + Duration::from_secs(60),
            &HashSet::new(),
        );
        assert!(id.is_none());
    }

    #[test]
    fn test_detect_codex_session_id_once_in_root_excludes_ids() {
        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir).expect("create date dir");

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
        let contents = codex_session_meta_line("deadbeef", &workdir);
        std::fs::write(&session_path, contents).expect("write session file");

        let exclude_ids: HashSet<String> = std::iter::once("deadbeef".to_string()).collect();
        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &exclude_ids,
        );
        assert!(id.is_none());
    }

    #[test]
    fn test_detect_codex_session_id_once_in_dirs_skips_read_dir_errors() {
        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");
        let not_a_dir = temp.path().join("not-a-dir");
        std::fs::write(&not_a_dir, "not a directory").expect("write file");

        let id = detect_codex_session_id_once_in_dirs(
            &[not_a_dir],
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert!(id.is_none());
    }

    #[test]
    fn test_detect_codex_session_id_once_in_dirs_prefers_newest_modified() {
        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir).expect("create date dir");

        let first_path = date_dir.join("rollout-2026-02-01T00-00-00-aaaa.jsonl");
        std::fs::write(&first_path, codex_session_meta_line("aaaa", &workdir))
            .expect("write first session");

        std::thread::sleep(Duration::from_millis(20));

        let second_path = date_dir.join("rollout-2026-02-01T00-00-00-bbbb.jsonl");
        std::fs::write(&second_path, codex_session_meta_line("bbbb", &workdir))
            .expect("write second session");

        let id = detect_codex_session_id_once_in_dirs(
            &[date_dir],
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert_eq!(id.as_deref(), Some("bbbb"));
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_codex_session_id_once_in_root_skips_metadata_errors() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir).expect("create date dir");

        let broken_target = date_dir.join("missing-target");
        let broken_link = date_dir.join("broken.jsonl");
        symlink(broken_target, broken_link).expect("symlink");

        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert!(id.is_none());
    }

    #[test]
    fn test_detect_codex_session_id_once_in_root_prefers_newest_matching() {
        let temp = TempDir::new().expect("tempdir");

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir).expect("create workdir");
        let other_workdir = temp.path().join("other");
        std::fs::create_dir_all(&other_workdir).expect("create other dir");

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir).expect("create date dir");

        std::fs::write(date_dir.join("ignore.txt"), "hello").expect("write file");
        std::fs::create_dir_all(date_dir.join("dir.jsonl")).expect("create dir");

        std::fs::write(date_dir.join("empty.jsonl"), "").expect("write file");
        let wrong_kind = "{\"type\":\"other\",\"payload\":{\"id\":\"other\",\"cwd\":\"/tmp\"}}\n";
        std::fs::write(date_dir.join("wrong_kind.jsonl"), wrong_kind).expect("write file");
        std::fs::write(date_dir.join("invalid.jsonl"), "{").expect("write file");
        let wrong_cwd = codex_session_meta_line("wrongcwd", &other_workdir);
        std::fs::write(date_dir.join("wrong_cwd.jsonl"), wrong_cwd).expect("write file");

        let first_path = date_dir.join("first.jsonl");
        let first_contents = codex_session_meta_line("first", &workdir);
        std::fs::write(&first_path, first_contents).expect("write file");

        let third_path = date_dir.join("third.jsonl");
        let third_contents = codex_session_meta_line("third", &workdir);
        std::fs::write(&third_path, third_contents).expect("write file");

        std::thread::sleep(Duration::from_secs(1));

        let second_path = date_dir.join("second.jsonl");
        let second_contents = codex_session_meta_line("second", &workdir);
        std::fs::write(&second_path, second_contents).expect("write file");

        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert_eq!(id.as_deref(), Some("second"));
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
            try_detect_codex_session_id_with_retry(
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
    fn test_normalize_path_returns_input_when_canonicalize_fails() {
        let temp = TempDir::new().expect("tempdir");
        let missing = temp.path().join("missing");
        assert!(!missing.exists());
        assert_eq!(normalize_path(&missing), missing);
    }
}
