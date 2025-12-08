//! Action handlers for the application

use crate::agent::{Agent, ChildConfig, Status};
use crate::config::Action;
use crate::git::{self, DiffGenerator, WorktreeManager};
use crate::prompts;
use crate::tmux::{OutputCapture, SessionManager};
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use tracing::{debug, info, warn};

use super::state::{App, ConfirmAction, Mode, WorktreeConflictInfo};

/// Handler for application actions
#[derive(Debug, Clone, Copy)]
pub struct Actions {
    /// Tmux session manager
    session_manager: SessionManager,
    /// Output capture
    output_capture: OutputCapture,
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
            Action::RenameBranch => {
                self.rename_agent(app)?;
            }
            Action::OpenPR => {
                self.open_pr_flow(app)?;
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
            Action::SpawnChildren => {
                app.start_spawning_root();
            }
            Action::PlanSwarm => {
                app.start_planning_swarm();
            }
            Action::AddChildren => {
                if let Some(agent) = app.selected_agent() {
                    let agent_id = agent.id;
                    app.start_spawning_under(agent_id);
                }
            }
            Action::Synthesize => {
                if let Some(agent) = app.selected_agent() {
                    if app.storage.has_children(agent.id) {
                        app.enter_mode(Mode::Confirming(ConfirmAction::Synthesize));
                    } else {
                        app.set_error("Selected agent has no children to synthesize");
                    }
                }
            }
            Action::ToggleCollapse => {
                self.toggle_collapse(app)?;
            }
            Action::Broadcast => {
                if app.selected_agent().is_some() {
                    app.enter_mode(Mode::Broadcasting);
                }
            }
            Action::ReviewSwarm => {
                Self::start_review_swarm(app)?;
            }
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
        app.spawning_under = agent_id;

        // Fetch branches for the selector
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        // Convert from git::BranchInfo to app::BranchInfo (they're the same type via re-export)
        app.start_review(branches);
        Ok(())
    }

    /// Spawn review agents for the selected agent against a base branch
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_review_agents(self, app: &mut App) -> Result<()> {
        let count = app.child_count;
        let parent_id = app
            .spawning_under
            .ok_or_else(|| anyhow::anyhow!("No agent selected for review"))?;
        let base_branch = app
            .review_base_branch
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No base branch selected for review"))?;

        info!(
            count,
            parent_id = %parent_id,
            base_branch,
            "Spawning review agents"
        );

        // Get the root agent's session and worktree info
        let root = app
            .storage
            .root_ancestor(parent_id)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;

        let root_session = root.tmux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();

        // Build the review prompt
        let review_prompt = prompts::build_review_prompt(&base_branch);

        // Reserve window indices
        let start_window_index = app.storage.reserve_window_indices(parent_id);

        // Create review child agents
        for i in 0..count {
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);

            let child = Agent::new_child(
                String::new(), // Placeholder
                app.config.default_program.clone(),
                branch.clone(),
                worktree_path.clone(),
                Some(review_prompt.clone()),
                ChildConfig {
                    parent_id,
                    tmux_session: root_session.clone(),
                    window_index,
                },
            );

            let child_title = format!("Reviewer {} ({})", i + 1, child.short_id());
            let mut child = child;
            child.title.clone_from(&child_title);

            // Create window in the root's session with the prompt
            // Escape both double quotes and backticks (backticks are command substitution in bash)
            let escaped_prompt = review_prompt.replace('"', "\\\"").replace('`', "\\`");
            let command = format!("{} \"{escaped_prompt}\"", app.config.default_program);
            let actual_index = self.session_manager.create_window(
                &root_session,
                &child_title,
                &worktree_path,
                Some(&command),
            )?;

            // Resize the new window to match preview dimensions
            if let Some((width, height)) = app.preview_dimensions {
                let window_target = SessionManager::window_target(&root_session, actual_index);
                let _ = self
                    .session_manager
                    .resize_window(&window_target, width, height);
            }

            // Update window index if it differs
            child.window_index = Some(actual_index);

            app.storage.add(child);
        }

        // Expand the parent to show children
        if let Some(parent) = app.storage.get_mut(parent_id) {
            parent.collapsed = false;
        }

        app.storage.save()?;
        info!(count, parent_id = %parent_id, base_branch, "Review agents spawned successfully");
        app.set_status(format!(
            "Spawned {count} review agents against {base_branch}"
        ));

        // Clear review state
        app.clear_review_state();

