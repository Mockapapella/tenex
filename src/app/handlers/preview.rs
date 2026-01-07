//! Preview operations: update preview and diff content

use crate::app::{App, Tab};
use crate::git::{self, DiffGenerator};
use crate::mux::SessionManager;
use crate::state::AppMode;
use anyhow::Result;

use super::Actions;

impl Actions {
    /// Update preview content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if preview update fails
    pub fn update_preview(self, app: &mut App) -> Result<()> {
        const HISTORY_LINES_DEFAULT: u32 = 1000;
        // When actively watching the preview and following the output, keep the history window
        // smaller so we can refresh more frequently without stuttering.
        const HISTORY_LINES_FOLLOWING: u32 = 300;

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
                let content = if matches!(&app.mode, AppMode::PreviewFocused(_)) {
                    self.output_capture
                        .capture_pane(&target)
                        .unwrap_or_default()
                } else {
                    let history_lines =
                        if app.data.active_tab == Tab::Preview && app.data.ui.preview_follow {
                            HISTORY_LINES_FOLLOWING
                        } else {
                            HISTORY_LINES_DEFAULT
                        };
                    self.output_capture
                        .capture_pane_with_history(&target, history_lines)
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

        // Auto-scroll to bottom only if follow mode is enabled
        // (disabled when user manually scrolls up, re-enabled when they scroll to bottom)
        if app.data.ui.preview_follow {
            app.data.ui.preview_scroll = usize::MAX;
        }

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
            None,
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
            None,
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
            None,
        ));

        handler.update_diff(&mut app)?;
        assert!(app.data.ui.diff_content.contains("Not a git repository"));
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
            None,
        ));
        app.data.storage.add(Agent::new(
            "b".to_string(),
            "claude".to_string(),
            "muster/b".to_string(),
            temp_dir.path().to_path_buf(),
            None,
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
            None,
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
            None,
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
            None,
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
            None,
        ));

        handler.update_diff_digest(&mut app)?;
        assert_eq!(app.data.ui.diff_hash, 0);
        assert!(!app.data.ui.diff_has_unseen_changes);
        Ok(())
    }
}
