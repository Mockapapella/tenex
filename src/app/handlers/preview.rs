//! Preview operations: update preview, diff, and commits content

use crate::app::{App, Tab};
use crate::git::{self, DiffGenerator};
use crate::mux::SessionManager;
use anyhow::Result;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::Actions;

impl Actions {
    /// Update preview content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if preview update fails
    pub fn update_preview(self, app: &mut App) -> Result<()> {
        // When actively watching the preview and following the output, keep the history window
        // smaller so we can refresh more frequently without stuttering.
        const HISTORY_LINES_FOLLOWING: u32 = 300;

        let old_line_count = app.data.ui.preview_text.lines.len();
        let old_scroll = app.data.ui.preview_scroll;
        let visible_height = match app.data.ui.preview_dimensions {
            Some((_, height)) => usize::from(height),
            None => 20,
        };

        // When the user manually scrolls up, stop using a short tail buffer to avoid
        // the viewport "jumping" as the tail window slides.
        let wants_full_history = !app.data.ui.preview_follow;
        let switching_to_full_history =
            wants_full_history && !app.data.ui.preview_using_full_history;

        if let Some(agent) = app.selected_agent() {
            let target = preview_target(app, agent);
            if self.session_manager.exists(&agent.mux_session) {
                let (cols, rows) = app.data.ui.preview_dimensions.unwrap_or((80, 24));
                let cols = cols.max(1);
                let rows = rows.max(1);

                let streamed_ok = self.try_update_preview_streamed(
                    app,
                    &target,
                    cols,
                    rows,
                    wants_full_history,
                    HISTORY_LINES_FOLLOWING,
                );
                if !streamed_ok {
                    self.update_preview_with_capture(
                        app,
                        &target,
                        rows,
                        wants_full_history,
                        HISTORY_LINES_FOLLOWING,
                    );
                }
            } else {
                app.data.ui.preview_vt_by_target.remove(&target);
                set_preview_message(app, "(Session not running)");
            }
        } else {
            set_preview_message(app, "(No agent selected)");
        }

        // If we just switched from a short tail buffer to full history, preserve the user's
        // scroll position relative to the bottom of the buffer. Without this, the viewport
        // can appear to "jump" far up because the top of the buffer gained many lines.
        if switching_to_full_history {
            let new_line_count = app.data.ui.preview_text.lines.len();

            let old_max = old_line_count.saturating_sub(visible_height);
            let old_scroll = old_scroll.min(old_max);
            let distance_from_bottom = old_max.saturating_sub(old_scroll);

            let new_max = new_line_count.saturating_sub(visible_height);
            app.data.ui.preview_scroll = new_max.saturating_sub(distance_from_bottom);
        }

        // Auto-scroll to bottom only if follow mode is enabled
        // (disabled when user manually scrolls up, re-enabled when they scroll to bottom)
        if app.data.ui.preview_follow {
            app.data.ui.preview_scroll = usize::MAX;
        }

        app.data.ui.preview_using_full_history = wants_full_history;

        Ok(())
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn try_update_preview_streamed(
        self,
        app: &mut App,
        target: &str,
        cols: u16,
        rows: u16,
        wants_full_history: bool,
        history_lines_following: u32,
    ) -> bool {
        const MAX_BYTES: u32 = 64 * 1024;
        // Keep selection responsive when switching back to a noisy target. Large backlogs can be
        // drained over subsequent refresh ticks instead of blocking one UI turn while we replay
        // everything at once.
        const MAX_REQUESTS_PER_REFRESH: usize = 1;

        let target_key = target.to_string();
        let max_bytes = usize::try_from(MAX_BYTES).unwrap_or(usize::MAX);
        let requested = if wants_full_history {
            usize::MAX
        } else {
            usize::try_from(history_lines_following)
                .unwrap_or(usize::MAX)
                .max(usize::from(rows))
        };

        let (content, cursor_position) = {
            let vt = app
                .data
                .ui
                .preview_vt_by_target
                .entry(target_key.clone())
                .or_insert_with(|| {
                    crate::app::state::PreviewVtState::new(target_key.clone(), cols, rows)
                });

            if vt.dims != (cols, rows) {
                vt.reset(target_key.clone(), cols, rows);
            }

            for _ in 0..MAX_REQUESTS_PER_REFRESH {
                match self.output_stream.read_output(target, vt.after, MAX_BYTES) {
                    Ok(crate::mux::OutputRead::Chunk(chunk)) => {
                        if chunk.end < vt.after {
                            return false;
                        }

                        if !chunk.data.is_empty() {
                            vt.parser.process(&chunk.data);
                        }
                        vt.after = chunk.end;

                        if chunk.data.is_empty() || chunk.data.len() < max_bytes {
                            break;
                        }
                    }
                    Ok(crate::mux::OutputRead::Reset(reset)) => {
                        vt.reset(target_key.clone(), cols, rows);
                        if !reset.checkpoint.is_empty() {
                            vt.parser.process(&reset.checkpoint);
                        }
                        vt.after = reset.start;
                    }
                    Err(_) => {
                        return false;
                    }
                }
            }

            let content = crate::mux::render::capture_lines(&mut vt.parser, requested);
            let (cursor_row, cursor_col) = vt.parser.screen().cursor_position();
            let cursor_hidden = vt.parser.screen().hide_cursor();
            let cursor_position = Some((cursor_col, cursor_row, cursor_hidden));
            (content, cursor_position)
        };

        app.data.ui.set_preview_content(content);
        app.data.ui.preview_cursor_position = cursor_position;
        app.data.ui.preview_pane_size = Some((cols, rows));
        true
    }

    fn update_preview_with_capture(
        self,
        app: &mut App,
        target: &str,
        rows: u16,
        wants_full_history: bool,
        history_lines_following: u32,
    ) {
        app.data.ui.preview_vt_by_target.remove(target);

        let history_lines = history_lines_following.max(u32::from(rows));
        let content = if wants_full_history {
            self.output_capture
                .capture_full_history(target)
                .unwrap_or_default()
        } else {
            self.output_capture
                .capture_pane_with_history(target, history_lines)
                .unwrap_or_default()
        };

        app.data.ui.set_preview_content(content);
        app.data.ui.preview_cursor_position = self.output_capture.cursor_position(target).ok();
        app.data.ui.preview_pane_size = self.output_capture.pane_size(target).ok();
    }

    /// Update diff content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if diff update fails
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn update_diff(self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            if agent.worktree_path.exists() {
                if let Ok(repo) = git::open_repository(&agent.worktree_path) {
                    let diff_gen = DiffGenerator::new(&repo);
                    app.data.ui.diff_force_refresh = false;

                    let model = match diff_gen.uncommitted_model() {
                        Ok(model) => model,
                        Err(err) => {
                            app.data.ui.diff_model = None;
                            app.data.ui.diff_hash = 0;
                            app.data.ui.diff_has_unseen_changes = false;
                            app.data
                                .ui
                                .set_diff_content(format!("(Failed to generate diff: {err:#})"));
                            return Ok(());
                        }
                    };
                    let marker_hash = diff_gen.uncommitted_change_marker().unwrap_or(u64::MAX);

                    app.data.ui.diff_hash = marker_hash;
                    app.data.ui.diff_model = Some(model.clone());

                    let (content, meta) = app.data.ui.build_diff_view(&model);
                    app.data.ui.set_diff_view(content, meta);

                    if app.data.active_tab == Tab::Diff {
                        app.data
                            .ui
                            .set_diff_last_seen_hash_for_agent(agent_id, marker_hash);
                        app.data.ui.diff_has_unseen_changes = false;
                    } else {
                        app.data.ui.diff_has_unseen_changes = marker_hash != 0
                            && marker_hash != app.data.ui.diff_last_seen_hash_for_agent(agent_id);
                    }
                } else {
                    app.data.ui.diff_model = None;
                    app.data.ui.diff_hash = 0;
                    app.data.ui.diff_has_unseen_changes = false;
                    app.data.ui.set_diff_content("(Not a git repository)");
                }
            } else {
                app.data.ui.diff_model = None;
                app.data.ui.diff_hash = 0;
                app.data.ui.diff_has_unseen_changes = false;
                app.data.ui.set_diff_content("(Worktree not found)");
            }
        } else {
            app.data.ui.diff_model = None;
            app.data.ui.diff_hash = 0;
            app.data.ui.diff_has_unseen_changes = false;
            app.data.ui.set_diff_content("(No agent selected)");
        }
        Ok(())
    }

