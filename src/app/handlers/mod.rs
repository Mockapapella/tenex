//! Action handlers for the application
//!
//! This module contains the `Actions` struct and all action handlers,
//! organized into focused submodules by domain.

mod agent_lifecycle;
mod broadcast;
mod git_ops;
mod preview;
mod swarm;
mod sync;
mod window;

use crate::config::Action;
use crate::git::{self, WorktreeManager};
use crate::mux::{OutputCapture, SessionManager};
use anyhow::{Context, Result};

use super::state::{App, ConfirmAction, ConfirmKind, Mode, OverlayMode, Tab, TextInputKind};

/// Handler for application actions
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    /// Mux session manager
    pub(crate) session_manager: SessionManager,
    /// Output capture
    pub(crate) output_capture: OutputCapture,
}

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
    pub fn handle_action(self, app: &mut App, action: Action) -> Result<()> {
        match action {
            // Mode entry actions
            Action::NewAgent => app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
                TextInputKind::Creating,
            ))),
            Action::NewAgentWithPrompt => app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
                TextInputKind::Prompting,
            ))),
            Action::Help => {
                app.ui.help_scroll = 0;
                app.enter_mode(Mode::Overlay(OverlayMode::Help));
            }
            Action::CommandPalette => app.start_command_palette(),
            Action::Cancel => app.exit_mode(),
            Action::Confirm => self.handle_confirm(app)?,

            // Navigation actions
            Action::SwitchTab => app.switch_tab(),
            Action::NextAgent => app.select_next(),
            Action::PrevAgent => app.select_prev(),

            // Scroll actions
            Action::ScrollUp => app.scroll_up(5),
            Action::ScrollDown => app.scroll_down(5),
            Action::ScrollTop => app.scroll_to_top(),
            Action::ScrollBottom => app.scroll_to_bottom(10000, 0),

            // Preview actions
            Action::FocusPreview => Self::handle_focus_preview(app),
            Action::UnfocusPreview => Self::handle_unfocus_preview(app),

            // Agent lifecycle actions
            Action::Kill => Self::handle_kill_action(app),
            Action::Quit => Self::handle_quit_action(app),
            Action::ToggleCollapse => self.toggle_collapse(app)?,

            // Spawning actions
            Action::SpawnChildren => app.start_spawning_root(),
            Action::PlanSwarm => app.start_planning_swarm(),
            Action::AddChildren => Self::handle_add_children(app),
            Action::Synthesize => Self::handle_synthesize(app),
            Action::ReviewSwarm => Self::start_review_swarm(app)?,
            Action::SpawnTerminal => self.handle_spawn_terminal(app)?,
            Action::SpawnTerminalPrompted => Self::handle_spawn_terminal_prompted(app),
            Action::Broadcast => Self::handle_broadcast(app),

            // Git operations
            Action::Push => Self::push_branch(app)?,
            Action::RenameBranch => Self::rename_agent(app)?,
            Action::OpenPR => Self::open_pr_flow(app)?,
            Action::Rebase => Self::rebase_branch(app)?,
            Action::Merge => Self::merge_branch(app)?,
        }
        Ok(())
    }

    fn handle_focus_preview(app: &mut App) {
        if app.selected_agent().is_some() {
            app.active_tab = Tab::Preview;
            app.enter_mode(Mode::PreviewFocused);
        }
    }

    fn handle_unfocus_preview(app: &mut App) {
        if app.mode == Mode::PreviewFocused {
            app.exit_mode();
        }
    }

    fn handle_kill_action(app: &mut App) {
        if app.selected_agent().is_some() {
            app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
                ConfirmAction::Kill,
            ))));
        }
    }

    fn handle_quit_action(app: &mut App) {
        if app.has_running_agents() {
            app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
                ConfirmAction::Quit,
            ))));
        } else {
            app.should_quit = true;
        }
    }

    fn handle_add_children(app: &mut App) {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            app.start_spawning_under(agent_id);
        }
    }

    fn handle_synthesize(app: &mut App) {
        if let Some(agent) = app.selected_agent() {
            if app.storage.has_children(agent.id) {
                app.enter_mode(Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(
                    ConfirmAction::Synthesize,
                ))));
            } else {
                app.set_error("Selected agent has no children to synthesize");
            }
        }
    }

    fn handle_spawn_terminal(self, app: &mut App) -> Result<()> {
        if app.selected_agent().is_some() {
            self.spawn_terminal(app, None)?;
        }
        Ok(())
    }

    fn handle_spawn_terminal_prompted(app: &mut App) {
        if app.selected_agent().is_some() {
            app.start_terminal_prompt();
        }
    }

    fn handle_broadcast(app: &mut App) {
        if app.selected_agent().is_some() {
            app.enter_mode(Mode::Overlay(OverlayMode::TextInput(
                TextInputKind::Broadcasting,
            )));
        }
    }

    /// Handle confirmation of an action
    fn handle_confirm(self, app: &mut App) -> Result<()> {
        if let Mode::Overlay(OverlayMode::Confirm(ConfirmKind::Action(action))) = &app.mode {
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
                ConfirmAction::Synthesize => {
                    self.synthesize(app)?;
                }
                ConfirmAction::WorktreeConflict => {
                    // This is handled separately in the TUI with R/D keys
                    // If we get here, just exit mode (like pressing Esc)
                }
            }
        }
        // Only exit mode if we're not showing an error modal
        // (error modal is set by operations like synthesize when they fail)
        if !matches!(app.mode, Mode::Overlay(OverlayMode::Error(_))) {
            app.exit_mode();
        }
        Ok(())
    }

    /// Start the review swarm flow
    ///
    /// If no agent is selected, shows an info popup.
    /// If an agent is selected, fetches branches and enters review mode.
    fn start_review_swarm(app: &mut App) -> Result<()> {
        // Check if an agent with a worktree is selected
        let selected = app.selected_agent();
        if selected.is_none() {
            app.show_review_info();
            return Ok(());
        }

        // Store the selected agent's ID for later use
        let agent_id = selected.map(|a| a.id);
        app.spawn.spawning_under = agent_id;

        // Fetch branches for the selector
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        // Convert from git::BranchInfo to app::BranchInfo (they're the same type via re-export)
        app.start_review(branches);
        Ok(())
    }

    /// Toggle collapse state of the selected agent
    ///
    /// # Errors
    ///
    /// Returns an error if toggle fails
    pub fn toggle_collapse(self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            if app.storage.has_children(agent_id)
                && let Some(agent) = app.storage.get_mut(agent_id)
            {
                agent.collapsed = !agent.collapsed;
                app.storage.save()?;
            }
        }
        Ok(())
    }

    /// Reset all agents and state
    fn reset_all(self, app: &mut App) -> Result<()> {
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path).ok();

        for agent in app.storage.iter() {
            let _ = self.session_manager.kill(&agent.mux_session);

            if let Some(ref repo) = repo {
                let worktree_mgr = WorktreeManager::new(repo);
                let _ = worktree_mgr.remove(&agent.branch);
            }
        }

        app.storage.clear();
        app.storage.save()?;
        app.validate_selection();

        app.set_status("All agents reset");
        Ok(())
    }
}

impl Default for Actions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
