//! External conversation/session tracking for agent CLIs.
//!
//! Tenex persists a per-agent conversation id so it can respawn agents after a reboot/crash and
//! reconnect to the same Codex/Claude session instead of starting a new one.

use crate::command;
use chrono::{Datelike as _, Duration as ChronoDuration, Local, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

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

/// Build argv for spawning an agent.
///
/// For Claude, Tenex can optionally force a stable session id (so it can be resumed later).
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
pub fn try_detect_codex_session_id(
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String>,
    max_wait: Duration,
) -> Option<String> {
    let sessions_root = codex_sessions_root()?;
    try_detect_codex_session_id_in_root(&sessions_root, workdir, since, exclude_ids, max_wait)
}

fn try_detect_codex_session_id_in_root(
    sessions_root: &Path,
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String>,
    max_wait: Duration,
) -> Option<String> {
    let deadline = SystemTime::now().checked_add(max_wait)?;
    loop {
        if let Some(found) =
            detect_codex_session_id_once_in_root(sessions_root, workdir, since, exclude_ids)
        {
            return Some(found);
        }

        if SystemTime::now() >= deadline {
            return None;
        }

        std::thread::sleep(Duration::from_millis(25));
    }
}

fn detect_codex_session_id_once_in_root(
    sessions_root: &Path,
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String>,
) -> Option<String> {
    let wanted_cwd = normalize_path(workdir);

    let mut best: Option<(String, SystemTime)> = None;
    for date_dir in codex_candidate_date_dirs(sessions_root) {
        if let Ok(entries) = std::fs::read_dir(&date_dir) {
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

fn codex_sessions_root() -> Option<PathBuf> {
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::paths::home_dir().map(|home| home.join(".codex")))?;
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
    use tempfile::TempDir;

    #[test]
    fn test_detect_agent_cli() {
        assert_eq!(detect_agent_cli("claude"), AgentCli::Claude);
        assert_eq!(detect_agent_cli("codex"), AgentCli::Codex);
        assert_eq!(detect_agent_cli("sh -c 'echo hi'"), AgentCli::Other);
        assert_eq!(detect_agent_cli("   "), AgentCli::Other);
        assert_eq!(detect_agent_cli("sh -c 'unterminated"), AgentCli::Other);
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
    fn test_build_spawn_argv_claude_does_not_duplicate_session_id() -> Result<()> {
        let argv = build_spawn_argv("claude --session-id existing", Some("hello"), Some("abc"))?;
        assert_eq!(argv, vec!["claude", "--session-id", "existing", "hello"]);
        Ok(())
    }

    #[test]
    fn test_build_spawn_argv_claude_does_not_inject_when_resume_present() -> Result<()> {
        let argv = build_spawn_argv("claude --resume existing", Some("hello"), Some("abc"))?;
        assert_eq!(argv, vec!["claude", "--resume", "existing", "hello"]);
        Ok(())
    }

    #[test]
    fn test_build_spawn_argv_non_claude_does_not_inject_session_id() -> Result<()> {
        let argv = build_spawn_argv("codex", Some("hello"), Some("abc"))?;
        assert_eq!(argv, vec!["codex", "hello"]);
        Ok(())
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
    fn test_try_detect_codex_session_id_from_fake_store() -> Result<()> {
        let temp = TempDir::new()?;

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir)?;

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
        let contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"deadbeef\",\"cwd\":\"{}\"}}}}\n",
            workdir.display()
        );
        std::fs::write(&session_path, contents)?;

        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert_eq!(id.as_deref(), Some("deadbeef"));
        Ok(())
    }

    #[test]
    fn test_try_detect_codex_session_id_in_root_returns_none_on_overflow() -> Result<()> {
        let temp = TempDir::new()?;
        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        std::fs::create_dir_all(&sessions_root)?;

        let id = try_detect_codex_session_id_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
            Duration::from_secs(u64::MAX),
        );
        assert!(id.is_none());
        Ok(())
    }

    #[test]
    fn test_try_detect_codex_session_id_in_root_times_out() -> Result<()> {
        let temp = TempDir::new()?;
        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        std::fs::create_dir_all(&sessions_root)?;

        let id = try_detect_codex_session_id_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
            Duration::from_millis(0),
        );
        assert!(id.is_none());
        Ok(())
    }

    #[test]
    fn test_try_detect_codex_session_id_returns_none_when_no_match() -> Result<()> {
        let temp = TempDir::new()?;
        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let id = try_detect_codex_session_id(
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
            Duration::from_millis(0),
        );
        assert!(id.is_none());
        Ok(())
    }

    #[test]
    fn test_codex_sessions_root() -> Result<()> {
        let root =
            codex_sessions_root().ok_or_else(|| anyhow::anyhow!("Missing Codex sessions root"))?;
        assert_eq!(
            root.file_name().and_then(|name| name.to_str()),
            Some("sessions")
        );
        Ok(())
    }

    #[test]
    fn test_detect_codex_session_id_once_in_root_filters_since() -> Result<()> {
        let temp = TempDir::new()?;

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir)?;

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
        let contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"deadbeef\",\"cwd\":\"{}\"}}}}\n",
            workdir.display()
        );
        std::fs::write(&session_path, contents)?;

        let id = detect_codex_session_id_once_in_root(
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
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir)?;

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");
        let contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"deadbeef\",\"cwd\":\"{}\"}}}}\n",
            workdir.display()
        );
        std::fs::write(&session_path, contents)?;

        let exclude_ids: HashSet<String> = std::iter::once("deadbeef".to_string()).collect();
        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &exclude_ids,
        );
        assert!(id.is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_codex_session_id_once_in_root_skips_metadata_errors() -> Result<()> {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new()?;

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir)?;

        let broken_target = date_dir.join("missing-target");
        let broken_link = date_dir.join("broken.jsonl");
        symlink(broken_target, broken_link)?;

        let id = detect_codex_session_id_once_in_root(
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
        std::fs::create_dir_all(&workdir)?;
        let other_workdir = temp.path().join("other");
        std::fs::create_dir_all(&other_workdir)?;

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir)?;

        std::fs::write(date_dir.join("ignore.txt"), "hello")?;
        std::fs::create_dir_all(date_dir.join("dir.jsonl"))?;

        std::fs::write(date_dir.join("empty.jsonl"), "")?;
        let wrong_kind = "{\"type\":\"other\",\"payload\":{\"id\":\"other\",\"cwd\":\"/tmp\"}}\n";
        std::fs::write(date_dir.join("wrong_kind.jsonl"), wrong_kind)?;
        std::fs::write(date_dir.join("invalid.jsonl"), "{")?;
        let wrong_cwd = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"wrongcwd\",\"cwd\":\"{}\"}}}}\n",
            other_workdir.display()
        );
        std::fs::write(date_dir.join("wrong_cwd.jsonl"), wrong_cwd)?;

        let first_path = date_dir.join("first.jsonl");
        let first_contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"first\",\"cwd\":\"{}\"}}}}\n",
            workdir.display()
        );
        std::fs::write(&first_path, first_contents)?;

        let third_path = date_dir.join("third.jsonl");
        let third_contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"third\",\"cwd\":\"{}\"}}}}\n",
            workdir.display()
        );
        std::fs::write(&third_path, third_contents)?;

        std::thread::sleep(Duration::from_secs(1));

        let second_path = date_dir.join("second.jsonl");
        let second_contents = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"second\",\"cwd\":\"{}\"}}}}\n",
            workdir.display()
        );
        std::fs::write(&second_path, second_contents)?;

        let id = detect_codex_session_id_once_in_root(
            &sessions_root,
            &workdir,
            SystemTime::UNIX_EPOCH,
            &HashSet::new(),
        );
        assert_eq!(id.as_deref(), Some("second"));
        Ok(())
    }

    #[test]
    fn test_try_detect_codex_session_id_in_root_waits_for_session() -> Result<()> {
        let temp = TempDir::new()?;

        let workdir = temp.path().join("worktree");
        std::fs::create_dir_all(&workdir)?;

        let sessions_root = temp.path().join("sessions");
        let date_dir = codex_date_dir(&sessions_root, Local::now().date_naive());
        std::fs::create_dir_all(&date_dir)?;

        let session_path = date_dir.join("rollout-2026-02-01T00-00-00-deadbeef.jsonl");

        let id = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(30));
                let _ = std::fs::write(
                    &session_path,
                    format!(
                        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"deadbeef\",\"cwd\":\"{}\"}}}}\n",
                        workdir.display()
                    ),
                );
            });

            try_detect_codex_session_id_in_root(
                &sessions_root,
                &workdir,
                SystemTime::UNIX_EPOCH,
                &HashSet::new(),
                Duration::from_millis(200),
            )
        });

        assert_eq!(id.as_deref(), Some("deadbeef"));
        Ok(())
    }
}
