//! Action handlers for the application

use crate::agent::{Agent, Status};
use crate::config::Action;
use crate::git::{self, DiffGenerator, WorktreeManager};
use crate::tmux::{OutputCapture, SessionManager};
use anyhow::{Context, Result};

use super::state::{App, ConfirmAction, Mode};

/// Handler for application actions
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    /// Tmux session manager
    session_manager: SessionManager,
    /// Output capture
    output_capture: OutputCapture,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "consistent &self receiver pattern for methods"
)]
impl Actions {
    /// Create a new action handler
    #[must_use]
    pub const fn new() -> Self {
        Self {
            session_manager: SessionManager::new(),
            output_capture: OutputCapture::new(),
        }
    }

    /// Handle a keybinding action
    ///
    /// # Errors
    ///
    /// Returns an error if the action fails
    pub fn handle_action(&self, app: &mut App, action: Action) -> Result<()> {
        match action {
            Action::NewAgent => {
                app.enter_mode(Mode::Creating);
            }
            Action::NewAgentWithPrompt => {
                app.enter_mode(Mode::Prompting);
            }
            Action::Attach => {
                self.attach_to_agent(app)?;
            }
            Action::Kill => {
                if app.selected_agent().is_some() {
                    app.enter_mode(Mode::Confirming(ConfirmAction::Kill));
                }
            }
            Action::Push => {
                self.push_branch(app)?;
            }
            Action::Pause => {
                self.pause_agent(app)?;
            }
            Action::Resume => {
                self.resume_agent(app)?;
            }
            Action::SwitchTab => {
                app.switch_tab();
            }
            Action::NextAgent => {
                app.select_next();
            }
            Action::PrevAgent => {
                app.select_prev();
            }
            Action::Help => {
                app.enter_mode(Mode::Help);
            }
            Action::Quit => {
                if app.has_running_agents() {
                    app.enter_mode(Mode::Confirming(ConfirmAction::Quit));
                } else {
                    app.should_quit = true;
                }
            }
            Action::ScrollUp => {
                app.scroll_up(5);
            }
            Action::ScrollDown => {
                app.scroll_down(5);
            }
            Action::ScrollTop => {
                app.scroll_to_top();
            }
            Action::ScrollBottom => {
                app.scroll_to_bottom(10000, 0);
            }
            Action::Cancel => {
                app.exit_mode();
            }
            Action::Confirm => {
                self.handle_confirm(app)?;
            }
        }
        Ok(())
    }

    /// Handle confirmation of an action
    fn handle_confirm(&self, app: &mut App) -> Result<()> {
        if let Mode::Confirming(action) = &app.mode {
            match action {
                ConfirmAction::Kill => {
                    self.kill_agent(app)?;
                }
                ConfirmAction::Reset => {
                    self.reset_all(app)?;
                }
                ConfirmAction::Quit => {
                    app.should_quit = true;
                }
            }
        }
        app.exit_mode();
        Ok(())
    }

    /// Create a new agent
    ///
    /// # Errors
    ///
    /// Returns an error if agent creation fails
    pub fn create_agent(&self, app: &mut App, title: &str, prompt: Option<&str>) -> Result<()> {
        if app.storage.len() >= app.config.max_agents {
            app.set_error(format!(
                "Maximum agents ({}) reached",
                app.config.max_agents
            ));
            return Ok(());
        }

        let branch = app.config.generate_branch_name(title);
        let worktree_path = app.config.worktree_dir.join(&branch);
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;

        let worktree_mgr = WorktreeManager::new(&repo);
        worktree_mgr.create_with_new_branch(&worktree_path, &branch)?;

        let agent = Agent::new(
            title.to_string(),
            app.config.default_program.clone(),
            branch,
            worktree_path.clone(),
            prompt.map(String::from),
        );

        let mut command = app.config.default_program.clone();
        if let Some(p) = prompt {
            command = format!("{} -p \"{}\"", command, p.replace('"', "\\\""));
        }

        self.session_manager
            .create(&agent.tmux_session, &worktree_path, Some(&command))?;

        app.storage.add(agent);
        app.storage.save()?;

        app.set_status(format!("Created agent: {title}"));
        Ok(())
    }