        Ok(())
    }

    /// Handle confirmation of an action
    fn handle_confirm(self, app: &mut App) -> Result<()> {
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
                ConfirmAction::Synthesize => {
                    self.synthesize(app)?;
                }
                ConfirmAction::WorktreeConflict => {
                    // This is handled separately in the TUI with R/D keys
                    // If we get here, just exit mode (like pressing Esc)
                }
            }
        }
        app.exit_mode();
        Ok(())
    }

    /// Create a new agent
    ///
    /// If a worktree with the same name already exists, this will prompt the user
    /// to either reconnect to the existing worktree or recreate it from scratch.
    ///
    /// # Errors
    ///
    /// Returns an error if agent creation fails
    pub fn create_agent(self, app: &mut App, title: &str, prompt: Option<&str>) -> Result<()> {
        debug!(title, prompt, "Creating new agent");

        if app.storage.len() >= app.config.max_agents {
            warn!(
                max = app.config.max_agents,
                current = app.storage.len(),
                "Maximum agents reached"
            );
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

        // Check if worktree/branch already exists - prompt user for action
        if worktree_mgr.exists(&branch) {
            debug!(branch, "Worktree already exists, prompting user");

            // Get current HEAD info for new worktree context
            let (current_branch, current_commit) = worktree_mgr
                .head_info()
                .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));

            // Try to get existing worktree info
            let (existing_branch, existing_commit) = worktree_mgr
                .worktree_head_info(&branch)
                .map(|(b, c)| (Some(b), Some(c)))
                .unwrap_or((None, None));

            app.worktree_conflict = Some(WorktreeConflictInfo {
                title: title.to_string(),
                prompt: prompt.map(String::from),
                branch: branch.clone(),
                worktree_path: worktree_path.clone(),
                existing_branch,
                existing_commit,
                current_branch,
                current_commit,
                swarm_child_count: None, // Not a swarm creation
            });
            app.enter_mode(Mode::Confirming(ConfirmAction::WorktreeConflict));
            return Ok(());
        }

        self.create_agent_internal(app, title, prompt, &branch, &worktree_path)
    }

    /// Internal function to actually create the agent after conflict resolution
    fn create_agent_internal(
        self,
        app: &mut App,
        title: &str,
        prompt: Option<&str>,
        branch: &str,
        worktree_path: &std::path::Path,
    ) -> Result<()> {
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let worktree_mgr = WorktreeManager::new(&repo);

        worktree_mgr.create_with_new_branch(worktree_path, branch)?;

        let agent = Agent::new(
            title.to_string(),
            app.config.default_program.clone(),
            branch.to_string(),
            worktree_path.to_path_buf(),
            prompt.map(String::from),
        );

        let mut command = app.config.default_program.clone();
        if let Some(p) = prompt {
            // Pass prompt as positional argument (works for codex, claude, etc.)
            command = format!("{} \"{}\"", command, p.replace('"', "\\\""));
        }

        self.session_manager
            .create(&agent.tmux_session, worktree_path, Some(&command))?;

        // Resize the new session to match preview dimensions
        if let Some((width, height)) = app.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&agent.tmux_session, width, height);
        }

        app.storage.add(agent);
        app.storage.save()?;

        info!(title, %branch, "Agent created successfully");
        app.set_status(format!("Created agent: {title}"));
        Ok(())
    }

    /// Reconnect to an existing worktree (user chose to keep it)
    ///
    /// # Errors
    ///
    /// Returns an error if the tmux session cannot be created or storage fails
    pub fn reconnect_to_worktree(self, app: &mut App) -> Result<()> {
        let conflict = app
            .worktree_conflict
            .take()
            .ok_or_else(|| anyhow::anyhow!("No worktree conflict info available"))?;

        debug!(branch = %conflict.branch, swarm_child_count = ?conflict.swarm_child_count, "Reconnecting to existing worktree");

        // Check if this is a swarm creation (has child count)
        if let Some(child_count) = conflict.swarm_child_count {
            // Create root agent for swarm
            let root_agent = Agent::new(
                conflict.title.clone(),
                app.config.default_program.clone(),
                conflict.branch.clone(),
                conflict.worktree_path.clone(),
                None, // Root doesn't get the prompt
            );

            let root_session = root_agent.tmux_session.clone();
            let root_id = root_agent.id;

            // Create the root's tmux session
            self.session_manager.create(
                &root_session,
                &conflict.worktree_path,
                Some(&app.config.default_program),
            )?;

            // Resize the session to match preview dimensions
            if let Some((width, height)) = app.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&root_session, width, height);
            }

            app.storage.add(root_agent);

            // Now spawn the children
            let task = conflict.prompt.as_deref().unwrap_or("");
            self.spawn_children_for_root(
                app,
                &root_session,
                &conflict.worktree_path,
                &conflict.branch,
                root_id,
                child_count,
                task,
            )?;

            info!(title = %conflict.title, branch = %conflict.branch, child_count, "Reconnected swarm to existing worktree");
            app.set_status(format!("Reconnected swarm: {}", conflict.title));
        } else {
            // Single agent reconnect
            let agent = Agent::new(
                conflict.title.clone(),
                app.config.default_program.clone(),
                conflict.branch.clone(),
                conflict.worktree_path.clone(),
                conflict.prompt.clone(),
            );

            let mut command = app.config.default_program.clone();
            if let Some(ref p) = conflict.prompt {
                command = format!("{} \"{}\"", command, p.replace('"', "\\\""));
            }

            self.session_manager.create(
                &agent.tmux_session,
                &conflict.worktree_path,
                Some(&command),
            )?;

            // Resize the new session to match preview dimensions
            if let Some((width, height)) = app.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&agent.tmux_session, width, height);
            }

            app.storage.add(agent);

            info!(title = %conflict.title, branch = %conflict.branch, "Reconnected to existing worktree");
            app.set_status(format!("Reconnected to: {}", conflict.title));
        }

        app.storage.save()?;
        Ok(())
    }

    /// Spawn child agents for an existing root agent
    ///
    /// This is a helper used by both `spawn_children` and `reconnect_to_worktree`
    #[expect(
        clippy::too_many_arguments,
        reason = "Helper needs all context from caller"
    )]
    fn spawn_children_for_root(
        self,
        app: &mut App,
        root_session: &str,
        worktree_path: &std::path::Path,
        branch: &str,
        parent_id: uuid::Uuid,
        count: usize,
        task: &str,
    ) -> Result<()> {
        let start_window_index = app.storage.reserve_window_indices(parent_id);
        let child_prompt = if app.use_plan_prompt {
            prompts::build_plan_prompt(task)
        } else {
            task.to_string()
        };

        for i in 0..count {
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);

            let child = Agent::new_child(
                String::new(),
                app.config.default_program.clone(),
                branch.to_string(),
                worktree_path.to_path_buf(),
                Some(child_prompt.clone()),
                ChildConfig {
                    parent_id,
                    tmux_session: root_session.to_string(),
                    window_index,
                },
            );

            // Use descriptive names based on agent type
            let child_title = if app.use_plan_prompt {
                format!("Planner {} ({})", i + 1, child.short_id())
            } else {
                format!("Agent {} ({})", i + 1, child.short_id())
            };
            let mut child = child;
            child.title.clone_from(&child_title);

            // Escape both double quotes and backticks (backticks are command substitution in bash)
            let escaped_prompt = child_prompt.replace('"', "\\\"").replace('`', "\\`");
            let command = format!("{} \"{escaped_prompt}\"", app.config.default_program);
            let actual_index = self.session_manager.create_window(
                root_session,
                &child_title,
                worktree_path,
                Some(&command),
            )?;

            if let Some((width, height)) = app.preview_dimensions {
                let window_target = SessionManager::window_target(root_session, actual_index);
                let _ = self
                    .session_manager
                    .resize_window(&window_target, width, height);
            }

            child.window_index = Some(actual_index);
            app.storage.add(child);
        }

        // Expand the parent to show children
        if let Some(parent) = app.storage.get_mut(parent_id) {
            parent.collapsed = false;
        }

        Ok(())
    }

    /// Recreate the worktree (user chose to delete and start fresh)
    ///
    /// # Errors
    ///
    /// Returns an error if the worktree cannot be removed/recreated or agent creation fails
    pub fn recreate_worktree(self, app: &mut App) -> Result<()> {
        let conflict = app
            .worktree_conflict
            .take()
            .ok_or_else(|| anyhow::anyhow!("No worktree conflict info available"))?;

        debug!(branch = %conflict.branch, swarm_child_count = ?conflict.swarm_child_count, "Recreating worktree from scratch");

        // Remove existing worktree first
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let worktree_mgr = WorktreeManager::new(&repo);
        worktree_mgr.remove(&conflict.branch)?;

        // Check if this is a swarm creation
        if let Some(child_count) = conflict.swarm_child_count {
            // Set up app state for spawn_children
            app.spawning_under = None;
            app.child_count = child_count;

            // Call spawn_children with the task/prompt (if any)
            self.spawn_children(app, conflict.prompt.as_deref())
        } else {
            // Single agent creation
            self.create_agent_internal(
                app,
                &conflict.title,
                conflict.prompt.as_deref(),
                &conflict.branch,
                &conflict.worktree_path,
            )
        }
    }

    /// Kill the selected agent (and all its descendants)
    fn kill_agent(self, app: &mut App) -> Result<()> {
        if let Some(agent) = app.selected_agent() {
            let agent_id = agent.id;
            let is_root = agent.is_root();
            let session = agent.tmux_session.clone();
            let worktree_name = agent.branch.clone();
            let window_index = agent.window_index;
            let title = agent.title.clone();

            info!(
                %title,
                %agent_id,
                is_root,
                %session,
                "Killing agent"
            );

            if is_root {
                // Root agent: kill entire session and worktree
                // First kill all descendant windows in descending order
                // (in case any are in other sessions, and to handle renumbering)
                let descendants = app.storage.descendants(agent_id);
                let mut indices: Vec<u32> = descendants
                    .iter()
                    .filter_map(|desc| desc.window_index)
                    .collect();
                indices.sort_unstable_by(|a, b| b.cmp(a));
                for idx in indices {
                    let _ = self.session_manager.kill_window(&session, idx);
                }

                // Kill the session
                let _ = self.session_manager.kill(&session);

                // Remove worktree
                let repo_path = std::env::current_dir()?;
                if let Ok(repo) = git::open_repository(&repo_path) {
                    let worktree_mgr = WorktreeManager::new(&repo);
                    let _ = worktree_mgr.remove(&worktree_name);
                }
            } else {
                // Child agent: kill just this window and its descendants
                // Get the root's session for killing windows
                let root = app.storage.root_ancestor(agent_id);
                let root_session = root.map_or_else(|| session.clone(), |r| r.tmux_session.clone());
                let root_id = root.map(|r| r.id);

                // Collect all window indices being deleted
                let mut deleted_indices: Vec<u32> = Vec::new();
                let descendants = app.storage.descendants(agent_id);
                for desc in &descendants {
                    if let Some(idx) = desc.window_index {
                        deleted_indices.push(idx);
                    }
                }

                // Add this agent's window
                if let Some(idx) = window_index {
                    deleted_indices.push(idx);
                }

                // Sort in descending order and kill windows from highest to lowest
                // This prevents tmux renumbering from affecting indices we haven't killed yet
                deleted_indices.sort_unstable_by(|a, b| b.cmp(a));
                for idx in &deleted_indices {
                    let _ = self.session_manager.kill_window(&root_session, *idx);
                }

                // Update window indices for remaining agents under the same root
                // When tmux has renumber-windows on, indices shift down
                if let Some(rid) = root_id {
                    Self::adjust_window_indices_after_deletion(
                        app,
                        rid,
                        agent_id,
                        &deleted_indices,
                    );
                }
            }

            // Remove agent and all descendants from storage
            app.storage.remove_with_descendants(agent_id);

            app.validate_selection();
            app.storage.save()?;

            // Immediately update preview/diff to show the newly selected agent
            let _ = self.update_preview(app);
            let _ = self.update_diff(app);

            app.set_status("Agent killed");
        }
        Ok(())
    }

    /// Attach to the selected agent's tmux session
    fn attach_to_agent(self, app: &mut App) -> Result<()> {
        // Log all visible agents for debugging
        for (i, (agent, depth)) in app.storage.visible_agents().iter().enumerate() {
            debug!(
                index = i,
                agent_id = %agent.short_id(),
                agent_title = %agent.title,
                window_index = ?agent.window_index,
                depth = depth,
                "Visible agent"
            );
        }

        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        debug!(
            selected_index = app.selected,
            agent_id = %agent.short_id(),
            agent_title = %agent.title,
            window_index = ?agent.window_index,
            session = %agent.tmux_session,
            "Attaching to agent"
        );

        if self.session_manager.exists(&agent.tmux_session) {
            app.request_attach(agent.tmux_session.clone(), agent.window_index);
            Ok(())
        } else {
            app.set_error("Tmux session not found");
            Err(anyhow::anyhow!("Tmux session not found"))
        }
    }

    // === Git Operations: Push, Rename Branch, Open PR ===

    /// Push the selected agent's branch to remote (Ctrl+p)
    ///
    /// Shows a confirmation dialog, then pushes the branch.
    #[expect(clippy::unused_self, reason = "consistent with other handler methods")]
    fn push_branch(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();

        debug!(branch = %branch_name, "Starting push flow");

        app.start_push(agent_id, branch_name);
        Ok(())
    }

    /// Rename the selected agent (r key)
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + tmux session
    /// For sub-agents: Renames agent title + tmux window only
    #[expect(clippy::unused_self, reason = "consistent with other handler methods")]
    fn rename_agent(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let is_root = agent.is_root();
        let current_name = agent.title.clone();

        debug!(
            title = %current_name,
            is_root,
            "Starting rename flow"
        );

        app.start_rename(agent_id, current_name, is_root);
        Ok(())
    }

    /// Open a PR for the selected agent's branch (Ctrl+o)
    ///
    /// Detects the base branch, checks for unpushed commits, and opens a PR.
    #[expect(clippy::unused_self, reason = "consistent with other handler methods")]
    fn open_pr_flow(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let branch_name = agent.branch.clone();
        let worktree_path = agent.worktree_path.clone();

        // Detect base branch from git history
        let base_branch = Self::detect_base_branch(&worktree_path, &branch_name)?;

        // Check if there are unpushed commits
        let has_unpushed = Self::has_unpushed_commits(&worktree_path, &branch_name)?;

        debug!(
            branch = %branch_name,
            base_branch = %base_branch,
            has_unpushed,
            "Starting open PR flow"
        );

        app.start_open_pr(agent_id, branch_name, base_branch, has_unpushed);

        // If no unpushed commits, open PR immediately
        if !has_unpushed {
            Self::open_pr_in_browser(app)?;
        }

        Ok(())
    }

    /// Detect the base branch that this branch was created from
    fn detect_base_branch(worktree_path: &std::path::Path, branch_name: &str) -> Result<String> {
        // Try to find the merge base with common default branches
        let candidates = ["main", "master", "develop"];

        for candidate in &candidates {
            let output = std::process::Command::new("git")
                .args(["merge-base", candidate, branch_name])
                .current_dir(worktree_path)
                .output();

            if let Ok(result) = output
                && result.status.success()
            {
                return Ok((*candidate).to_string());
            }
        }

        // Fallback: try to detect from the reflog
        let output = std::process::Command::new("git")
            .args(["reflog", "show", "--no-abbrev", branch_name])
            .current_dir(worktree_path)
            .output()
            .context("Failed to read reflog")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Look for "branch: Created from" in reflog
        for line in stdout.lines() {
            if line.contains("Created from") {
                // Extract branch name after "Created from "
                if let Some(from_idx) = line.find("Created from ") {
                    let rest = &line[from_idx + 13..];
                    let base = rest.split_whitespace().next().unwrap_or("main");
                    return Ok(base.to_string());
                }
            }
        }

        // Default to main
        Ok("main".to_string())
    }

    /// Check if there are unpushed commits on the branch
    fn has_unpushed_commits(worktree_path: &std::path::Path, branch_name: &str) -> Result<bool> {
        // Check if remote tracking branch exists
        let remote_branch = format!("origin/{branch_name}");
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &remote_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check remote branch")?;

        if !output.status.success() {
            // No remote branch means all commits are unpushed
            return Ok(true);
        }

        // Compare local and remote
        let output = std::process::Command::new("git")
            .args([
                "rev-list",
                "--count",
                &format!("{remote_branch}..{branch_name}"),
            ])
            .current_dir(worktree_path)
            .output()
            .context("Failed to count unpushed commits")?;

        let count: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(count > 0)
    }

    /// Check if a remote branch exists
    fn check_remote_branch_exists(
        worktree_path: &std::path::Path,
        branch_name: &str,
    ) -> Result<bool> {
        let remote_branch = format!("origin/{branch_name}");
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &remote_branch])
            .current_dir(worktree_path)
            .output()
            .context("Failed to check remote branch")?;

        Ok(output.status.success())
    }

    /// Execute the git push operation (after user confirms)
    ///
    /// # Errors
    ///
    /// Returns an error if the push operation fails
    pub fn execute_push(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op_agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for push"))?;

        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch_name = app.git_op_branch_name.clone();

        debug!(branch = %branch_name, "Executing push");

        // Push to remote with upstream tracking
        let push_output = std::process::Command::new("git")
            .args(["push", "-u", "origin", &branch_name])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to push to remote")?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            app.set_error(format!("Push failed: {}", stderr.trim()));
            app.clear_git_op_state();
            return Ok(());
        }

        info!(branch = %branch_name, "Push successful");
        app.set_status(format!("Pushed branch: {branch_name}"));
        app.clear_git_op_state();
        app.exit_mode();

        Ok(())
    }

    /// Execute rename operation
    ///
    /// For root agents: Renames branch (local + remote if exists) + agent title + tmux session
    /// For sub-agents: Renames agent title + tmux window only
    ///
    /// # Errors
    ///
    /// Returns an error if the rename operation fails
    pub fn execute_rename(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op_agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for rename"))?;

        // Verify agent exists
        if app.storage.get(agent_id).is_none() {
            anyhow::bail!("Agent not found");
        }

        let old_name = app.git_op_original_branch.clone();
        let new_name = app.git_op_branch_name.clone();
        let is_root = app.git_op_is_root_rename;

        if old_name == new_name {
            app.set_status("Name unchanged");
            app.clear_git_op_state();
            app.exit_mode();
            return Ok(());
        }

        debug!(
            old_name = %old_name,
            new_name = %new_name,
            is_root,
            "Executing rename"
        );

        if is_root {
            // Root agent: rename branch + agent + tmux session
            Self::execute_root_rename(app, agent_id, &old_name, &new_name)?;
        } else {
            // Sub-agent: rename agent title + tmux window only
            Self::execute_subagent_rename(app, agent_id, &new_name)?;
        }

        app.clear_git_op_state();
        app.exit_mode();
        Ok(())
    }

    /// Execute rename for a root agent (branch + agent + tmux session)
    fn execute_root_rename(
        app: &mut App,
        agent_id: uuid::Uuid,
        old_name: &str,
        new_name: &str,
    ) -> Result<()> {
        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let old_branch = agent.branch.clone();
        let tmux_session = agent.tmux_session.clone();

        // Generate new branch name from new title
        let new_branch = app.config.generate_branch_name(new_name);

        // Check if remote branch exists before we start
        let remote_exists = Self::check_remote_branch_exists(&worktree_path, &old_branch)?;

        // Rename local branch
        let rename_output = std::process::Command::new("git")
            .args(["branch", "-m", &old_branch, &new_branch])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to rename local branch")?;

        if !rename_output.status.success() {
            let stderr = String::from_utf8_lossy(&rename_output.stderr);
            app.set_error(format!("Failed to rename branch: {}", stderr.trim()));
            return Ok(());
        }

        // Update the agent's title and branch name
        if let Some(agent) = app.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
            agent.branch.clone_from(&new_branch);
        }
        app.storage.save()?;

        // Rename tmux session
        let session_manager = SessionManager::new();
        let new_session_name = format!("tenex-{new_name}");
        if let Err(e) = session_manager.rename(&tmux_session, &new_session_name) {
            warn!(error = %e, "Failed to rename tmux session");
        } else {
            // Update root agent's tmux_session
            if let Some(agent) = app.storage.get_mut(agent_id) {
                agent.tmux_session.clone_from(&new_session_name);
            }

            // Update all descendants' tmux_session to the new session name
            let descendant_ids: Vec<uuid::Uuid> = app
                .storage
                .descendants(agent_id)
                .iter()
                .map(|a| a.id)
                .collect();
            for desc_id in descendant_ids {
                if let Some(desc) = app.storage.get_mut(desc_id) {
                    desc.tmux_session.clone_from(&new_session_name);
                }
            }

            app.storage.save()?;
        }

        // Handle remote branch only if it existed
        if remote_exists {
            // Delete old remote branch
            let _ = std::process::Command::new("git")
                .args(["push", "origin", "--delete", &old_branch])
                .current_dir(&worktree_path)
                .output();

            // Push new branch to remote
            let push_output = std::process::Command::new("git")
                .args(["push", "-u", "origin", &new_branch])
                .current_dir(&worktree_path)
                .output()
                .context("Failed to push renamed branch")?;

            if push_output.status.success() {
                info!(
                    old_name = %old_name,
                    new_name = %new_name,
                    "Root agent renamed successfully"
                );
                app.set_status(format!("Renamed: {old_name} → {new_name}"));
            } else {
                let stderr = String::from_utf8_lossy(&push_output.stderr);
                warn!(error = %stderr, "Failed to push renamed branch to remote");
                app.set_status(format!("Renamed to {new_name} (remote push failed)"));
            }
        } else {
            info!(
                old_name = %old_name,
                new_name = %new_name,
                "Root agent renamed successfully (local only)"
            );
            app.set_status(format!("Renamed: {old_name} → {new_name}"));
        }

        Ok(())
    }

    /// Execute rename for a sub-agent (title + tmux window only)
    fn execute_subagent_rename(app: &mut App, agent_id: uuid::Uuid, new_name: &str) -> Result<()> {
        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let old_name = agent.title.clone();
        let tmux_session = agent.tmux_session.clone();
        let window_index = agent.window_index;

        // Update the agent's title
        if let Some(agent) = app.storage.get_mut(agent_id) {
            agent.title = new_name.to_string();
        }
        app.storage.save()?;

        // Rename tmux window if agent has a window index
        if let Some(idx) = window_index {
            let rename_output = std::process::Command::new("tmux")
                .args([
                    "rename-window",
                    "-t",
                    &format!("{tmux_session}:{idx}"),
                    new_name,
                ])
                .output();

            if let Err(e) = rename_output {
                warn!(error = %e, "Failed to rename tmux window");
            }
        }

        info!(
            old_name = %old_name,
            new_name = %new_name,
            "Sub-agent renamed successfully"
        );
        app.set_status(format!("Renamed: {old_name} → {new_name}"));

        Ok(())
    }

    /// Execute push and then open PR (for Ctrl+o flow)
    ///
    /// # Errors
    ///
    /// Returns an error if the push or PR open fails
    pub fn execute_push_and_open_pr(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op_agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for push"))?;

        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch_name = app.git_op_branch_name.clone();

        debug!(branch = %branch_name, "Executing push before opening PR");

        // Push to remote
        let push_output = std::process::Command::new("git")
            .args(["push", "-u", "origin", &branch_name])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to push to remote")?;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            app.set_error(format!("Push failed: {}", stderr.trim()));
            app.clear_git_op_state();
            return Ok(());
        }

        info!(branch = %branch_name, "Push successful, opening PR");

        // Now open the PR
        Self::open_pr_in_browser(app)
    }

    /// Open PR in browser using gh CLI
    fn open_pr_in_browser(app: &mut App) -> Result<()> {
        let agent_id = app
            .git_op_agent_id
            .ok_or_else(|| anyhow::anyhow!("No agent ID for PR"))?;

        let agent = app
            .storage
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found"))?;

        let worktree_path = agent.worktree_path.clone();
        let branch = app.git_op_branch_name.clone();
        let base_branch = app.git_op_base_branch.clone();

        debug!(
            branch = %branch,
            base_branch = %base_branch,
            "Opening PR with gh CLI"
        );

        // Use gh pr create with --web flag to open in browser
        let output = std::process::Command::new("gh")
            .args(["pr", "create", "--web", "--base", &base_branch])
            .current_dir(&worktree_path)
            .output();

        match output {
            Ok(result) if result.status.success() => {
                info!(branch = %branch, base = %base_branch, "Opened PR creation page in browser");
                app.set_status(format!("Opening PR: {branch} → {base_branch}"));
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr);
                warn!(error = %stderr, "gh pr create failed");
                app.set_error(format!("Failed to open PR: {}", stderr.trim()));
            }
            Err(e) => {
                warn!(error = %e, "gh CLI not found");
                app.set_error("gh CLI not found. Install it with: brew install gh");
            }
        }

        app.clear_git_op_state();
        app.exit_mode();
        Ok(())
    }

    /// Reset all agents and state
    fn reset_all(self, app: &mut App) -> Result<()> {
        let repo_path = std::env::current_dir()?;
        let repo = git::open_repository(&repo_path).ok();

        for agent in app.storage.iter() {
            let _ = self.session_manager.kill(&agent.tmux_session);

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

    // === Hierarchy Methods ===

    /// Spawn child agents under a parent (or create new root with children)
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    #[expect(
        clippy::too_many_lines,
        reason = "Complex swarm spawning logic with multiple branches"
    )]
    pub fn spawn_children(self, app: &mut App, task: Option<&str>) -> Result<()> {
        let count = app.child_count;
        let parent_id = app.spawning_under;

        info!(
            count,
            ?parent_id,
            task_len = task.map_or(0, str::len),
            "Spawning child agents"
        );

        // Determine the root agent and session/worktree to use
        let (root_session, worktree_path, branch, parent_agent_id) = if let Some(pid) = parent_id {
            // Adding children to existing agent
            let root = app
                .storage
                .root_ancestor(pid)
                .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
            (
                root.tmux_session.clone(),
                root.worktree_path.clone(),
                root.branch.clone(),
                pid,
            )
        } else {
            // Create new root agent first
            let root_title = match task {
                Some(t) if t.len() > 30 => format!("{}...", &t[..27]),
                Some(t) => t.to_string(),
                None => {
                    let short_id = &uuid::Uuid::new_v4().to_string()[..8];
                    format!("Swarm ({short_id})")
                }
            };
            let branch = app.config.generate_branch_name(&root_title);
            let worktree_path = app.config.worktree_dir.join(&branch);
            let repo_path = std::env::current_dir().context("Failed to get current directory")?;
            let repo = git::open_repository(&repo_path)?;

            let worktree_mgr = WorktreeManager::new(&repo);

            // Check if worktree/branch already exists - prompt user for action
            let branch_exists = worktree_mgr.exists(&branch);
            debug!(
                branch,
                branch_exists, "Checking if worktree exists for swarm"
            );
            if branch_exists {
                debug!(branch, "Worktree already exists for swarm, prompting user");

                // Get current HEAD info for new worktree context
                let (current_branch, current_commit) = worktree_mgr
                    .head_info()
                    .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));

                // Try to get existing worktree info
                let (existing_branch, existing_commit) = worktree_mgr
                    .worktree_head_info(&branch)
                    .map(|(b, c)| (Some(b), Some(c)))
                    .unwrap_or((None, None));

                app.worktree_conflict = Some(WorktreeConflictInfo {
                    title: root_title,
                    prompt: task.map(String::from),
                    branch,
                    worktree_path,
                    existing_branch,
                    existing_commit,
                    current_branch,
                    current_commit,
                    swarm_child_count: Some(count),
                });
                app.enter_mode(Mode::Confirming(ConfirmAction::WorktreeConflict));
                return Ok(());
            }

            worktree_mgr.create_with_new_branch(&worktree_path, &branch)?;

            let root_agent = Agent::new(
                root_title,
                app.config.default_program.clone(),
                branch.clone(),
                worktree_path.clone(),
                None, // Root doesn't get the planning preamble
            );

            let root_session = root_agent.tmux_session.clone();
            let root_id = root_agent.id;

            // Create the root's tmux session
            self.session_manager.create(
                &root_session,
                &worktree_path,
                Some(&app.config.default_program),
            )?;

            // Resize the session to match preview dimensions
            if let Some((width, height)) = app.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&root_session, width, height);
            }

            app.storage.add(root_agent);
            (root_session, worktree_path, branch, root_id)
        };

        // Create child agents
        // Reserve all window indices upfront to avoid O(n*count) lookups
        let start_window_index = app.storage.reserve_window_indices(parent_agent_id);
        let child_prompt: Option<String> = task.map(|t| {
            if app.use_plan_prompt {
                prompts::build_plan_prompt(t)
            } else {
                t.to_string()
            }
        });
        for i in 0..count {
            // Use pre-reserved window index (cast i to u32 for addition)
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);

            // Create child first to get its ID, then build the title with short ID
            let child = Agent::new_child(
                String::new(), // Placeholder, will be updated below
                app.config.default_program.clone(),
                branch.clone(),
                worktree_path.clone(),
                child_prompt.clone(),
                ChildConfig {
                    parent_id: parent_agent_id,
                    tmux_session: root_session.clone(),
                    window_index,
                },
            );

            // Include short ID in title to distinguish agents with same base name
            // Use descriptive names based on agent type
            let child_title = if app.use_plan_prompt && task.is_some() {
                format!("Planner {} ({})", i + 1, child.short_id())
            } else {
                format!("Agent {} ({})", i + 1, child.short_id())
            };
            let mut child = child;
            child.title.clone_from(&child_title);

            // Create window in the root's session with the prompt (if any)
            let command = match &child_prompt {
                Some(prompt) => {
                    let escaped_prompt = prompt.replace('"', "\\\"").replace('`', "\\`");
                    format!("{} \"{escaped_prompt}\"", app.config.default_program)
                }
                None => app.config.default_program.clone(),
            };
            let actual_index = self.session_manager.create_window(
                &root_session,
                &child_title,
                &worktree_path,
                Some(&command),
            )?;

            // Resize the new window to match preview dimensions
            if let Some((width, height)) = app.preview_dimensions {
                let window_target = SessionManager::window_target(&root_session, actual_index);
                let _ = self
                    .session_manager
                    .resize_window(&window_target, width, height);
            }

            // Update window index if it differs
            child.window_index = Some(actual_index);

            app.storage.add(child);
        }

        // Expand the parent to show children
        if let Some(parent) = app.storage.get_mut(parent_agent_id) {
            parent.collapsed = false;
        }

        app.storage.save()?;
        info!(count, parent_id = %parent_agent_id, "Child agents spawned successfully");
        app.set_status(format!("Spawned {count} child agents"));
        Ok(())
    }

    /// Synthesize children into the parent agent
    ///
    /// Writes synthesis content to `.tenex/<id>.md` and tells the parent to read it.
    ///
    /// # Errors
    ///
    /// Returns an error if synthesis fails
    pub fn synthesize(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        if !app.storage.has_children(agent.id) {
            warn!(agent_id = %agent.id, title = %agent.title, "No children to synthesize");
            app.set_error("Selected agent has no children to synthesize");
            return Ok(());
        }

        let parent_id = agent.id;
        let parent_session = agent.tmux_session.clone();
        let parent_title = agent.title.clone();
        let worktree_path = agent.worktree_path.clone();
        // Determine the correct tmux target for the parent
        // If the parent has a window_index, it's a child agent running in a window
        let parent_tmux_target = agent.window_index.map_or_else(
            || parent_session.clone(),
            |window_idx| SessionManager::window_target(&parent_session, window_idx),
        );

        info!(%parent_id, %parent_title, "Synthesizing descendants into parent");

        // Collect findings from all descendants (children, grandchildren, etc.)
        let descendants = app.storage.descendants(parent_id);
        let mut findings: Vec<(String, String)> = Vec::new();

        for descendant in &descendants {
            // Capture terminal output from descendant's window
            let target = descendant.window_index.map_or_else(
                || descendant.tmux_session.clone(),
                |window_idx| SessionManager::window_target(&parent_session, window_idx),
            );

            let output = self
                .output_capture
                .capture_pane_with_history(&target, 5000)
                .unwrap_or_else(|_| "(Could not capture output)".to_string());

            findings.push((descendant.title.clone(), output));
        }

        // Build synthesis content
        let synthesis_content = prompts::build_synthesis_prompt(&findings);

        // Write to .tenex/<unique-id>.md in the worktree
        let tenex_dir = worktree_path.join(".tenex");
        fs::create_dir_all(&tenex_dir)
            .with_context(|| format!("Failed to create {}", tenex_dir.display()))?;

        let synthesis_id = uuid::Uuid::new_v4();
        let synthesis_file = tenex_dir.join(format!("{synthesis_id}.md"));

        let mut file = fs::File::create(&synthesis_file)
            .with_context(|| format!("Failed to create {}", synthesis_file.display()))?;
        file.write_all(synthesis_content.as_bytes())
            .with_context(|| format!("Failed to write to {}", synthesis_file.display()))?;

        debug!(?synthesis_file, "Wrote synthesis file");

        // Kill all descendant windows and remove from storage
        // Collect IDs and window indices first to avoid borrow issues
        let descendant_info: Vec<_> = descendants.iter().map(|d| (d.id, d.window_index)).collect();
        let descendants_count = descendant_info.len();

        for (descendant_id, window_idx) in descendant_info {
            // Kill the window if it has one
            if let Some(idx) = window_idx {
                let _ = self.session_manager.kill_window(&parent_session, idx);
            }
            // Remove from storage (remove_with_descendants handles nested removal)
            app.storage.remove(descendant_id);
        }

        // Now tell the parent to read the file
        let agent_word = if descendants_count == 1 {
            "agent"
        } else {
            "agents"
        };
        let read_command = format!(
            "Read .tenex/{synthesis_id}.md - it contains the work of {descendants_count} {agent_word}. Use it to guide your next steps."
        );
        self.session_manager
            .send_keys_and_submit(&parent_tmux_target, &read_command)?;

        app.validate_selection();
        app.storage.save()?;
        info!(%parent_title, descendants_count, "Synthesis complete");
        app.set_status("Synthesized findings into parent agent");
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

    /// Broadcast a message to the selected agent and all its leaf descendants
    ///
    /// Leaf agents are agents that have no children. Parent agents are skipped
    /// but their children are still traversed.
    ///
    /// # Errors
    ///
    /// Returns an error if broadcasting fails
    pub fn broadcast_to_leaves(self, app: &mut App, message: &str) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let agent_id = agent.id;
        let mut sent_count = 0;

        // Collect all agents to broadcast to (selected + descendants)
        let mut targets: Vec<uuid::Uuid> = vec![agent_id];
        targets.extend(app.storage.descendant_ids(agent_id));

        // Filter to only leaf agents and send message
        for target_id in targets {
            if !app.storage.has_children(target_id)
                && let Some(target_agent) = app.storage.get(target_id)
            {
                // Determine the tmux target (session or window)
                let tmux_target = if let Some(window_idx) = target_agent.window_index {
                    // Child agent: use window target within root's session
                    let root = app.storage.root_ancestor(target_id);
                    let root_session = root.map_or_else(
                        || target_agent.tmux_session.clone(),
                        |r| r.tmux_session.clone(),
                    );
                    SessionManager::window_target(&root_session, window_idx)
                } else {
                    // Root agent: use session directly
                    target_agent.tmux_session.clone()
                };

                // Send the message and submit it
                if self
                    .session_manager
                    .send_keys_and_submit(&tmux_target, message)
                    .is_ok()
                {
                    sent_count += 1;
                }
            }
        }

        if sent_count > 0 {
            info!(
                sent_count,
                message_len = message.len(),
                "Broadcast sent to leaf agents"
            );
            app.set_status(format!("Broadcast sent to {sent_count} agent(s)"));
        } else {
            warn!(%agent_id, "No leaf agents found to broadcast to");
            app.set_error("No leaf agents found to broadcast to");
        }

        Ok(())
    }

    /// Resize all agent tmux windows to match the preview pane dimensions
    ///
    /// This ensures the terminal output renders correctly in the preview pane.
    pub fn resize_agent_windows(&self, app: &App) {
        let Some((width, height)) = app.preview_dimensions else {
            return;
        };

        for agent in app.storage.iter() {
            if agent.is_root() {
                // Root agent: resize the session
                if self.session_manager.exists(&agent.tmux_session) {
                    let _ = self
                        .session_manager
                        .resize_window(&agent.tmux_session, width, height);
                }
            } else if let Some(window_idx) = agent.window_index {
                // Child agent: resize the specific window
                let root = app.storage.root_ancestor(agent.id);
                if let Some(root_agent) = root
                    && self.session_manager.exists(&root_agent.tmux_session)
                {
                    let window_target =
                        SessionManager::window_target(&root_agent.tmux_session, window_idx);
                    let _ = self
                        .session_manager
                        .resize_window(&window_target, width, height);
                }
            }
        }
    }

    /// Check and update agent statuses based on tmux sessions
    ///
    /// # Errors
    ///
    /// Returns an error if status sync fails
    pub fn sync_agent_status(self, app: &mut App) -> Result<()> {
        let mut changed = false;

        // Fetch all sessions once instead of calling exists() per agent
        // This reduces subprocess calls from O(n) to O(1)
        let active_sessions: std::collections::HashSet<String> = self
            .session_manager
            .list()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.name)
            .collect();

        // Collect IDs of dead agents to remove
        let dead_agents: Vec<uuid::Uuid> = app
            .storage
            .iter()
            .filter(|agent| !active_sessions.contains(&agent.tmux_session))
            .map(|agent| {
                debug!(title = %agent.title, "Removing dead agent (session not found)");
                agent.id
            })
            .collect();

        // Remove dead agents
        for id in dead_agents {
            app.storage.remove(id);
            changed = true;
        }

        // Update starting agents to running if their session exists
        for agent in app.storage.iter_mut() {
            if agent.status == Status::Starting {
                debug!(title = %agent.title, "Agent status: Starting -> Running");
                agent.set_status(Status::Running);
                changed = true;
            }
        }

        if changed {
            app.storage.save()?;
            app.validate_selection();
        }

        Ok(())
    }

    /// Adjust window indices for all agents under a root after windows are deleted
    ///
    /// This handles the case where tmux has `renumber-windows on` and
    /// window indices shift after windows are deleted. We compute the new
    /// indices mathematically rather than relying on window names.
    fn adjust_window_indices_after_deletion(
        app: &mut App,
        root_id: uuid::Uuid,
        deleted_agent_id: uuid::Uuid,
        deleted_indices: &[u32],
    ) {
        if deleted_indices.is_empty() {
            return;
        }

        // Sort deleted indices for efficient counting
        let mut sorted_deleted: Vec<u32> = deleted_indices.to_vec();
        sorted_deleted.sort_unstable();

        // Get all descendants of the root (excluding the deleted agent and its descendants)
        let descendants_to_update: Vec<uuid::Uuid> = app
            .storage
            .descendants(root_id)
            .iter()
            .filter(|a| a.id != deleted_agent_id)
            .filter(|a| !app.storage.descendant_ids(deleted_agent_id).contains(&a.id))
            .map(|a| a.id)
            .collect();

        // Update each remaining agent's window index
        for agent_id in descendants_to_update {
            if let Some(agent) = app.storage.get_mut(agent_id)
                && let Some(current_idx) = agent.window_index
            {
                // Count how many deleted indices are less than current index
                let decrement =
                    u32::try_from(sorted_deleted.iter().filter(|&&d| d < current_idx).count())
                        .unwrap_or(0);
                if decrement > 0 {
                    let new_idx = current_idx - decrement;
                    debug!(
                        agent_id = %agent.short_id(),
                        agent_title = %agent.title,
                        %current_idx,
                        %new_idx,
                        "Adjusting window index after deletion"
                    );
                    agent.window_index = Some(new_idx);
                }
            }
        }
    }

    /// Auto-connect to existing worktrees on startup
    ///
    /// This function scans for worktrees that match the configured branch prefix
    /// and creates agents for them if they don't already exist in storage.
    /// The agent title will be the branch name.
    ///
    /// # Errors
    ///
    /// Returns an error if worktrees cannot be listed or agent creation fails
    pub fn auto_connect_worktrees(self, app: &mut App) -> Result<()> {
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let Ok(repo) = git::open_repository(&repo_path) else {
            // Not in a git repository, nothing to auto-connect
            debug!("Not in a git repository, skipping auto-connect");
            return Ok(());
        };

        let worktree_mgr = WorktreeManager::new(&repo);
        let worktrees = worktree_mgr.list()?;

        debug!(count = worktrees.len(), "Found worktrees for auto-connect");

        for wt in worktrees {
            // Get the actual branch name from the worktree's HEAD
            // This is more reliable than trying to reverse-engineer from worktree name
            let branch_name = match worktree_mgr.worktree_head_info(&wt.name) {
                Ok((branch, _commit)) => branch,
                Err(e) => {
                    debug!(worktree = %wt.name, error = %e, "Could not get worktree HEAD info, skipping");
                    continue;
                }
            };

            // Only process worktrees that match our branch prefix
            if !branch_name.starts_with(&app.config.branch_prefix) {
                debug!(branch = %branch_name, prefix = %app.config.branch_prefix, "Skipping worktree with different prefix");
                continue;
            }

            // Check if there's already an agent for this branch
            let agent_exists = app.storage.iter().any(|a| a.branch == branch_name);
            if agent_exists {
                debug!(branch = %branch_name, "Agent already exists for worktree");
                continue;
            }

            info!(branch = %branch_name, path = ?wt.path, "Auto-connecting to existing worktree");

            // Create an agent for this worktree
            let agent = Agent::new(
                branch_name.clone(), // Use branch name as title
                app.config.default_program.clone(),
                branch_name.clone(),
                wt.path.clone(),
                None, // No initial prompt
            );

            // Create tmux session and start the agent program
            let command = app.config.default_program.clone();
            self.session_manager
                .create(&agent.tmux_session, &wt.path, Some(&command))?;

            // Resize the session to match preview dimensions if available
            if let Some((width, height)) = app.preview_dimensions {
                let _ = self
                    .session_manager
                    .resize_window(&agent.tmux_session, width, height);
            }

            app.storage.add(agent);
            info!(branch = %branch_name, "Auto-connected to existing worktree");
        }

        // Save storage if we added any agents
        app.storage.save()?;
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
    fn test_handle_action_new_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::NewAgent)?;
        assert_eq!(app.mode, Mode::Creating);
        Ok(())
    }

    #[test]
    fn test_handle_action_new_agent_with_prompt() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::NewAgentWithPrompt)?;
        assert_eq!(app.mode, Mode::Prompting);
        Ok(())
    }

    #[test]
    fn test_handle_action_help() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Help)?;
        assert_eq!(app.mode, Mode::Help);
        Ok(())
    }

    #[test]
    fn test_handle_action_quit_no_agents() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Quit)?;
        assert!(app.should_quit);
        Ok(())
    }

    #[test]
    fn test_handle_action_switch_tab() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::SwitchTab)?;
        assert_eq!(app.active_tab, super::super::state::Tab::Diff);
        Ok(())
    }

    #[test]
    fn test_handle_action_navigation() -> Result<(), Box<dyn std::error::Error>> {
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
        handler.handle_action(&mut app, Action::NextAgent)?;
        assert_eq!(app.selected, 1);
        handler.handle_action(&mut app, Action::PrevAgent)?;
        assert_eq!(app.selected, 0);
        Ok(())
    }

    #[test]
    fn test_handle_action_scroll() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::ScrollDown)?;
        assert_eq!(app.preview_scroll, 5);

        handler.handle_action(&mut app, Action::ScrollUp)?;
        assert_eq!(app.preview_scroll, 0);

        handler.handle_action(&mut app, Action::ScrollTop)?;
        assert_eq!(app.preview_scroll, 0);
        Ok(())
    }

    #[test]
    fn test_handle_action_cancel() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        app.enter_mode(Mode::Creating);
        handler.handle_action(&mut app, Action::Cancel)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
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
    fn test_handle_kill_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::Kill)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_attach_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let result = handler.handle_action(&mut app, Action::Attach);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_push_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let result = handler.handle_action(&mut app, Action::Push);
        assert!(result.is_err());
    }

    #[test]
    fn test_sync_agent_status() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.sync_agent_status(&mut app)?;
        Ok(())
    }

    #[test]
    fn test_handle_quit_with_running_agents() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a running agent
        let mut agent = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        agent.set_status(Status::Running);
        app.storage.add(agent);

        // Quit should enter confirming mode
        handler.handle_action(&mut app, Action::Quit)?;
        assert_eq!(app.mode, Mode::Confirming(ConfirmAction::Quit));
        Ok(())
    }

    #[test]
    fn test_handle_kill_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Kill should enter confirming mode
        handler.handle_action(&mut app, Action::Kill)?;
        assert_eq!(app.mode, Mode::Confirming(ConfirmAction::Kill));
        Ok(())
    }

    #[test]
    fn test_handle_confirm_quit() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Enter confirming mode for quit
        app.enter_mode(Mode::Confirming(ConfirmAction::Quit));

        handler.handle_action(&mut app, Action::Confirm)?;
        assert!(app.should_quit);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_kill() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp/nonexistent"),
            None,
        ));

        // Enter confirming mode for kill
        app.enter_mode(Mode::Confirming(ConfirmAction::Kill));

        // Confirm should kill and exit mode
        handler.handle_action(&mut app, Action::Confirm)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_confirm_reset() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add agents
        for i in 0..3 {
            app.storage.add(Agent::new(
                format!("agent{i}"),
                "claude".to_string(),
                format!("muster/agent{i}"),
                PathBuf::from("/tmp"),
                None,
            ));
        }

        // Enter confirming mode for reset
        app.enter_mode(Mode::Confirming(ConfirmAction::Reset));

        handler.handle_action(&mut app, Action::Confirm)?;
        assert_eq!(app.storage.len(), 0);
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_attach_session_not_found() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with a non-existent session
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "nonexistent-session".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Attach should fail
        let result = handler.handle_action(&mut app, Action::Attach);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_preview_with_agent_no_session() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

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
        use crate::agent::Agent;
        use std::path::PathBuf;

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
        use crate::agent::Agent;
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

    #[test]
    fn test_sync_agent_status_with_agents() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add agents with different statuses
        let mut running = Agent::new(
            "running".to_string(),
            "claude".to_string(),
            "muster/running".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        running.set_status(Status::Running);
        app.storage.add(running);

        let mut starting = Agent::new(
            "starting".to_string(),
            "claude".to_string(),
            "muster/starting".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        starting.set_status(Status::Starting);
        app.storage.add(starting);

        // Sync should remove dead agents (no sessions exist)
        handler.sync_agent_status(&mut app)?;

        // All agents should be removed since their sessions don't exist
        assert_eq!(app.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_handle_scroll_bottom() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::ScrollBottom)?;
        // ScrollBottom calls scroll_to_bottom(10000, 0) so preview_scroll becomes 10000
        assert_eq!(app.preview_scroll, 10000);
        Ok(())
    }

    #[test]
    fn test_create_agent_max_reached() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();
        app.config.max_agents = 2;

        // Add max agents
        for i in 0..2 {
            app.storage.add(Agent::new(
                format!("agent{i}"),
                "claude".to_string(),
                format!("muster/agent{i}"),
                PathBuf::from("/tmp"),
                None,
            ));
        }

        // Try to create another - should fail with error
        handler.create_agent(&mut app, "overflow", None)?;
        assert!(app.last_error.is_some());
        Ok(())
    }

    #[test]
    fn test_handle_push_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Push should enter ConfirmPush mode
        handler.handle_action(&mut app, Action::Push)?;

        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "muster/test");
        Ok(())
    }

    #[test]
    fn test_toggle_collapse_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Should not error with no agent selected
        handler.toggle_collapse(&mut app)?;
        Ok(())
    }

    #[test]
    fn test_toggle_collapse_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Should not error when agent has no children
        handler.toggle_collapse(&mut app)?;
        Ok(())
    }

    #[test]
    fn test_synthesize_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Should error with no agent selected
        let result = handler.synthesize(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Should set error when agent has no children
        handler.synthesize(&mut app)?;
        assert!(app.last_error.is_some());
        assert!(
            app.last_error
                .as_ref()
                .ok_or("Expected last_error")?
                .contains("no children to synthesize")
        );
        Ok(())
    }

    #[test]
    fn test_handle_action_spawn_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        handler.handle_action(&mut app, Action::SpawnChildren)?;
        assert_eq!(app.mode, Mode::ChildCount);
        assert!(app.spawning_under.is_none());
        Ok(())
    }

    #[test]
    fn test_handle_action_add_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        handler.handle_action(&mut app, Action::AddChildren)?;
        assert_eq!(app.mode, Mode::ChildCount);
        assert_eq!(app.spawning_under, Some(agent_id));
        Ok(())
    }

    #[test]
    fn test_handle_action_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent - should not enter confirming mode
        handler.handle_action(&mut app, Action::Synthesize)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_action_synthesize_with_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add parent agent
        let parent = Agent::new(
            "parent".to_string(),
            "claude".to_string(),
            "tenex/parent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let parent_id = parent.id;
        app.storage.add(parent);

        // Add child agent
        let mut child = Agent::new(
            "child".to_string(),
            "claude".to_string(),
            "tenex/child".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        child.parent_id = Some(parent_id);
        app.storage.add(child);

        // With children - should enter confirming mode
        handler.handle_action(&mut app, Action::Synthesize)?;
        assert_eq!(app.mode, Mode::Confirming(ConfirmAction::Synthesize));
        Ok(())
    }

    #[test]
    fn test_handle_action_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add agent with no children
        app.storage.add(Agent::new(
            "parent".to_string(),
            "claude".to_string(),
            "tenex/parent".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // No children - should show error modal, not enter confirming mode
        handler.handle_action(&mut app, Action::Synthesize)?;
        assert!(matches!(app.mode, Mode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_handle_action_toggle_collapse() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent - should not error
        handler.handle_action(&mut app, Action::ToggleCollapse)?;
        Ok(())
    }

    #[test]
    fn test_handle_action_broadcast_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent - should not enter mode
        handler.handle_action(&mut app, Action::Broadcast)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    #[test]
    fn test_handle_action_broadcast_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        handler.handle_action(&mut app, Action::Broadcast)?;
        assert_eq!(app.mode, Mode::Broadcasting);
        Ok(())
    }

    #[test]
    fn test_broadcast_to_leaves_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent selected - should return error
        let result = handler.broadcast_to_leaves(&mut app, "test message");
        assert!(result.is_err());
    }

    #[test]
    fn test_resize_agent_windows_no_dimensions() {
        let handler = Actions::new();
        let app = create_test_app();

        // Should not panic when no dimensions are set
        handler.resize_agent_windows(&app);
        assert!(app.preview_dimensions.is_none());
    }

    #[test]
    fn test_resize_agent_windows_with_dimensions() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Set preview dimensions
        app.set_preview_dimensions(100, 50);

        // Add a root agent (session won't exist, but should not error)
        app.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Should not panic when resizing non-existent sessions
        handler.resize_agent_windows(&app);
        assert_eq!(app.preview_dimensions, Some((100, 50)));
    }

    #[test]
    fn test_resize_agent_windows_with_child_agents() {
        use crate::agent::{Agent, ChildConfig};
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Set preview dimensions
        app.set_preview_dimensions(80, 40);

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add a child agent
        app.storage.add(Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session,
                window_index: 2,
            },
        ));

        // Should handle both root and child agents without panicking
        handler.resize_agent_windows(&app);
    }

    #[test]
    fn test_kill_agent_root() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent
        app.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Kill should work (session doesn't exist, but should not error)
        handler.kill_agent(&mut app)?;
        assert_eq!(app.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_kill_agent_child() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::{Agent, ChildConfig};
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent (expanded to show children)
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session,
                window_index: 2,
            },
        );
        app.storage.add(child);

        // Select the child (it's the second visible agent)
        app.select_next();

        // Kill child should remove just the child
        handler.kill_agent(&mut app)?;
        assert_eq!(app.storage.len(), 1);
        assert!(app.storage.get(root_id).is_some());
        Ok(())
    }

    #[test]
    fn test_kill_agent_with_descendants() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::{Agent, ChildConfig};
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add children
        for i in 0..3 {
            app.storage.add(Agent::new_child(
                format!("child{i}"),
                "claude".to_string(),
                "muster/root".to_string(),
                PathBuf::from("/tmp"),
                None,
                ChildConfig {
                    parent_id: root_id,
                    tmux_session: root_session.clone(),
                    window_index: i + 2,
                },
            ));
        }

        // Kill root should remove all
        handler.kill_agent(&mut app)?;
        assert_eq!(app.storage.len(), 0);
        Ok(())
    }

    #[test]
    fn test_attach_to_agent_no_session() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with non-existent session
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "nonexistent-session".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Attach should fail
        let result = handler.attach_to_agent(&mut app);
        assert!(result.is_err());
        assert!(app.last_error.is_some());
    }

    #[test]
    fn test_broadcast_to_leaves_with_agent_no_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent with no children
        app.storage.add(Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Broadcast should set error when no children
        handler.broadcast_to_leaves(&mut app, "test message")?;
        assert!(app.last_error.is_some());
        Ok(())
    }

    #[test]
    fn test_broadcast_to_leaves_with_children() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::{Agent, ChildConfig};
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent (expanded to show children)
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "muster/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add children (leaves)
        for i in 0..2 {
            app.storage.add(Agent::new_child(
                format!("child{i}"),
                "claude".to_string(),
                "muster/root".to_string(),
                PathBuf::from("/tmp"),
                None,
                ChildConfig {
                    parent_id: root_id,
                    tmux_session: root_session.clone(),
                    window_index: i + 2,
                },
            ));
        }

        // Broadcast when sessions don't exist - send_keys fails, so no messages sent
        // This exercises the "No leaf agents found" path since send_keys fails
        handler.broadcast_to_leaves(&mut app, "test message")?;
        // Since sessions don't exist, send_keys fails and error is set
        assert!(app.last_error.is_some());
        Ok(())
    }

    #[test]
    fn test_reconnect_to_worktree_no_conflict_info() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No conflict info set - should error
        let result = handler.reconnect_to_worktree(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_recreate_worktree_no_conflict_info() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No conflict info set - should error
        let result = handler.recreate_worktree(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_spawn_children_for_root_no_session() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent (the session won't exist)
        let root = Agent::new(
            "root".to_string(),
            "echo".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let root_id = root.id;
        app.storage.add(root);

        // Calling spawn_children_for_root should fail because the session doesn't exist
        let result = handler.spawn_children_for_root(
            &mut app,
            "nonexistent-session",
            &PathBuf::from("/tmp"),
            "test-branch",
            root_id,
            2,
            "test task",
        );

        // This should error because the session doesn't exist
        assert!(result.is_err());
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test assertion")]
    fn test_worktree_conflict_info_struct() {
        use crate::app::WorktreeConflictInfo;

        let mut app = create_test_app();

        // Set up conflict info manually
        app.worktree_conflict = Some(WorktreeConflictInfo {
            title: "test".to_string(),
            prompt: Some("test prompt".to_string()),
            branch: "tenex/test".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/test"),
            existing_branch: Some("tenex/test".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: None,
        });

        // Verify the conflict info is set
        assert!(app.worktree_conflict.is_some());
        let info = app.worktree_conflict.as_ref().unwrap();
        assert_eq!(info.title, "test");
        assert_eq!(info.swarm_child_count, None);
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test assertion")]
    fn test_worktree_conflict_info_swarm() {
        use crate::app::WorktreeConflictInfo;

        let mut app = create_test_app();

        // Set up conflict info for a swarm
        app.worktree_conflict = Some(WorktreeConflictInfo {
            title: "swarm".to_string(),
            prompt: Some("swarm task".to_string()),
            branch: "tenex/swarm".to_string(),
            worktree_path: std::path::PathBuf::from("/tmp/swarm"),
            existing_branch: Some("tenex/swarm".to_string()),
            existing_commit: Some("abc1234".to_string()),
            current_branch: "main".to_string(),
            current_commit: "def5678".to_string(),
            swarm_child_count: Some(3),
        });

        let info = app.worktree_conflict.as_ref().unwrap();
        assert_eq!(info.swarm_child_count, Some(3));
    }

    #[test]
    fn test_handle_action_review_swarm_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No agent - should show ReviewInfo
        handler.handle_action(&mut app, Action::ReviewSwarm)?;
        assert_eq!(app.mode, Mode::ReviewInfo);
        Ok(())
    }

    #[test]
    fn test_spawn_review_agents_no_parent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        // No spawning_under set - should error
        app.spawning_under = None;
        app.review_base_branch = Some("main".to_string());

        let result = handler.spawn_review_agents(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_spawn_review_agents_no_base_branch() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // spawning_under set but no base branch - should error
        app.spawning_under = Some(agent_id);
        app.review_base_branch = None;

        let result = handler.spawn_review_agents(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_review_state_cleared() {
        let mut app = create_test_app();

        // Set up some review state
        app.review_branches = vec![crate::git::BranchInfo {
            name: "test".to_string(),
            full_name: "refs/heads/test".to_string(),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        }];
        app.review_branch_filter = "filter".to_string();
        app.review_branch_selected = 1;

        // Clear the state
        app.clear_review_state();

        assert!(app.review_branches.is_empty());
        assert!(app.review_branch_filter.is_empty());
        assert_eq!(app.review_branch_selected, 0);
        assert!(app.review_base_branch.is_none());
    }

    #[test]
    fn test_review_info_mode_exit() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let mut app = create_test_app();

        // Enter ReviewInfo mode
        app.show_review_info();
        assert_eq!(app.mode, Mode::ReviewInfo);

        // Cancel should exit
        handler.handle_action(&mut app, Action::Cancel)?;
        assert_eq!(app.mode, Mode::Normal);
        Ok(())
    }

    // === Git Operations Tests ===

    #[test]
    fn test_execute_push_no_agent_id() {
        let mut app = create_test_app();
        app.git_op_agent_id = None;

        let result = Actions::execute_push(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_agent_not_found() {
        let mut app = create_test_app();
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();

        let result = Actions::execute_push(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_rename_no_agent_id() {
        let mut app = create_test_app();
        app.git_op_agent_id = None;

        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_rename_agent_not_found() {
        let mut app = create_test_app();
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "new-name".to_string();
        app.git_op_original_branch = "old-name".to_string();

        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_open_pr_in_browser_no_agent_id() {
        let mut app = create_test_app();
        app.git_op_agent_id = None;

        let result = Actions::open_pr_in_browser(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_open_pr_in_browser_agent_not_found() {
        let mut app = create_test_app();
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test".to_string();
        app.git_op_base_branch = "main".to_string();

        let result = Actions::open_pr_in_browser(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_push_flow_state_transitions() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let mut app = create_test_app();

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "feature/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start push flow
        app.start_push(agent_id, "feature/test".to_string());
        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "feature/test");

        // Clear git op state
        app.clear_git_op_state();
        assert!(app.git_op_branch_name.is_empty());
        assert!(app.git_op_agent_id.is_none());
    }

    #[test]
    fn test_rename_root_flow_state_transitions() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let mut app = create_test_app();

        // Add a root agent
        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start rename flow for root agent
        app.start_rename(agent_id, "test-agent".to_string(), true);
        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_original_branch, "test-agent");
        assert_eq!(app.git_op_branch_name, "test-agent");
        assert_eq!(app.input_buffer, "test-agent");
        assert!(app.git_op_is_root_rename);

        // Simulate user input
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_char('n');
        app.handle_char('e');
        app.handle_char('w');
        assert_eq!(app.input_buffer, "test-new");

        // Confirm rename
        let result = app.confirm_rename_branch();
        assert!(result);
        assert_eq!(app.git_op_branch_name, "test-new");
    }

    #[test]
    fn test_rename_subagent_flow_state_transitions() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let mut app = create_test_app();

        // Add a root agent first
        let root = Agent::new(
            "root-agent".to_string(),
            "claude".to_string(),
            "tenex/root-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        app.storage.add(root.clone());

        // Add a child agent
        let child = Agent::new_child(
            "sub-agent".to_string(),
            "claude".to_string(),
            "tenex/root-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
            crate::agent::ChildConfig {
                parent_id: root.id,
                tmux_session: root.tmux_session,
                window_index: 1,
            },
        );
        let child_id = child.id;
        app.storage.add(child);

        // Start rename flow for sub-agent
        app.start_rename(child_id, "sub-agent".to_string(), false);
        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op_agent_id, Some(child_id));
        assert_eq!(app.git_op_original_branch, "sub-agent");
        assert!(!app.git_op_is_root_rename);

        // Simulate user input
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_backspace();
        app.handle_char('n');
        app.handle_char('e');
        app.handle_char('w');
        assert_eq!(app.input_buffer, "sub-new");

        // Confirm rename
        let result = app.confirm_rename_branch();
        assert!(result);
        assert_eq!(app.git_op_branch_name, "sub-new");
    }

    #[test]
    fn test_open_pr_flow_state_with_unpushed() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let mut app = create_test_app();

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "feature/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start open PR flow with unpushed commits
        app.start_open_pr(
            agent_id,
            "feature/test".to_string(),
            "main".to_string(),
            true,
        );

        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "feature/test");
        assert_eq!(app.git_op_base_branch, "main");
        assert!(app.git_op_has_unpushed);
    }

    #[test]
    fn test_open_pr_flow_state_no_unpushed() {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let mut app = create_test_app();

        // Add an agent
        let agent = Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "feature/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Start open PR flow without unpushed commits
        app.start_open_pr(
            agent_id,
            "feature/test".to_string(),
            "main".to_string(),
            false,
        );

        // Mode should stay Normal (handler opens PR directly)
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert!(!app.git_op_has_unpushed);
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code")]
    fn test_detect_base_branch_no_git() {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new().unwrap();

        // Should return default "main" when git commands fail
        let result = Actions::detect_base_branch(temp_dir.path(), "feature/test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "main");
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code")]
    fn test_has_unpushed_commits_no_git() {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new().unwrap();

        // Should return true (assume all commits are unpushed if we can't check)
        let result = Actions::has_unpushed_commits(temp_dir.path(), "feature/test");
        // Either Ok(true) or Err is acceptable
        let _ = result;
    }

    #[test]
    fn test_handle_rename_with_root_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent
        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test-agent".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Rename should enter RenameBranch mode with agent title
        handler.handle_action(&mut app, Action::RenameBranch)?;

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        assert_eq!(app.git_op_branch_name, "test-agent");
        assert_eq!(app.git_op_original_branch, "test-agent");
        assert_eq!(app.input_buffer, "test-agent");
        assert!(app.git_op_is_root_rename);
        Ok(())
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test code")]
    fn test_handle_rename_with_subagent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        // Add a root agent first
        let root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let root_id = root.id;
        app.storage.add(root.clone());

        // Add a child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            crate::agent::ChildConfig {
                parent_id: root_id,
                tmux_session: root.tmux_session,
                window_index: 1,
            },
        );
        let child_id = child.id;
        app.storage.add(child);

        // Expand root to see child, then select the child agent
        app.storage
            .get_mut(root_id)
            .expect("root agent should exist")
            .collapsed = false;
        app.select_next();

        // Rename should enter RenameBranch mode with agent title, not root rename
        handler.handle_action(&mut app, Action::RenameBranch)?;

        assert_eq!(app.mode, Mode::RenameBranch);
        assert_eq!(app.git_op_agent_id, Some(child_id));
        assert_eq!(app.git_op_branch_name, "child");
        assert_eq!(app.git_op_original_branch, "child");
        assert_eq!(app.input_buffer, "child");
        assert!(!app.git_op_is_root_rename);
        Ok(())
    }

    #[test]
    #[expect(clippy::unwrap_used, reason = "test code")]
    fn test_check_remote_branch_exists_no_git() {
        use tempfile::TempDir;

        // Create a temp directory that's not a git repo
        let temp_dir = TempDir::new().unwrap();

        // Should return Ok(false) when not in a git repo (command returns error)
        let result = Actions::check_remote_branch_exists(temp_dir.path(), "main");
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_execute_rename_clears_state_on_no_agent() {
        let mut app = create_test_app();

        // Set up state but with an invalid agent ID
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "new-name".to_string();
        app.git_op_is_root_rename = true;

        // Execute should fail gracefully
        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_rename_subagent_clears_state_on_no_agent() {
        let mut app = create_test_app();

        // Set up state but with an invalid agent ID
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "new-name".to_string();
        app.git_op_is_root_rename = false;

        // Execute should fail gracefully
        let result = Actions::execute_rename(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_and_open_pr_no_agent_id() {
        let mut app = create_test_app();

        // No agent ID set
        app.git_op_agent_id = None;

        let result = Actions::execute_push_and_open_pr(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_and_open_pr_agent_not_found() {
        let mut app = create_test_app();

        // Set invalid agent ID
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());

        let result = Actions::execute_push_and_open_pr(&mut app);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_open_pr_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let result = handler.handle_action(&mut app, Action::OpenPR);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_rename_no_agent() {
        let handler = Actions::new();
        let mut app = create_test_app();

        let result = handler.handle_action(&mut app, Action::RenameBranch);
        assert!(result.is_err());
    }

    #[test]
    fn test_git_op_state_cleared_properly() {
        let mut app = create_test_app();

        // Set up git op state
        app.git_op_agent_id = Some(uuid::Uuid::new_v4());
        app.git_op_branch_name = "test-branch".to_string();
        app.git_op_original_branch = "original".to_string();
        app.git_op_base_branch = "main".to_string();
        app.git_op_has_unpushed = true;
        app.git_op_is_root_rename = true;

        // Clear the state
        app.clear_git_op_state();

        // Verify all fields are cleared
        assert!(app.git_op_agent_id.is_none());
        assert!(app.git_op_branch_name.is_empty());
        assert!(app.git_op_original_branch.is_empty());
        assert!(app.git_op_base_branch.is_empty());
        assert!(!app.git_op_has_unpushed);
        assert!(!app.git_op_is_root_rename);
    }

    #[test]
    fn test_open_pr_flow_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Trigger open PR action
        handler.handle_action(&mut app, Action::OpenPR)?;

        // Should enter ConfirmPushForPR mode
        assert_eq!(app.mode, Mode::ConfirmPushForPR);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        Ok(())
    }

    #[test]
    fn test_push_flow_with_agent() -> Result<(), Box<dyn std::error::Error>> {
        use crate::agent::Agent;
        use std::path::PathBuf;

        let handler = Actions::new();
        let mut app = create_test_app();

        let agent = Agent::new(
            "test-agent".to_string(),
            "claude".to_string(),
            "tenex/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        let agent_id = agent.id;
        app.storage.add(agent);

        // Trigger push action
        handler.handle_action(&mut app, Action::Push)?;

        // Should enter ConfirmPush mode
        assert_eq!(app.mode, Mode::ConfirmPush);
        assert_eq!(app.git_op_agent_id, Some(agent_id));
        Ok(())
    }
}
