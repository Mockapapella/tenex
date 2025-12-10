//! Preview operations: update preview and diff content

use crate::git::{self, DiffGenerator};
use crate::tmux::SessionManager;
use anyhow::Result;

use super::Actions;
use crate::app::state::App;

impl Actions {
    /// Update preview content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if preview update fails
    pub fn update_preview(self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            // Determine the tmux target (session or specific window)
            let tmux_target = if let Some(window_idx) = agent.window_index {
                // Child agent: target specific window within root's session
                let agent_id = agent.id;
                let root = app.storage.root_ancestor(agent_id);
                let root_session =
                    root.map_or_else(|| agent.tmux_session.clone(), |r| r.tmux_session.clone());
                SessionManager::window_target(&root_session, window_idx)
            } else {
                // Root agent: use session directly
                agent.tmux_session.clone()
            };

            if self.session_manager.exists(&agent.tmux_session) {
                let content = self
                    .output_capture
                    .capture_pane_with_history(&tmux_target, 1000)
                    .unwrap_or_default();
                app.preview_content = content;
            } else {
                app.preview_content = String::from("(Session not running)");
            }
        } else {
            app.preview_content = String::from("(No agent selected)");
        }

        // Auto-scroll to bottom only if follow mode is enabled
        // (disabled when user manually scrolls up, re-enabled when they scroll to bottom)
        if app.preview_follow {
            app.preview_scroll = usize::MAX;
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

                    app.diff_content = content;
                } else {
                    app.diff_content = String::from("(Not a git repository)");
                }
            } else {
                app.diff_content = String::from("(Worktree not found)");
            }
        } else {
            app.diff_content = String::from("(No agent selected)");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::config::Config;
    use std::path::PathBuf;

    fn create_test_app() -> App {
        App::new(Config::default(), Storage::default())
    }

    #[test]
    fn test_update_preview_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_preview(&mut app)?;
        assert!(app.preview_content.contains("No agent selected"));
        Ok(())
    }

    #[test]
    fn test_update_diff_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_diff(&mut app)?;
        assert!(app.diff_content.contains("No agent selected"));
        Ok(())
    }

    #[test]
    fn test_update_preview_with_agent_no_session() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "nonexistent-session".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        handler.update_preview(&mut app)?;
        assert!(app.preview_content.contains("Session not running"));
        Ok(())
    }

    #[test]
    fn test_update_diff_with_agent_no_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with non-existent worktree
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/nonexistent/path"),
            None,
        ));

        handler.update_diff(&mut app)?;
        assert!(app.diff_content.contains("Worktree not found"));
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
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            temp_dir.path().to_path_buf(),
            None,
        ));

        handler.update_diff(&mut app)?;
        assert!(app.diff_content.contains("Not a git repository"));
        Ok(())
    }
}
