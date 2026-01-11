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

        let old_line_count = app.data.ui.preview_content.lines().count();
        let old_scroll = app.data.ui.preview_scroll;
        let visible_height = app
            .data
            .ui
            .preview_dimensions
            .map_or(20, |(_, h)| usize::from(h));

        // When the user manually scrolls up, stop using a short tail buffer to avoid
        // the viewport "jumping" as the tail window slides.
        let wants_full_history = !app.data.ui.preview_follow;
        let switching_to_full_history =
            wants_full_history && !app.data.ui.preview_using_full_history;

        if let Some(agent) = app.selected_agent() {
            // Determine the target (session or specific window)
            let target = if let Some(window_idx) = agent.window_index {
                // Child agent: target specific window within root's session
                let agent_id = agent.id;
                let root = app.data.storage.root_ancestor(agent_id);
                let root_session =
                    root.map_or_else(|| agent.mux_session.clone(), |r| r.mux_session.clone());
                SessionManager::window_target(&root_session, window_idx)
            } else {
                // Root agent: use session directly
                agent.mux_session.clone()
            };

            if self.session_manager.exists(&agent.mux_session) {
                let content = if wants_full_history {
                    self.output_capture
                        .capture_full_history(&target)
                        .unwrap_or_default()
                } else {
                    self.output_capture
                        .capture_pane_with_history(&target, HISTORY_LINES_FOLLOWING)
                        .unwrap_or_default()
                };
                app.data.ui.preview_content = content;
                app.data.ui.preview_cursor_position =
                    self.output_capture.cursor_position(&target).ok();
                app.data.ui.preview_pane_size = self.output_capture.pane_size(&target).ok();
            } else {
                app.data.ui.preview_content = String::from("(Session not running)");
                app.data.ui.preview_cursor_position = None;
                app.data.ui.preview_pane_size = None;
            }
        } else {
            app.data.ui.preview_content = String::from("(No agent selected)");
            app.data.ui.preview_cursor_position = None;
            app.data.ui.preview_pane_size = None;
        }

        // If we just switched from a short tail buffer to full history, preserve the user's
        // scroll position relative to the bottom of the buffer. Without this, the viewport
        // can appear to "jump" far up because the top of the buffer gained many lines.
        if switching_to_full_history {
            let new_line_count = app.data.ui.preview_content.lines().count();

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

    /// Update diff content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if diff update fails
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

                    app.data.ui.diff_hash = model.hash;
                    app.data.ui.diff_model = Some(model.clone());

                    let (content, meta) = app.data.ui.build_diff_view(&model);
                    app.data.ui.set_diff_view(content, meta);

                    if app.data.active_tab == Tab::Diff {
                        app.data
                            .ui
                            .set_diff_last_seen_hash_for_agent(agent_id, model.hash);
                        app.data.ui.diff_has_unseen_changes = false;
                    } else {
                        app.data.ui.diff_has_unseen_changes = model.hash != 0
                            && model.hash != app.data.ui.diff_last_seen_hash_for_agent(agent_id);
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
                let digest = diff_gen.uncommitted_digest()?;

                app.data.ui.diff_hash = digest.hash;
                app.data.ui.diff_has_unseen_changes = digest.hash != 0
                    && digest.hash != app.data.ui.diff_last_seen_hash_for_agent(agent_id);
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
        const MAX_COMMITS: usize = 200;

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
            git_log_rich(&worktree_path, Some(&range), MAX_COMMITS)
                .or_else(|_| git_log_rich(&worktree_path, None, MAX_COMMITS))?;

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
        let Some(id) = parts.next() else {
            continue;
        };

        let Some(short_id) = parts.next() else {
            continue;
        };

        let Some(date) = parts.next() else {
            continue;
        };

        let Some(author) = parts.next() else {
            continue;
        };

        let Some(subject) = parts.next() else {
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
    use git2::{Repository, RepositoryInitOptions, Signature};
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        App::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        )
    }

    #[test]
    fn test_update_preview_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_preview(&mut app)?;
        assert!(app.data.ui.preview_content.contains("No agent selected"));
        Ok(())
    }

    #[test]
    fn test_update_diff_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_diff(&mut app)?;
        assert!(app.data.ui.diff_content.contains("No agent selected"));
        Ok(())
    }

    #[test]
    fn test_update_preview_with_agent_no_session() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "nonexistent-session".to_string(),
            PathBuf::from("/tmp"),
        ));

        handler.update_preview(&mut app)?;
        assert!(app.data.ui.preview_content.contains("Session not running"));
        Ok(())
    }

    #[test]
    fn test_update_diff_with_agent_no_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with non-existent worktree
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));

        handler.update_diff(&mut app)?;
        assert!(app.data.ui.diff_content.contains("Worktree not found"));
        Ok(())
    }

    #[test]
    fn test_update_diff_with_agent_valid_worktree() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Create a temp directory (not a git repo)
        let temp_dir = TempDir::new()?;

        // Add an agent with valid worktree path (but not git repo)
        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        handler.update_diff(&mut app)?;
        assert!(app.data.ui.diff_content.contains("Not a git repository"));
        Ok(())
    }

    #[test]
    fn test_update_commits_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_commits(&mut app)?;
        assert!(app.data.ui.commits_content.contains("No agent selected"));
        assert_eq!(app.data.ui.commits_hash, 0);
        Ok(())
    }

    #[test]
    fn test_update_commits_with_agent_no_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app)?;
        assert!(app.data.ui.commits_content.contains("Worktree not found"));
        assert_eq!(app.data.ui.commits_hash, 0);
        Ok(())
    }

    #[test]
    fn test_update_commits_with_agent_non_git_worktree() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new()?;

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app)?;
        assert!(app.data.ui.commits_content.contains("Not a git repository"));
        assert_eq!(app.data.ui.commits_hash, 0);
        Ok(())
    }

    #[test]
    fn test_update_commits_includes_description_body() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");

        // Initial commit on master
        fs::write(&file_path, "hello\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let first_commit = repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        // Create and checkout feature branch
        let first = repo.find_commit(first_commit)?;
        repo.branch("tenex/test", &first, false)?;
        repo.set_head("refs/heads/tenex/test")?;

        // Commit with body description
        fs::write(&file_path, "hello world\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Add world\n\nThis adds world to greeting.",
            &tree,
            &[&first],
        )?;

        // Wire into app as selected agent
        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/does-not-exist".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app)?;
        assert!(
            app.data
                .ui
                .commits_content
                .contains("This adds world to greeting.")
        );
        Ok(())
    }

    #[test]
    fn test_update_commits_falls_back_to_head_history_when_base_range_invalid()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("trunk");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/trunk")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])?;

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/does-not-exist".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app)?;
        assert!(app.data.ui.commits_content.contains("HEAD history"));
        Ok(())
    }

    #[test]
    fn test_update_commits_truncates_long_body() -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let first_commit = repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])?;

        let first = repo.find_commit(first_commit)?;
        repo.branch("tenex/test", &first, false)?;
        repo.set_head("refs/heads/tenex/test")?;

        fs::write(&file_path, "hello world\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        let body = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let message = format!("Big body\n\n{body}");
        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&first])?;

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app)?;
        assert!(app.data.ui.commits_content.contains('…'));
        Ok(())
    }

    #[test]
    fn test_update_commits_shows_no_commits_when_range_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])?;

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "master".to_string(),
            temp_dir.path().to_path_buf(),
        ));
        app.data.active_tab = crate::app::Tab::Commits;

        handler.update_commits(&mut app)?;
        assert!(app.data.ui.commits_content.contains("(No commits)"));
        Ok(())
    }

    #[test]
    fn test_update_commits_digest_sets_unseen_when_hash_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();
        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");

        fs::write(&file_path, "hello\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        let first_commit = repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])?;

        let first = repo.find_commit(first_commit)?;
        repo.branch("tenex/test", &first, false)?;
        repo.set_head("refs/heads/tenex/test")?;

        fs::write(&file_path, "hello world\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Change", &tree, &[&first])?;

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

        handler.update_commits_digest(&mut app)?;
        assert_ne!(app.data.ui.commits_hash, 0);
        assert!(app.data.ui.commits_has_unseen_changes);

        app.data
            .ui
            .set_commits_last_seen_hash_for_agent(agent_id, app.data.ui.commits_hash);

        handler.update_commits_digest(&mut app)?;
        assert!(!app.data.ui.commits_has_unseen_changes);
        Ok(())
    }

    #[test]
    fn test_diff_unseen_dot_only_shows_changes_since_last_view_per_agent()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        fs::write(&file_path, "hello world\n")?;

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
        handler.update_diff(&mut app)?;
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app)?;
        assert!(!app.data.ui.diff_has_unseen_changes);

        app.data.select_next();
        app.data.select_prev();

        handler.update_diff_digest(&mut app)?;
        assert!(!app.data.ui.diff_has_unseen_changes);

        Ok(())
    }

    #[test]
    fn test_update_preview_child_agent_window_target() -> Result<(), Box<dyn std::error::Error>> {
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
            },
        );
        app.data.storage.add(child);

        app.data.select_next();
        assert!(matches!(
            app.selected_agent(),
            Some(agent) if agent.window_index.is_some()
        ));

        handler.update_preview(&mut app)?;
        assert!(app.data.ui.preview_content.contains("Session not running"));
        Ok(())
    }

    #[test]
    fn test_update_diff_sets_unseen_when_not_viewing_diff_tab()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        fs::write(&file_path, "hello world\n")?;

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff(&mut app)?;
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(app.data.ui.diff_has_unseen_changes);

        Ok(())
    }

    #[test]
    fn test_update_diff_digest_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_diff_digest(&mut app)?;
        assert_eq!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
        Ok(())
    }

    #[test]
    fn test_update_diff_digest_missing_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
        ));

        handler.update_diff_digest(&mut app)?;
        assert_eq!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
        Ok(())
    }

    #[test]
    fn test_update_diff_digest_sets_unseen_when_not_viewing_diff_tab()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        fs::write(&file_path, "hello world\n")?;

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app)?;
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(app.data.ui.diff_has_unseen_changes);

        Ok(())
    }

    #[test]
    fn test_update_diff_digest_sets_unseen_after_viewing_diff()
    -> Result<(), Box<dyn std::error::Error>> {
        use tempfile::TempDir;

        let handler = Actions::new();
        let mut app = create_test_app();

        let temp_dir = TempDir::new()?;

        let mut init_opts = RepositoryInitOptions::new();
        init_opts.initial_head("master");
        let repo = Repository::init_opts(temp_dir.path(), &init_opts)?;
        repo.set_head("refs/heads/master")?;

        let sig = Signature::now("Test", "test@test.com")?;
        let file_path = temp_dir.path().join("file.txt");
        fs::write(&file_path, "hello\n")?;

        let mut index = repo.index()?;
        index.add_path(Path::new("file.txt"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])?;

        fs::write(&file_path, "hello world\n")?;

        app.data.storage = Storage::default();
        app.data.storage.add(Agent::new(
            "a".to_string(),
            "claude".to_string(),
            "muster/a".to_string(),
            temp_dir.path().to_path_buf(),
        ));

        // Viewing diff marks the current hash as "seen".
        app.data.active_tab = crate::app::Tab::Diff;
        handler.update_diff(&mut app)?;
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);

        // Make another change and ensure digest marks it as unseen while in preview.
        fs::write(&file_path, "hello again\n")?;

        app.data.active_tab = crate::app::Tab::Preview;
        handler.update_diff_digest(&mut app)?;
        assert_ne!(app.data.ui.diff_hash, 0);
        assert!(app.data.ui.diff_has_unseen_changes);

        Ok(())
    }
}
