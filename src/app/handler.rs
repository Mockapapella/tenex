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

use super::state::{App, ConfirmAction, Mode};

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
        }
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

        // If worktree/branch already exists, remove it first to start fresh
        if worktree_mgr.exists(&branch) {
            worktree_mgr.remove(&branch)?;
        }

        worktree_mgr.create_with_new_branch(&worktree_path, &branch)?;

        let agent = Agent::new(
            title.to_string(),
            app.config.default_program.clone(),
            branch.clone(),
            worktree_path.clone(),
            prompt.map(String::from),
        );

        let mut command = app.config.default_program.clone();
        if let Some(p) = prompt {
            // Pass prompt as positional argument (works for codex, claude, etc.)
            command = format!("{} \"{}\"", command, p.replace('"', "\\\""));
        }

        self.session_manager
            .create(&agent.tmux_session, &worktree_path, Some(&command))?;

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

    /// Push the selected agent's branch to remote
    fn push_branch(self, app: &mut App) -> Result<()> {
        let agent = app
            .selected_agent()
            .ok_or_else(|| anyhow::anyhow!("No agent selected"))?;

        let _session_exists = self.session_manager.exists(&agent.tmux_session);
        app.set_status(format!("Pushing branch: {}", agent.branch));
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

        // Auto-scroll to bottom so the latest output is visible
        // Use a large value that will be clamped by the render function
        app.preview_scroll = usize::MAX;

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
    pub fn spawn_children(self, app: &mut App, task: &str) -> Result<()> {
        let count = app.child_count;
        let parent_id = app.spawning_under;

        info!(
            count,
            ?parent_id,
            task_len = task.len(),
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
            let root_title = if task.len() > 30 {
                format!("{}...", &task[..27])
            } else {
                task.to_string()
            };
            let branch = app.config.generate_branch_name(&root_title);
            let worktree_path = app.config.worktree_dir.join(&branch);
            let repo_path = std::env::current_dir().context("Failed to get current directory")?;
            let repo = git::open_repository(&repo_path)?;

            let worktree_mgr = WorktreeManager::new(&repo);
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
        let plan_prompt = prompts::build_plan_prompt(task);
        for i in 0..count {
            // Use pre-reserved window index (cast i to u32 for addition)
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);

            // Create child first to get its ID, then build the title with short ID
            let child = Agent::new_child(
                String::new(), // Placeholder, will be updated below
                app.config.default_program.clone(),
                branch.clone(),
                worktree_path.clone(),
                Some(plan_prompt.clone()),
                ChildConfig {
                    parent_id: parent_agent_id,
                    tmux_session: root_session.clone(),
                    window_index,
                },
            );

            // Include short ID in title to distinguish agents with same base name
            let child_title = format!("Child {} ({})", i + 1, child.short_id());
            let mut child = child;
            child.title.clone_from(&child_title);

            // Create window in the root's session with the planning prompt
            let command = format!(
                "{} \"{}\"",
                app.config.default_program,
                plan_prompt.replace('"', "\\\"")
            );
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
        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Push should set status message
        handler.handle_action(&mut app, Action::Push)?;
        assert!(app.status_message.is_some());
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
}