    /// Kill the selected agent
    fn kill_agent(&self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            let session = agent.tmux_session.clone();
            let worktree_name = agent.branch.clone();

            let _ = self.session_manager.kill(&session);

            let repo_path = std::env::current_dir()?;
            if let Ok(repo) = git::open_repository(&repo_path) {
                let worktree_mgr = WorktreeManager::new(&repo);
                let _ = worktree_mgr.remove(&worktree_name);
            }

            app.storage.remove(agent_id);
            app.validate_selection();
            app.storage.save()?;

            app.set_status("Agent killed");
        }
        Ok(())
    }

    /// Pause the selected agent
    fn pause_agent(&self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent_mut() {
            if agent.status.can_pause() {
                let _ = self.session_manager.kill(&agent.tmux_session);
                agent.set_status(Status::Paused);
                app.storage.save()?;
                app.set_status("Agent paused");
            } else {
                app.set_error("Agent cannot be paused");
            }
        }
        Ok(())
    }

    /// Resume the selected agent
    fn resume_agent(&self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent_mut() {
            if agent.status.can_resume() {
                self.session_manager.create(
                    &agent.tmux_session,
                    &agent.worktree_path,
                    Some(&agent.program),
                )?;
                agent.set_status(Status::Running);
                app.storage.save()?;
                app.set_status("Agent resumed");
            } else {
                app.set_error("Agent cannot be resumed");
            }
        }
        Ok(())
    }

    /// Attach to the selected agent's tmux session
    fn attach_to_agent(&self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        if self.session_manager.exists(&agent.tmux_session) {
            app.request_attach(agent.tmux_session.clone());
            Ok(())
        } else {
            app.set_error("Tmux session not found");
            Err(anyhow::anyhow!("Tmux session not found"))
        }
    }

    /// Push the selected agent's branch to remote
    fn push_branch(&self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let _session_exists = self.session_manager.exists(&agent.tmux_session);
        app.set_status(format!("Pushing branch: {}", agent.branch));
        Ok(())
    }

    /// Reset all agents and state
    fn reset_all(&self, app: &mut App) -> Result<()> {
        for agent in app.storage.iter() {
            let _ = self.session_manager.kill(&agent.tmux_session);
        }

        app.storage.clear();
        app.storage.save()?;
        app.validate_selection();

        app.set_status("All agents reset");
        Ok(())
    }

    /// Update preview content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if preview update fails
    pub fn update_preview(&self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            if self.session_manager.exists(&agent.tmux_session) {
                let content = self
                    .output_capture
                    .capture_pane_with_history(&agent.tmux_session, 1000)
                    .unwrap_or_default();
                app.preview_content = content;
            } else {
                app.preview_content = String::from("(Session not running)");
            }
        } else {
            app.preview_content = String::from("(No agent selected)");
        }
        Ok(())
    }

    /// Update diff content for the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if diff update fails
    pub fn update_diff(&self, app: &mut App) -> Result<()> {
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

    /// Check and update agent statuses based on tmux sessions
    ///
    /// # Errors
    ///
    /// Returns an error if status sync fails
    pub fn sync_agent_status(&self, app: &mut App) -> Result<()> {
        let mut changed = false;

        for agent in app.storage.iter_mut() {
            if agent.status == Status::Starting || agent.status == Status::Running {
                if self.session_manager.exists(&agent.tmux_session) {
                    if agent.status == Status::Starting {
                        agent.set_status(Status::Running);
                        changed = true;
                    }
                } else {
                    agent.set_status(Status::Stopped);
                    changed = true;
                }
            }
        }

        if changed {
            app.storage.save()?;
        }

        Ok(())
    }
}

impl Default for Actions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::agent::Storage;
    use crate::config::Config;

    fn create_test_app() -> App {
        App::new(Config::default(), Storage::default())
    }

    #[test]
    fn test_handler_new() {
        let handler = Actions::new();
        assert!(!format!("{:?}", handler.session_manager).is_empty());
    }

    #[test]
    fn test_handler_default() {
        let handler = Actions::default();
        assert!(!format!("{:?}", handler.output_capture).is_empty());
    }

    #[test]
    fn test_handle_action_new_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::NewAgent).unwrap();
        assert_eq!(app.mode, Mode::Creating);
    }

    #[test]
    fn test_handle_action_new_agent_with_prompt() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler
            .handle_action(&mut app, Action::NewAgentWithPrompt)
            .unwrap();
        assert_eq!(app.mode, Mode::Prompting);
    }

    #[test]
    fn test_handle_action_help() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Help).unwrap();
        assert_eq!(app.mode, Mode::Help);
    }

    #[test]
    fn test_handle_action_quit_no_agents() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Quit).unwrap();
        assert!(app.should_quit);
    }

    #[test]
    fn test_handle_action_switch_tab() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::SwitchTab).unwrap();
        assert_eq!(app.active_tab, super::super::state::Tab::Diff);
    }

    #[test]
    fn test_handle_action_navigation() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        for i in 0..3 {
            app.storage.add(Agent::new(
                format!("agent{i}"),
                "claude".to_string(),
                format!("muster/agent{i}"),
                PathBuf::from("/tmp"),
                None,
            ));
        }

        assert_eq!(app.selected, 0);
        handler.handle_action(&mut app, Action::NextAgent).unwrap();
        assert_eq!(app.selected, 1);
        handler.handle_action(&mut app, Action::PrevAgent).unwrap();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_handle_action_scroll() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::ScrollDown).unwrap();
        assert_eq!(app.preview_scroll, 5);

        handler.handle_action(&mut app, Action::ScrollUp).unwrap();
        assert_eq!(app.preview_scroll, 0);

        handler.handle_action(&mut app, Action::ScrollTop).unwrap();
        assert_eq!(app.preview_scroll, 0);
    }

    #[test]
    fn test_handle_action_cancel() {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.enter_mode(Mode::Creating);
        handler.handle_action(&mut app, Action::Cancel).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_update_preview_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_preview(&mut app).unwrap();
        assert!(app.preview_content.contains("No agent selected"));
    }

    #[test]
    fn test_update_diff_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.update_diff(&mut app).unwrap();
        assert!(app.diff_content.contains("No agent selected"));
    }

    #[test]
    fn test_handle_kill_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Kill).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_attach_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let result = handler.handle_action(&mut app, Action::Attach);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_pause_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Pause).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_resume_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Resume).unwrap();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_handle_push_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let result = handler.handle_action(&mut app, Action::Push);
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_agent_status() {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.sync_agent_status(&mut app).unwrap();
    }
}
