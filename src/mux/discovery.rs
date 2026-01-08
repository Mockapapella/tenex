//! Mux daemon socket discovery helpers.
//!
//! Tenex can run multiple mux daemons (for example across rebuilds/upgrades when the default
//! socket fingerprint changes). This module helps locate the daemon that owns a set of stored
//! sessions so agents can survive restarts.

use super::endpoint::socket_endpoint_from_value;
use super::ipc;
use super::protocol::{MuxRequest, MuxResponse};
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as StreamTrait;
use std::collections::{HashMap, HashSet};

/// Attempt to find a running mux daemon socket that contains at least one of the requested
/// session names.
///
/// `preferred_socket` is checked first when provided.
#[must_use]
pub fn discover_socket_for_sessions<S: std::hash::BuildHasher>(
    wanted_sessions: &HashSet<String, S>,
    preferred_socket: Option<&str>,
) -> Option<String> {
    if wanted_sessions.is_empty() {
        return None;
    }

    let mut candidates = Vec::new();
    if let Some(socket) = preferred_socket
        .map(str::trim)
        .filter(|socket| !socket.is_empty())
    {
        candidates.push(socket.to_string());
    }

    if let Ok(default_socket) = super::socket_display() {
        candidates.push(default_socket);
    }

    candidates.extend(running_mux_sockets());

    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));

    let mut best: Option<(usize, String)> = None;
    for candidate in candidates {
        let Some(matches) = probe_session_matches(&candidate, wanted_sessions) else {
            continue;
        };
        if matches == 0 {
            continue;
        }

        match &best {
            None => best = Some((matches, candidate)),
            Some((best_matches, _)) if matches > *best_matches => {
                best = Some((matches, candidate));
            }
            _ => {}
        }
    }

    best.map(|(_, socket)| socket)
}

fn probe_session_matches<S: std::hash::BuildHasher>(
    socket: &str,
    wanted_sessions: &HashSet<String, S>,
) -> Option<usize> {
    let endpoint = socket_endpoint_from_value(socket).ok()?;
    let mut stream = Stream::connect(endpoint.name).ok()?;

    ipc::write_json(&mut stream, &MuxRequest::ListSessions).ok()?;
    let response: MuxResponse = ipc::read_json(&mut stream).ok()?;

    let MuxResponse::Sessions { sessions } = response else {
        return None;
    };

    Some(
        sessions
            .into_iter()
            .filter(|session| wanted_sessions.contains(&session.name))
            .count(),
    )
}

#[cfg(target_os = "linux")]
fn running_mux_sockets() -> Vec<String> {
    let mut sockets = HashSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str() else {
            continue;
        };
        if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }

        let base = entry.path();
        let Ok(cmdline) = std::fs::read(base.join("cmdline")) else {
            continue;
        };
        if !cmdline_contains_muxd(&cmdline) {
            continue;
        }

        let Ok(environ) = std::fs::read(base.join("environ")) else {
            continue;
        };

        if let Some(value) = parse_environ(&environ).get("TENEX_MUX_SOCKET") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                let _ = sockets.insert(trimmed.to_string());
            }
        }
    }

    sockets.into_iter().collect()
}

#[cfg(not(target_os = "linux"))]
fn running_mux_sockets() -> Vec<String> {
    Vec::new()
}

fn cmdline_contains_muxd(cmdline: &[u8]) -> bool {
    cmdline
        .split(|b| *b == 0)
        .filter(|arg| !arg.is_empty())
        .any(|arg| arg == b"muxd")
}

fn parse_environ(environ: &[u8]) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    for entry in environ.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
        let mut parts = entry.splitn(2, |b| *b == b'=');
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };

        let (Ok(key), Ok(value)) = (std::str::from_utf8(key), std::str::from_utf8(value)) else {
            continue;
        };

        vars.insert(key.to_string(), value.to_string());
    }

    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use tempfile::TempDir;

    #[test]
    fn test_cmdline_contains_muxd() {
        assert!(cmdline_contains_muxd(b"tenex\0muxd\0"));
        assert!(!cmdline_contains_muxd(b"tenex\0\0"));
    }

    #[test]
    fn test_parse_environ() {
        let env = b"A=1\0TENEX_MUX_SOCKET=test\0B=2\0";
        let parsed = parse_environ(env);
        assert_eq!(parsed.get("A"), Some(&"1".to_string()));
        assert_eq!(parsed.get("TENEX_MUX_SOCKET"), Some(&"test".to_string()));
        assert_eq!(parsed.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn test_discover_socket_for_sessions_returns_none_for_empty_input() {
        let wanted_sessions: HashSet<String> = HashSet::new();
        assert!(discover_socket_for_sessions(&wanted_sessions, None).is_none());
    }

    #[test]
    fn test_discover_socket_for_sessions_finds_matching_session() -> Result<()> {
        let session_manager = crate::mux::SessionManager::new();
        let session_name = format!("tenex-test-discovery-{}", uuid::Uuid::new_v4());
        let workdir = TempDir::new()?;
        session_manager.create(&session_name, workdir.path(), None)?;

        let socket = crate::mux::socket_display()?;

        let mut wanted_sessions = HashSet::new();
        wanted_sessions.insert(session_name.clone());

        let discovered = discover_socket_for_sessions(&wanted_sessions, Some(&socket));

        let _ = session_manager.kill(&session_name);

        anyhow::ensure!(
            discovered.as_deref() == Some(socket.as_str()),
            "Expected to rediscover mux socket"
        );
        Ok(())
    }

    #[test]
    fn test_discover_socket_for_sessions_returns_none_when_no_matches() -> Result<()> {
        let session_manager = crate::mux::SessionManager::new();
        let existing_session = format!("tenex-test-discovery-existing-{}", uuid::Uuid::new_v4());
        let workdir = TempDir::new()?;
        session_manager.create(&existing_session, workdir.path(), None)?;

        let mut wanted_sessions = HashSet::new();
        wanted_sessions.insert(format!(
            "tenex-test-discovery-missing-{}",
            uuid::Uuid::new_v4()
        ));

        let socket = crate::mux::socket_display()?;
        let discovered = discover_socket_for_sessions(&wanted_sessions, Some(&socket));
        let _ = session_manager.kill(&existing_session);
        assert!(discovered.is_none());
        Ok(())
    }
}