    /// Update diff digest (hash + unseen flag) without rebuilding the full diff view.
    ///
    /// # Errors
    ///
    /// Returns an error if digest computation fails.
    pub fn update_diff_digest(self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            if agent.worktree_path.exists() {
                let repo = git::open_repository(&agent.worktree_path)?;
                let diff_gen = DiffGenerator::new(&repo);
                let marker_hash = diff_gen.uncommitted_change_marker()?;

                app.data.ui.diff_hash = marker_hash;
                app.data.ui.diff_has_unseen_changes = marker_hash != 0
                    && marker_hash != app.data.ui.diff_last_seen_hash_for_agent(agent_id);
            } else {
                app.data.ui.diff_hash = 0;
                app.data.ui.diff_has_unseen_changes = false;
            }
        } else {
            app.data.ui.diff_hash = 0;
            app.data.ui.diff_has_unseen_changes = false;
        }

        Ok(())
    }

    /// Update commit list content for the selected agent.
    ///
    /// Shows only commits that are in the current branch and not in the detected base branch
    /// (i.e. `base..HEAD`), similar to what a PR would contain.
    ///
    /// # Errors
    ///
    /// Returns an error if git operations fail unexpectedly.
    pub fn update_commits(self, app: &mut App) -> Result<()> {
        Self::update_commits_with_limit(app, 200)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn update_commits_with_limit(app: &mut App, max_commits: usize) -> Result<()> {
        let max_commits = max_commits.max(1);

        let Some(agent) = app.selected_agent() else {
            app.data.ui.commits_hash = 0;
            app.data.ui.commits_has_unseen_changes = false;
            app.data.ui.set_commits_content("(No agent selected)");
            return Ok(());
        };

        let agent_id = agent.id;
        let worktree_path = agent.worktree_path.clone();
        let branch_name = agent.branch.clone();

        if !worktree_path.exists() {
            app.data.ui.commits_hash = 0;
            app.data.ui.commits_has_unseen_changes = false;
            app.data.ui.set_commits_content("(Worktree not found)");
            return Ok(());
        }

        if git::open_repository(&worktree_path).is_err() {
            app.data.ui.commits_hash = 0;
            app.data.ui.commits_has_unseen_changes = false;
            app.data.ui.set_commits_content("(Not a git repository)");
            return Ok(());
        }

        let base_branch = Self::detect_base_branch(&worktree_path, &branch_name);

        let range = format!("{base_branch}..HEAD");

        let (commits, used_range, truncated) =
            git_log_rich(&worktree_path, Some(&range), max_commits)
                .or_else(|_| git_log_rich(&worktree_path, None, max_commits))?;

        let commits_hash = hash_commit_ids(commits.iter().map(|c| c.id.as_str()));
        app.data.ui.commits_hash = commits_hash;

        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("Branch: {branch_name}"));
        let suffix = if truncated { " (truncated)" } else { "" };
        if used_range {
            lines.push(format!(
                "Commits: {base_branch}..HEAD ({n} shown){suffix}",
                n = commits.len()
            ));
        } else {
            lines.push(format!(
                "Commits: HEAD history ({n} shown){suffix}",
                n = commits.len()
            ));
        }

        if commits.is_empty() {
            lines.push("(No commits)".to_string());
        } else {
            for commit in commits {
                lines.push(format!("{}  {}", commit.short_id, commit.subject));

                let mut meta = format!("  {} • {}", commit.date, commit.author);
                let decorations = commit.decorations.trim();
                if !decorations.is_empty() {
                    meta.push_str(" • ");
                    meta.push_str(decorations);
                }
                lines.push(meta);

                let body = commit.body.trim_end();
                let body = body.trim_start_matches(['\n', '\r']);
                if !body.trim().is_empty() {
                    const MAX_BODY_LINES_PER_COMMIT: usize = 40;

                    let mut iter = body.lines();
                    for line in iter.by_ref().take(MAX_BODY_LINES_PER_COMMIT) {
                        if line.is_empty() {
                            lines.push("    ".to_string());
                        } else {
                            lines.push(format!("    {line}"));
                        }
                    }

                    if iter.next().is_some() {
                        lines.push("    …".to_string());
                    }
                }
            }
        }

        app.data.ui.set_commits_content(lines.join("\n"));

        if app.data.active_tab == Tab::Commits {
            app.data
                .ui
                .set_commits_last_seen_hash_for_agent(agent_id, commits_hash);
            app.data.ui.commits_has_unseen_changes = false;
        } else {
            app.data.ui.commits_has_unseen_changes = commits_hash != 0
                && commits_hash != app.data.ui.commits_last_seen_hash_for_agent(agent_id);
        }

        Ok(())
    }

    /// Update commits digest (hash + unseen flag) without rebuilding the full commits view.
    ///
    /// # Errors
    ///
    /// Returns an error if digest computation fails unexpectedly.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn update_commits_digest(self, app: &mut App) -> Result<()> {
        const MAX_COMMITS: usize = 200;

        let Some(agent) = app.selected_agent() else {
            app.data.ui.commits_hash = 0;
            app.data.ui.commits_has_unseen_changes = false;
            return Ok(());
        };

        let agent_id = agent.id;
        let worktree_path = agent.worktree_path.clone();
        let branch_name = agent.branch.clone();

        if !worktree_path.exists() || git::open_repository(&worktree_path).is_err() {
            app.data.ui.commits_hash = 0;
            app.data.ui.commits_has_unseen_changes = false;
            return Ok(());
        }

        let base_branch = Self::detect_base_branch(&worktree_path, &branch_name);

        let range = format!("{base_branch}..HEAD");

        let (commit_ids, _used_range, _truncated) =
            git_log_commit_ids(&worktree_path, Some(&range), MAX_COMMITS)
                .or_else(|_| git_log_commit_ids(&worktree_path, None, MAX_COMMITS))?;

        let commits_hash = hash_commit_ids(commit_ids.iter().map(String::as_str));
        app.data.ui.commits_hash = commits_hash;
        app.data.ui.commits_has_unseen_changes = commits_hash != 0
            && commits_hash != app.data.ui.commits_last_seen_hash_for_agent(agent_id);

        Ok(())
    }
}

