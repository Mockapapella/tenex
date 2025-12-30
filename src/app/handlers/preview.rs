//! Preview operations: update preview and diff content

use crate::git::{self, DiffGenerator};
use crate::mux::SessionManager;
use crate::state::AppMode;
use anyhow::Result;

use super::Actions;
use crate::app::App;

impl Actions {
    /// Update preview content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if preview update fails
    pub fn update_preview(self, app: &mut App) -> Result<()> {
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
                    self.output_capture
                        .capture_pane_with_history(&target, 1000)
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
            if agent.worktree_path.exists() {
                if let Ok(repo) = git::open_repository(&agent.worktree_path) {
                    let diff_gen = DiffGenerator::new(&repo);
                    let files = diff_gen.uncommitted().unwrap_or_default();

                    let mut content = String::new();
                    for file in files {
                        content.push_str(&file.to_string_colored());
                        content.push('\n');
                    }

                    if content.is_empty() {
                        content = String::from("(No changes)");
                    }

                    app.data.ui.set_diff_content(content);
                } else {
                    app.data.ui.set_diff_content("(Not a git repository)");
                }
            } else {
                app.data.ui.set_diff_content("(Worktree not found)");
            }
        } else {
            app.data.ui.set_diff_content("(No agent selected)");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::app::Settings;
    use crate::config::Config;
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
}
