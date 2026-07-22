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
///
/// # Errors
///
/// Returns an error when `program` cannot be parsed into an argv vector.
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

fn detect_codex_session_id_once_in_root<S: std::hash::BuildHasher>(
    sessions_root: &Path,
    workdir: &Path,
    since: SystemTime,
    exclude_ids: &HashSet<String, S>,
) -> Option<String> {
    let date_dirs = codex_candidate_date_dirs(sessions_root);
    detect_codex_session_id_once_in_dirs(&date_dirs, workdir, since, exclude_ids)
}

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

fn codex_sessions_root() -> Option<PathBuf> {
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