fn preview_target(app: &App, agent: &crate::agent::Agent) -> String {
    agent.window_index.map_or_else(
        || agent.mux_session.clone(),
        |window_idx| {
            let agent_id = agent.id;
            let root = app.data.storage.root_ancestor(agent_id);
            let root_session = root.unwrap_or(agent).mux_session.clone();
            SessionManager::window_target(&root_session, window_idx)
        },
    )
}

fn set_preview_message(app: &mut App, message: &str) {
    app.data.ui.set_preview_content(message.to_string());
    app.data.ui.preview_cursor_position = None;
    app.data.ui.preview_pane_size = None;
}

#[derive(Debug)]
struct CommitInfo {
    id: String,
    short_id: String,
    date: String,
    author: String,
    subject: String,
    decorations: String,
    body: String,
}

fn git_log_commit_ids(
    worktree_path: &std::path::Path,
    range: Option<&str>,
    max_commits: usize,
) -> Result<(Vec<String>, bool, bool)> {
    // Fetch `max_commits + 1` lines so we can detect truncation without an extra command.
    let max_plus_one = max_commits.saturating_add(1);
    let max_plus_one = u32::try_from(max_plus_one).unwrap_or(u32::MAX);

    let mut cmd = crate::git::git_command();
    cmd.args([
        "log",
        "--no-color",
        "--format=%H",
        "-n",
        &max_plus_one.to_string(),
    ])
    .current_dir(worktree_path);

    let used_range = range.is_some_and(|range| {
        cmd.arg(range);
        true
    });

    let output = cmd.output()?;
    if !output.status.success() {
        anyhow::bail!("git log failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut ids: Vec<String> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();

    let truncated = ids.len() > max_commits;
    if truncated {
        ids.truncate(max_commits);
    }

    Ok((ids, used_range, truncated))
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn git_log_rich(
    worktree_path: &std::path::Path,
    range: Option<&str>,
    max_commits: usize,
) -> Result<(Vec<CommitInfo>, bool, bool)> {
    const FIELD_SEP: char = '\u{001F}';
    const RECORD_SEP: char = '\u{001E}';

    // Fetch `max_commits + 1` lines so we can detect truncation without an extra command.
    let max_plus_one = max_commits.saturating_add(1);
    let max_plus_one = u32::try_from(max_plus_one).unwrap_or(u32::MAX);

    let mut cmd = crate::git::git_command();
    cmd.args([
        "log",
        "--no-color",
        "--decorate=short",
        "--date=format:%Y-%m-%d %H:%M",
        &format!("--format=%H{FIELD_SEP}%h{FIELD_SEP}%ad{FIELD_SEP}%an{FIELD_SEP}%s{FIELD_SEP}%D{FIELD_SEP}%b{RECORD_SEP}"),
        "-n",
        &max_plus_one.to_string(),
    ])
    .current_dir(worktree_path);

    let used_range = range.is_some_and(|range| {
        cmd.arg(range);
        true
    });

    let output = cmd.output()?;
    if !output.status.success() {
        anyhow::bail!("git log failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commits: Vec<CommitInfo> = Vec::new();
    for record in stdout.split(RECORD_SEP) {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }

        let mut parts = record.splitn(7, FIELD_SEP);
        let Some(id) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };

        let Some(short_id) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };

        let Some(date) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };

        let Some(author) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };

        let Some(subject) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };

        let decorations = parts.next().unwrap_or_default();
        let body = parts.next().unwrap_or_default();

        commits.push(CommitInfo {
            id: id.to_string(),
            short_id: short_id.to_string(),
            date: date.to_string(),
            author: author.to_string(),
            subject: subject.to_string(),
            decorations: decorations.to_string(),
            body: body.to_string(),
        });
    }

    let truncated = commits.len() > max_commits;
    if truncated {
        commits.truncate(max_commits);
    }

    Ok((commits, used_range, truncated))
}

fn hash_commit_ids<'a>(ids: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut count = 0u64;

    for id in ids {
        count = count.saturating_add(1);
        id.hash(&mut hasher);
    }

    if count == 0 {
        return 0;
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use base64::Engine as _;
    use git2::{Repository, RepositoryInitOptions, Signature};
    use interprocess::local_socket::traits::Stream as _;
    use std::fs;
    #[cfg(not(windows))]
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn create_test_app() -> App {
        App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        )
    }

    #[derive(Debug)]
    struct MockMuxConfig {
        session_exists: bool,
        read_output_responses: std::collections::VecDeque<crate::mux::MuxResponse>,
        capture_visible: String,
        capture_history: String,
        capture_full_history: String,
        pane_size: (u16, u16),
        cursor_position: (u16, u16, bool),
        observed_requests: Vec<crate::mux::MuxRequest>,
    }

    fn make_mock_mux_socket(dir: &TempDir) -> (String, interprocess::local_socket::Name<'static>) {
        #[cfg(windows)]
        {
            use interprocess::local_socket::GenericNamespaced;
            use interprocess::local_socket::prelude::*;

            let display = format!("tenex-mux-preview-test-{}", uuid::Uuid::new_v4());
            let name = display
                .clone()
                .to_ns_name::<GenericNamespaced>()
                .unwrap()
                .into_owned();
            (display, name)
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
                .unwrap()
                .into_owned();
            (display, name)
        }
    }

    fn mux_output_chunk(start: u64, end: u64, data: &[u8]) -> crate::mux::MuxResponse {
        crate::mux::MuxResponse::OutputChunk {
            start,
            end,
            data_b64: base64::engine::general_purpose::STANDARD.encode(data),
        }
    }

    fn mux_output_reset(start: u64, checkpoint: &[u8]) -> crate::mux::MuxResponse {
        crate::mux::MuxResponse::OutputReset {
            start,
            checkpoint_b64: base64::engine::general_purpose::STANDARD.encode(checkpoint),
        }
    }

    fn expect_mux_error_message(
        response: crate::mux::MuxResponse,
    ) -> Result<String, Box<dyn std::error::Error>> {
        match response {
            crate::mux::MuxResponse::Err { message } => Ok(message),
            other => Err(format!("expected Err response, got {other:?}").into()),
        }
    }

    fn spawn_mock_mux_server(
        name: interprocess::local_socket::Name<'static>,
        config: Arc<Mutex<MockMuxConfig>>,
        expected_requests: usize,
    ) -> std::thread::JoinHandle<()> {
        use interprocess::local_socket::ListenerOptions;
        use interprocess::local_socket::traits::ListenerExt;

        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("Expected mock mux listener to start");

        std::thread::spawn(move || {
            let mut handled = 0usize;
            for mut stream in listener.incoming().flatten() {
                while handled < expected_requests {
                    let Ok(request) = crate::mux::read_json::<crate::mux::MuxRequest>(&mut stream)
                    else {
                        break;
                    };

                    let response = {
                        let mut config = config
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        config.observed_requests.push(request.clone());

                        match request {
                            crate::mux::MuxRequest::Ping => crate::mux::MuxResponse::Pong {
                                version: "mock".to_string(),
                            },
                            crate::mux::MuxRequest::SessionExists { .. } => {
                                crate::mux::MuxResponse::Bool {
                                    value: config.session_exists,
                                }
                            }
                            crate::mux::MuxRequest::ReadOutput { .. } => config
                                .read_output_responses
                                .pop_front()
                                .unwrap_or_else(|| crate::mux::MuxResponse::Err {
                                    message: "mock: unexpected read_output".to_string(),
                                }),
                            crate::mux::MuxRequest::Capture { kind, .. } => {
                                let text = match kind {
                                    crate::mux::CaptureKind::Visible => {
                                        config.capture_visible.clone()
                                    }
                                    crate::mux::CaptureKind::History { .. } => {
                                        config.capture_history.clone()
                                    }
                                    crate::mux::CaptureKind::FullHistory => {
                                        config.capture_full_history.clone()
                                    }
                                };
                                crate::mux::MuxResponse::Text { text }
                            }
                            crate::mux::MuxRequest::PaneSize { .. } => {
                                crate::mux::MuxResponse::Size {
                                    cols: config.pane_size.0,
                                    rows: config.pane_size.1,
                                }
                            }
                            crate::mux::MuxRequest::CursorPosition { .. } => {
                                crate::mux::MuxResponse::Position {
                                    x: config.cursor_position.0,
                                    y: config.cursor_position.1,
                                    hidden: config.cursor_position.2,
                                }
                            }
                            other => crate::mux::MuxResponse::Err {
                                message: format!("mock: unsupported request {other:?}"),
                            },
                        }
                    };

                    let _ = crate::mux::write_json(&mut stream, &response);
                    handled = handled.saturating_add(1);
                }

                if handled >= expected_requests {
                    break;
                }
            }
        })
    }

    struct MockMuxServerGuard(Option<std::thread::JoinHandle<()>>);

    impl MockMuxServerGuard {
        fn new(handle: std::thread::JoinHandle<()>) -> Self {
            Self(Some(handle))
        }
    }

    impl Drop for MockMuxServerGuard {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take() {
                let _ = handle.join();
            }
        }
    }

    #[test]
    fn test_mock_mux_server_guard_drop_noop_when_none() {
        let guard = MockMuxServerGuard(None);
        drop(guard);
    }

    #[cfg(not(windows))]
    fn write_git_override_script(dir: &TempDir, script_body: &str) -> PathBuf {
        let path = dir.path().join("git");
        fs::write(&path, script_body).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[test]
    fn test_update_preview_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_preview(&mut app).unwrap();
        assert!(app.data.ui.preview_content.contains("No agent selected"));
    }

    #[test]
    fn test_update_diff_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_diff(&mut app).unwrap();
        assert!(app.data.ui.diff_content.contains("No agent selected"));
    }

    #[test]
    fn test_update_preview_with_agent_no_session() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "nonexistent-session".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.update_preview(&mut app).unwrap();
        assert!(app.data.ui.preview_content.contains("Session not running"));
    }

    #[test]
    fn test_update_diff_with_agent_no_worktree() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with non-existent worktree
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));

        handler.update_diff(&mut app).unwrap();
        assert!(app.data.ui.diff_content.contains("Worktree not found"));
    }

    #[test]
    fn test_update_diff_with_agent_valid_worktree() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Create a temp directory (not a git repo)
        let temp_dir = TempDir::new().unwrap();

        // Add an agent with valid worktree path (but not git repo)
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        handler.update_diff(&mut app).unwrap();
        assert!(app.data.ui.diff_content.contains("Not a git repository"));
    }

    #[test]
    fn test_update_commits_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains("No agent selected"));
        assert_eq!(app.data.ui.commits_hash, 0);
    }

    #[test]
    fn test_update_commits_with_agent_no_worktree() {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains("Worktree not found"));
        assert_eq!(app.data.ui.commits_hash, 0);
    }

    #[test]
    fn test_update_commits_with_agent_non_git_worktree() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains("Not a git repository"));
        assert_eq!(app.data.ui.commits_hash, 0);
    }

    #[test]
    fn test_update_commits_includes_description_body() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        // Initial commit on master
        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let first_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        // Create and checkout feature branch
        let first = repo.find_commit(first_commit).unwrap();
        repo.branch("tenex/test", &first, false).unwrap();
        repo.set_head("refs/heads/tenex/test").unwrap();

        // Commit with body description
        fs::write(&file_path, "hello world\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let parents = [&first];
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add world\n\nThis adds world.\n\nThis adds world to greeting.",
            &tree,
            &parents,
        )
        .unwrap();

        // Wire into app as selected agent
        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/does-not-exist".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains("This adds world."));
        assert!(
            app.data
                .ui
                .commits_content
                .contains("This adds world to greeting.")
        );
        assert!(app.data.ui.commits_content.contains("\n    \n"));
    }

    #[test]
    fn test_update_commits_falls_back_to_head_history_when_base_range_invalid() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("trunk");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/trunk").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/does-not-exist".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains("HEAD history"));
    }

    #[test]
    fn test_update_commits_truncates_long_body() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let first_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        let first = repo.find_commit(first_commit).unwrap();
        repo.branch("tenex/test", &first, false).unwrap();
        repo.set_head("refs/heads/tenex/test").unwrap();

        fs::write(&file_path, "hello world\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();

        let body = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let message = format!("Big body\n\n{body}");
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&first])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains('…'));
    }

    #[test]
    fn test_update_commits_shows_truncated_suffix_when_limit_is_small() {
        use tempfile::TempDir;

        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let base_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        let base = repo.find_commit(base_commit).unwrap();
        repo.branch("tenex/test", &base, false).unwrap();
        repo.set_head("refs/heads/tenex/test").unwrap();

        let mut parent = base;
        for idx in 0..2usize {
            fs::write(&file_path, format!("change {idx}\n")).unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("file.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let new_commit = repo
                .commit(Some("HEAD"), &sig, &sig, "Change", &tree, &[&parent])
                .unwrap();
            parent = repo.find_commit(new_commit).unwrap();
        }

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        Actions::update_commits_with_limit(&mut app, 1).unwrap();
        assert!(app.data.ui.commits_content.contains("(truncated)"));
    }

    #[test]
    fn test_update_commits_shows_no_commits_when_range_empty() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "master".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app).unwrap();
        assert!(app.data.ui.commits_content.contains("(No commits)"));
    }

    #[test]
    fn test_update_commits_sets_unseen_when_not_in_commits_tab() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let first_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        let first = repo.find_commit(first_commit).unwrap();
        repo.branch("tenex/test", &first, false).unwrap();
        repo.set_head("refs/heads/tenex/test").unwrap();

        fs::write(&file_path, "hello world\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Change", &tree, &[&first])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Preview;

        handler.update_commits(&mut app).unwrap();
        assert_ne!(app.data.ui.commits_hash, 0);
        assert!(app.data.ui.commits_has_unseen_changes);
    }

    #[test]
    fn test_update_commits_digest_sets_unseen_when_hash_changes() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let first_commit = repo
            .commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        let first = repo.find_commit(first_commit).unwrap();
        repo.branch("tenex/test", &first, false).unwrap();
        repo.set_head("refs/heads/tenex/test").unwrap();

        fs::write(&file_path, "hello world\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Change", &tree, &[&first])
            .unwrap();

        let agent = Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
        );
        let agent_id = agent.id;
        app.data.storage = Storage::default();
        app.data.storage.add(agent);
        app.data.active_tab = crate::app::Tab::Preview;

        handler.update_commits_digest(&mut app).unwrap();
        assert_ne!(app.data.ui.commits_hash, 0);
        assert!(app.data.ui.commits_has_unseen_changes);

        app.data
            .ui
            .set_commits_last_seen_hash_for_agent(agent_id, app.data.ui.commits_hash);

        handler.update_commits_digest(&mut app).unwrap();
        assert!(!app.data.ui.commits_has_unseen_changes);
    }

    #[test]
    fn test_update_commits_digest_falls_back_when_range_git_log_fails() {
        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("trunk");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/trunk").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            // Pick a branch name that does not exist in this repo so base branch detection
            // falls through to the default candidates and yields a missing range.
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Preview;

        handler.update_commits_digest(&mut app).unwrap();
        assert_ne!(app.data.ui.commits_hash, 0);
        assert!(app.data.ui.commits_has_unseen_changes);
    }

    #[test]
    fn test_update_commits_digest_returns_early_when_worktree_is_not_git() {
        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Preview;

        handler.update_commits_digest(&mut app).unwrap();
        assert_eq!(app.data.ui.commits_hash, 0);
        assert!(!app.data.ui.commits_has_unseen_changes);
    }

    #[test]
    fn test_update_commits_digest_returns_early_when_worktree_missing() {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));
        app.data.active_tab = crate::app::Tab::Preview;

        handler.update_commits_digest(&mut app).unwrap();
        assert_eq!(app.data.ui.commits_hash, 0);
        assert!(!app.data.ui.commits_has_unseen_changes);
    }

    #[test]
    fn test_update_commits_digest_sets_hash_zero_when_range_is_empty() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "master".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Preview;

        handler.update_commits_digest(&mut app).unwrap();
        assert_eq!(app.data.ui.commits_hash, 0);
        assert!(!app.data.ui.commits_has_unseen_changes);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_git_log_commit_ids_bails_when_git_log_fails() {
        let dir = TempDir::new().unwrap();
        let script = write_git_override_script(&dir, "#!/bin/sh\nexit 1\n");

        let err = crate::git::with_git_program_override_for_tests(script, || {
            git_log_commit_ids(dir.path(), None, 10).unwrap_err()
        });
        let err_text = format!("{err:#}");
        assert!(err_text.contains("git log failed"));
    }

    #[test]
    fn test_git_log_commit_ids_propagates_spawn_errors() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("missing-git");
        let _err = crate::git::with_git_program_override_for_tests(missing, || {
            git_log_commit_ids(dir.path(), None, 10).unwrap_err()
        });
    }

    #[test]
    fn test_git_log_rich_propagates_spawn_errors() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("missing-git");
        let _err = crate::git::with_git_program_override_for_tests(missing, || {
            git_log_rich(dir.path(), None, 10).unwrap_err()
        });
    }

    #[cfg(not(windows))]
    #[test]
    fn test_git_log_commit_ids_truncates_output() {
        let dir = TempDir::new().unwrap();
        let script = write_git_override_script(&dir, "#!/bin/sh\nprintf 'one\\ntwo\\n'\nexit 0\n");

        let (ids, used_range, truncated) =
            crate::git::with_git_program_override_for_tests(script, || {
                git_log_commit_ids(dir.path(), None, 1).unwrap()
            });

        assert_eq!(ids, vec!["one".to_string()]);
        assert!(!used_range);
        assert!(truncated);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_git_log_rich_skips_records_with_missing_fields() {
        let dir = TempDir::new().unwrap();
        let script = write_git_override_script(
            &dir,
            concat!(
                "#!/bin/sh\n",
                "printf '\\037short\\037date\\037author\\037subject\\037dec\\037body\\036'\n",
                "printf 'id\\037\\037date\\037author\\037subject\\037dec\\037body\\036'\n",
                "printf 'id\\037short\\037\\037author\\037subject\\037dec\\037body\\036'\n",
                "printf 'id\\037short\\037date\\037\\037subject\\037dec\\037body\\036'\n",
                "printf 'id\\037short\\037date\\037author\\037\\037dec\\037body\\036'\n",
                "printf 'good\\037g\\0372026-03-29 09:00\\037Alice\\037Subject\\037\\037Body\\036'\n",
                "exit 0\n",
            ),
        );

        let (commits, used_range, truncated) =
            crate::git::with_git_program_override_for_tests(script, || {
                git_log_rich(dir.path(), None, 10).unwrap()
            });

        assert_eq!(commits.len(), 1);
        assert!(!used_range);
        assert!(!truncated);
        assert_eq!(commits[0].id, "good");
        assert_eq!(commits[0].short_id, "g");
        assert_eq!(commits[0].author, "Alice");
        assert_eq!(commits[0].subject, "Subject");
        assert_eq!(commits[0].body, "Body");
    }

    #[cfg(not(windows))]
    #[test]
    fn test_git_log_rich_truncates_commits() {
        let dir = TempDir::new().unwrap();
        let script = write_git_override_script(
            &dir,
            concat!(
                "#!/bin/sh\n",
                "printf 'a\\037a\\0372026-03-29 09:00\\037Alice\\037One\\037\\037\\036'\n",
                "printf 'b\\037b\\0372026-03-29 09:01\\037Bob\\037Two\\037\\037\\036'\n",
                "exit 0\n",
            ),
        );

        let (commits, used_range, truncated) =
            crate::git::with_git_program_override_for_tests(script, || {
                git_log_rich(dir.path(), None, 1).unwrap()
            });

        assert_eq!(commits.len(), 1);
        assert!(!used_range);
        assert!(truncated);
        assert_eq!(commits[0].id, "a");
    }

    #[test]
    fn test_diff_unseen_dot_only_shows_changes_since_last_view_per_agent() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        fs::write(&file_path, "hello world\n").unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.storage.add(Agent::new(
            "b".to_string(),
            "claude".to_string(),
            "muster/b".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Diff;
        handler.update_diff(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app).unwrap();
        assert!(!app.data.ui.diff_has_unseen_changes);

        app.data.select_next();
        app.data.select_prev();

        handler.update_diff_digest(&mut app).unwrap();
        assert!(!app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_preview_child_agent_window_target() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/child".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 1,
                repo_root: None,
            },
        );
        app.data.storage.add(child);

        let agent = app.selected_agent().unwrap();
        assert!(agent.window_index.is_none());

        app.data.select_next();
        let agent = app.selected_agent().unwrap();
        assert!(agent.window_index.is_some());

        handler.update_preview(&mut app).unwrap();
        assert!(app.data.ui.preview_content.contains("Session not running"));
    }

    #[test]
    fn test_update_diff_sets_unseen_when_not_viewing_diff_tab() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        fs::write(&file_path, "hello world\n").unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_diff_digest_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_diff_digest(&mut app).unwrap();
        assert_eq!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_diff_digest_missing_worktree() {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));

        handler.update_diff_digest(&mut app).unwrap();
        assert_eq!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_diff_digest_sets_unseen_when_not_viewing_diff_tab() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        fs::write(&file_path, "hello world\n").unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_diff_digest_sets_unseen_after_viewing_diff() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        fs::write(&file_path, "hello world\n").unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        // Viewing diff marks the current hash as "seen".
        app.data.active_tab = crate::app::Tab::Diff;
        handler.update_diff(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);

        // Make another change and ensure digest marks it as unseen while in preview.
        fs::write(&file_path, "hello again\n").unwrap();

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_diff_digest_errors_when_worktree_is_not_git_repo() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();
        let worktree_path = temp_dir.path().join("not-a-repo");
        fs::create_dir_all(&worktree_path).unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            worktree_path,
        ));

        let err = handler.update_diff_digest(&mut app).unwrap_err();
        assert!(err.to_string().contains("Failed to open git repository"));
    }

    #[test]
    fn test_update_diff_digest_errors_when_diff_marker_fails() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("bare.git");
        Repository::init_bare(&repo_path).unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            repo_path,
        ));

        let err = handler.update_diff_digest(&mut app).unwrap_err();
        assert!(
            err.to_string()
                .contains("Failed to get repository status for diff marker")
        );
    }

    #[test]
    fn test_update_diff_digest_clears_unseen_when_marker_is_zero() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app).unwrap();
        assert_eq!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_diff_digest_clears_unseen_when_marker_matches_last_seen() {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts).unwrap();
        repo.set_head("refs/heads/master").unwrap();

        let sig = Signature::now("Test", "test@test.com").unwrap();
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        fs::write(&file_path, "hello world\n").unwrap();

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Diff;
        handler.update_diff(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app).unwrap();
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
    }

    #[test]
    fn test_update_preview_streamed_handles_reset_and_resize() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: {
                let mut responses = std::collections::VecDeque::new();
                responses.push_back(mux_output_reset(0, b"checkpoint\n"));
                responses.push_back(mux_output_chunk(0, 6, b"hello\n"));
                responses
            },
            capture_visible: String::new(),
            capture_history: String::new(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 4);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();

        let handler = Actions::new();
        let mut app = create_test_app();
        app.data.ui.preview_dimensions = Some((80, 10));
        app.data.storage.add(Agent::new(
            "preview".to_string(),
            "echo".to_string(),
            "muster/preview".to_string(),
            socket_dir.path().to_path_buf(),
        ));
        let session = app
            .selected_agent()
            .expect("Expected selected agent")
            .mux_session
            .clone();

        handler.update_preview(&mut app).unwrap();
        app.data.ui.preview_dimensions = Some((81, 11));
        handler.update_preview(&mut app).unwrap();

        let vt = app.data.ui.preview_vt_by_target.get(&session).unwrap();
        assert_eq!(vt.dims, (81, 11));
    }

    #[test]
    fn test_update_preview_streamed_covers_empty_and_max_chunks() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: {
                let mut responses = std::collections::VecDeque::new();
                responses.push_back(mux_output_chunk(0, 1, &vec![b'a'; 64 * 1024]));
                responses.push_back(mux_output_chunk(1, 1, b""));
                responses
            },
            capture_visible: String::new(),
            capture_history: String::new(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 4);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();

        let handler = Actions::new();
        let mut app = create_test_app();
        app.data.ui.preview_dimensions = Some((80, 10));
        app.data.ui.preview_follow = true;
        app.data.storage.add(Agent::new(
            "preview".to_string(),
            "echo".to_string(),
            "muster/preview".to_string(),
            socket_dir.path().to_path_buf(),
        ));

        handler.update_preview(&mut app).unwrap();
        assert!(!app.data.ui.preview_content.is_empty());

        handler.update_preview(&mut app).unwrap();
        assert!(!app.data.ui.preview_content.is_empty());
    }

    #[test]
    fn test_update_preview_falls_back_to_capture_when_stream_chunk_rewinds() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let capture_history = "captured history\n".to_string();
        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: {
                let mut responses = std::collections::VecDeque::new();
                responses.push_back(mux_output_chunk(0, 5, b"rewind\n"));
                responses
            },
            capture_visible: String::new(),
            capture_history: capture_history.clone(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 5);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();

        let handler = Actions::new();
        let mut app = create_test_app();
        app.data.ui.preview_dimensions = Some((80, 10));
        app.data.storage.add(Agent::new(
            "preview".to_string(),
            "echo".to_string(),
            "muster/preview".to_string(),
            socket_dir.path().to_path_buf(),
        ));

        let session = app
            .selected_agent()
            .expect("Expected selected agent")
            .mux_session
            .clone();
        let mut vt = crate::app::state::PreviewVtState::new(session.clone(), 80, 10);
        vt.after = 10;
        app.data.ui.preview_vt_by_target.insert(session, vt);

        handler.update_preview(&mut app).unwrap();
        assert_eq!(app.data.ui.preview_content, capture_history);

        let observed = config
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observed_requests
            .clone();
        assert!(
            observed
                .iter()
                .any(|req| matches!(req, crate::mux::MuxRequest::ReadOutput { .. }))
        );
        assert!(
            observed
                .iter()
                .any(|req| matches!(req, crate::mux::MuxRequest::Capture { .. }))
        );
    }

    #[test]
    fn test_mock_mux_server_handles_decode_error_then_ping() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: std::collections::VecDeque::new(),
            capture_visible: String::new(),
            capture_history: String::new(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name.clone(), Arc::clone(&config), 1);
        let _server_guard = MockMuxServerGuard::new(server);

        {
            let _stream = interprocess::local_socket::Stream::connect(socket_name).unwrap();
        }

        crate::mux::set_socket_override(&socket_display).unwrap();
        let version = crate::mux::running_daemon_version().unwrap();
        assert_eq!(version, Some("mock".to_string()));
    }

    #[test]
    fn test_mock_mux_server_defaults_read_output_to_error_when_queue_empty() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: std::collections::VecDeque::new(),
            capture_visible: String::new(),
            capture_history: String::new(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 1);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();
        let err = crate::mux::OutputStream::new()
            .read_output("session", 0, 1024)
            .unwrap_err();
        let err_text = format!("{err:#}");
        let ok = err_text.contains("mock: unexpected read_output");
        assert!(ok);
    }

    #[test]
    fn test_mock_mux_server_handles_visible_capture_requests() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: std::collections::VecDeque::new(),
            capture_visible: "visible\n".to_string(),
            capture_history: String::new(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 1);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();
        let text = crate::mux::OutputCapture::new()
            .capture_pane("session")
            .unwrap();
        assert_eq!(text, "visible\n");
    }

    #[test]
    fn test_mock_mux_server_returns_error_for_unsupported_request() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (_socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: std::collections::VecDeque::new(),
            capture_visible: String::new(),
            capture_history: String::new(),
            capture_full_history: String::new(),
            pane_size: (80, 24),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name.clone(), Arc::clone(&config), 1);
        let _server_guard = MockMuxServerGuard::new(server);

        let mut stream = interprocess::local_socket::Stream::connect(socket_name).unwrap();
        crate::mux::write_json(&mut stream, &crate::mux::MuxRequest::ListSessions).unwrap();
        let response = crate::mux::read_json::<crate::mux::MuxResponse>(&mut stream).unwrap();
        let message = expect_mux_error_message(response).unwrap();
        assert!(message.starts_with("mock: unsupported request"));
    }

    #[test]
    fn test_expect_mux_error_message_returns_error_for_non_error_response() {
        let response = crate::mux::MuxResponse::Pong {
            version: "mock".to_string(),
        };

        let err = expect_mux_error_message(response).expect_err("expected error response");
        assert!(err.to_string().contains("expected Err response"));
    }

    #[test]
    fn test_update_preview_falls_back_to_capture_history_when_stream_errors() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: {
                let mut responses = std::collections::VecDeque::new();
                responses.push_back(crate::mux::MuxResponse::Err {
                    message: "boom".to_string(),
                });
                responses
            },
            capture_visible: String::new(),
            capture_history: "captured history".to_string(),
            capture_full_history: String::new(),
            pane_size: (90, 30),
            cursor_position: (7, 9, true),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 5);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();

        let handler = Actions::new();
        let mut app = create_test_app();
        app.data.ui.preview_dimensions = Some((90, 30));
        app.data.storage.add(Agent::new(
            "preview".to_string(),
            "echo".to_string(),
            "muster/preview".to_string(),
            socket_dir.path().to_path_buf(),
        ));
        let session = app
            .selected_agent()
            .expect("Expected selected agent")
            .mux_session
            .clone();

        handler.update_preview(&mut app).unwrap();
        assert_eq!(app.data.ui.preview_content, "captured history");
        assert_eq!(app.data.ui.preview_cursor_position, Some((7, 9, true)));
        assert_eq!(app.data.ui.preview_pane_size, Some((90, 30)));
        assert_eq!(app.data.ui.preview_scroll, usize::MAX);
        assert!(!app.data.ui.preview_vt_by_target.contains_key(&session));
    }

    #[test]
    fn test_update_preview_switching_to_full_history_preserves_scroll_distance() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket_dir = TempDir::new().unwrap();
        let (socket_display, socket_name) = make_mock_mux_socket(&socket_dir);

        let new_content = (0..20)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let config = Arc::new(Mutex::new(MockMuxConfig {
            session_exists: true,
            read_output_responses: {
                let mut responses = std::collections::VecDeque::new();
                responses.push_back(crate::mux::MuxResponse::Err {
                    message: "boom".to_string(),
                });
                responses
            },
            capture_visible: String::new(),
            capture_history: String::new(),
            capture_full_history: new_content.clone(),
            pane_size: (80, 3),
            cursor_position: (0, 0, false),
            observed_requests: Vec::new(),
        }));

        let server = spawn_mock_mux_server(socket_name, Arc::clone(&config), 5);
        let _server_guard = MockMuxServerGuard::new(server);

        crate::mux::set_socket_override(&socket_display).unwrap();

        let handler = Actions::new();
        let mut app = create_test_app();
        app.data.ui.preview_dimensions = Some((80, 3));
        app.data.ui.preview_follow = false;
        app.data.ui.preview_scroll = 2;

        let old_content = (0..10)
            .map(|idx| format!("old-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.data.ui.set_preview_content(old_content);

        app.data.storage.add(Agent::new(
            "preview".to_string(),
            "echo".to_string(),
            "muster/preview".to_string(),
            socket_dir.path().to_path_buf(),
        ));

        handler.update_preview(&mut app).unwrap();
        assert_eq!(app.data.ui.preview_content, new_content);
        assert_eq!(app.data.ui.preview_scroll, 12);
    }

    #[test]
    fn test_update_diff_sets_error_message_when_diff_generation_fails() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().join("bare.git");
        Repository::init_bare(&repo_path).unwrap();

        app.data.storage.add(Agent::new(
            "diff".to_string(),
            "echo".to_string(),
            "muster/diff".to_string(),
            repo_path,
        ));

        handler.update_diff(&mut app).unwrap();
        assert!(app.data.ui.diff_content.contains("Failed to generate diff"));
        assert!(app.data.ui.diff_model.is_none());
        assert_eq!(app.data.ui.diff_hash, 0);
    }
}
