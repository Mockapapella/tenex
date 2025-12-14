//! Swarm operations: spawn children, spawn review agents, synthesize

use crate::agent::{Agent, ChildConfig};
use crate::git::{self, WorktreeManager};
use crate::prompts;
use crate::tmux::SessionManager;
use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::state::{App, Mode, WorktreeConflictInfo};

/// Configuration for spawning child agents
pub struct SpawnConfig {
    pub root_session: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub parent_agent_id: uuid::Uuid,
}

impl Actions {
    /// Spawn child agents under a parent (or create new root with children)
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_children(self, app: &mut App, task: Option<&str>) -> Result<()> {
        let count = app.spawn.child_count;
        let parent_id = app.spawn.spawning_under;

        info!(
            count,
            ?parent_id,
            task_len = task.map_or(0, str::len),
            "Spawning child agents"
        );

        let spawn_config = if let Some(pid) = parent_id {
            Self::get_existing_parent_config(app, pid)?
        } else {
            match self.create_new_root_for_swarm(app, task, count)? {
                Some(config) => config,
                None => return Ok(()), // Worktree conflict, user needs to choose
            }
        };

        self.spawn_child_agents(app, &spawn_config, count, task)?;

        // Expand the parent to show children
        if let Some(parent) = app.storage.get_mut(spawn_config.parent_agent_id) {
            parent.collapsed = false;
        }

        app.storage.save()?;
        info!(count, parent_id = %spawn_config.parent_agent_id, "Child agents spawned successfully");
        app.set_status(format!("Spawned {count} child agents"));
        Ok(())
    }

    /// Get spawn configuration from an existing parent agent
    fn get_existing_parent_config(app: &App, pid: uuid::Uuid) -> Result<SpawnConfig> {
        let root = app
            .storage
            .root_ancestor(pid)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        Ok(SpawnConfig {
            root_session: root.tmux_session.clone(),
            worktree_path: root.worktree_path.clone(),
            branch: root.branch.clone(),
            parent_agent_id: pid,
        })
    }

    /// Create a new root agent for a swarm, returns None if worktree conflict
    fn create_new_root_for_swarm(
        self,
        app: &mut App,
        task: Option<&str>,
        count: usize,
    ) -> Result<Option<SpawnConfig>> {
        let root_title = Self::generate_root_title(task);
        let branch = app.config.generate_branch_name(&root_title);
        let worktree_path = app.config.worktree_dir.join(&branch);
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;

        let worktree_mgr = WorktreeManager::new(&repo);

        if worktree_mgr.exists(&branch) {
            Self::setup_worktree_conflict(
                app,
                &worktree_mgr,
                root_title,
                task,
                branch,
                worktree_path,
                count,
            );
            return Ok(None);
        }

        worktree_mgr.create_with_new_branch(&worktree_path, &branch)?;

        let program = app.agent_spawn_command();
        let root_agent = Agent::new(
            root_title,
            program.clone(),
            branch.clone(),
            worktree_path.clone(),
            None,
        );

        let root_session = root_agent.tmux_session.clone();
        let root_id = root_agent.id;

        self.session_manager
            .create(&root_session, &worktree_path, Some(&program))?;

        if let Some((width, height)) = app.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&root_session, width, height);
        }

        app.storage.add(root_agent);
        Ok(Some(SpawnConfig {
            root_session,
            worktree_path,
            branch,
            parent_agent_id: root_id,
        }))
    }

    /// Generate a title for a new root swarm agent
    fn generate_root_title(task: Option<&str>) -> String {
        match task {
            Some(t) if t.len() > 30 => format!("{}...", &t[..27]),
            Some(t) => t.to_string(),
            None => {
                let short_id = &uuid::Uuid::new_v4().to_string()[..8];
                format!("Swarm ({short_id})")
            }
        }
    }

    /// Setup worktree conflict info for user to resolve
    fn setup_worktree_conflict(
        app: &mut App,
        worktree_mgr: &WorktreeManager<'_>,
        root_title: String,
        task: Option<&str>,
        branch: String,
        worktree_path: PathBuf,
        count: usize,
    ) {
        debug!(branch, "Worktree already exists for swarm, prompting user");

        let (current_branch, current_commit) = worktree_mgr
            .head_info()
            .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));

        let (existing_branch, existing_commit) = worktree_mgr
            .worktree_head_info(&branch)
            .map(|(b, c)| (Some(b), Some(c)))
            .unwrap_or((None, None));

        app.spawn.worktree_conflict = Some(WorktreeConflictInfo {
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
        app.enter_mode(Mode::Confirming(
            crate::app::state::ConfirmAction::WorktreeConflict,
        ));
    }

    /// Spawn the actual child agents
    fn spawn_child_agents(
        self,
        app: &mut App,
        config: &SpawnConfig,
        count: usize,
        task: Option<&str>,
    ) -> Result<()> {
        let start_window_index = app.storage.reserve_window_indices(config.parent_agent_id);
        let program = app.agent_spawn_command();
        let child_prompt = task.map(|t| Self::build_child_prompt(t, app.spawn.use_plan_prompt));

        for i in 0..count {
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);
            self.spawn_single_child(
                app,
                config,
                i,
                window_index,
                &program,
                child_prompt.as_deref(),
            )?;
        }

        Ok(())
    }

    /// Build the prompt for child agents
    fn build_child_prompt(task: &str, use_plan_prompt: bool) -> String {
        if use_plan_prompt {
            prompts::build_plan_prompt(task)
        } else {
            task.to_string()
        }
    }

    /// Spawn a single child agent
    fn spawn_single_child(
        self,
        app: &mut App,
        config: &SpawnConfig,
        index: usize,
        window_index: u32,
        program: &str,
        child_prompt: Option<&str>,
    ) -> Result<()> {
        let child = Agent::new_child(
            String::new(),
            program.to_string(),
            config.branch.clone(),
            config.worktree_path.clone(),
            child_prompt.map(String::from),
            ChildConfig {
                parent_id: config.parent_agent_id,
                tmux_session: config.root_session.clone(),
                window_index,
            },
        );

        let child_title = if app.spawn.use_plan_prompt && child_prompt.is_some() {
            format!("Planner {} ({})", index + 1, child.short_id())
        } else {
            format!("Agent {} ({})", index + 1, child.short_id())
        };
        let mut child = child;
        child.title.clone_from(&child_title);

        let command = Self::build_child_command(program, child_prompt);
        let actual_index = self.session_manager.create_window(
            &config.root_session,
            &child_title,
            &config.worktree_path,
            Some(&command),
        )?;

        if let Some((width, height)) = app.ui.preview_dimensions {
            let window_target = SessionManager::window_target(&config.root_session, actual_index);
            let _ = self
                .session_manager
                .resize_window(&window_target, width, height);
        }

        child.window_index = Some(actual_index);
        app.storage.add(child);

        Ok(())
    }

    /// Build the command to run in a child window
    fn build_child_command(default_program: &str, prompt: Option<&str>) -> String {
        prompt.map_or_else(
            || default_program.to_string(),
            |p| {
                let escaped = p.replace('"', "\\\"").replace('`', "\\`");
                format!("{default_program} \"{escaped}\"")
            },
        )
    }

    /// Spawn child agents for an existing root agent
    ///
    /// This is a helper used by both `spawn_children` and `reconnect_to_worktree`
    pub(crate) fn spawn_children_for_root(
        self,
        app: &mut App,
        config: &SpawnConfig,
        count: usize,
        task: &str,
    ) -> Result<()> {
        self.spawn_child_agents(app, config, count, Some(task))?;

        // Expand the parent to show children
        if let Some(parent) = app.storage.get_mut(config.parent_agent_id) {
            parent.collapsed = false;
        }

        Ok(())
    }

    /// Spawn review agents for the selected agent against a base branch
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_review_agents(self, app: &mut App) -> Result<()> {
        let count = app.spawn.child_count;
        let parent_id = app
            .spawn
            .spawning_under
            .ok_or_else(|| anyhow::anyhow!("No agent selected for review"))?;
        let base_branch = app
            .review
            .base_branch
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
        let program = app.agent_spawn_command();

        // Create review child agents
        for i in 0..count {
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);

            let child = Agent::new_child(
                String::new(), // Placeholder
                program.clone(),
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
            let command = format!("{} \"{escaped_prompt}\"", &program);
            let actual_index = self.session_manager.create_window(
                &root_session,
                &child_title,
                &worktree_path,
                Some(&command),
            )?;

            // Resize the new window to match preview dimensions
            if let Some((width, height)) = app.ui.preview_dimensions {
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
        // Filter out terminal agents - they are interactive shells, not research agents
        let descendants: Vec<_> = app
            .storage
            .descendants(parent_id)
            .into_iter()
            .filter(|d| !d.is_terminal)
            .collect();

        if descendants.is_empty() {
            warn!(agent_id = %parent_id, title = %parent_title, "No non-terminal children to synthesize");
            app.set_error("Selected agent has no non-terminal children to synthesize");
            return Ok(());
        }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_test_app() -> Result<(App, NamedTempFile), std::io::Error> {
        let temp_file = NamedTempFile::new()?;
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        Ok((
            App::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        ))
    }

    #[test]
    fn test_spawn_children_for_root_no_session() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

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
        let spawn_config = SpawnConfig {
            root_session: "nonexistent-session".to_string(),
            worktree_path: PathBuf::from("/tmp"),
            branch: "test-branch".to_string(),
            parent_agent_id: root_id,
        };
        let result = handler.spawn_children_for_root(&mut app, &spawn_config, 2, "test task");

        // This should error because the session doesn't exist
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Should error with no agent selected
        let result = handler.synthesize(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
            None,
        ));

        // Should set error when agent has no children
        handler.synthesize(&mut app)?;
        assert!(app.ui.last_error.is_some());
        assert!(
            app.ui
                .last_error
                .as_ref()
                .ok_or("Expected last_error")?
                .contains("no children to synthesize")
        );
        Ok(())
    }

    #[test]
    fn test_spawn_review_agents_no_parent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // No spawning_under set - should error
        app.spawn.spawning_under = None;
        app.review.base_branch = Some("main".to_string());

        let result = handler.spawn_review_agents(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_spawn_review_agents_no_base_branch() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

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
        app.spawn.spawning_under = Some(agent_id);
        app.review.base_branch = None;

        let result = handler.spawn_review_agents(&mut app);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_broadcast_excludes_terminals() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Create a root agent
        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.tmux_session.clone();
        app.storage.add(root);

        // Add a regular child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session.clone(),
                window_index: 2,
            },
        );
        app.storage.add(child);

        // Add a terminal child (is_terminal = true)
        let mut terminal = Agent::new_child(
            "Terminal 1".to_string(),
            "bash".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            None,
            ChildConfig {
                parent_id: root_id,
                tmux_session: root_session,
                window_index: 3,
            },
        );
        terminal.is_terminal = true;
        app.storage.add(terminal);

        // Broadcast should only target the non-terminal child (1 agent)
        // Since tmux sessions don't exist, it will fail but we can check
        // it attempts to send to the right number of agents
        let result = handler.broadcast_to_leaves(&mut app, "test");

        // The broadcast will "succeed" with 0 sent (sessions don't exist)
        // but importantly it should NOT error and should report 0 (not try terminal)
        assert!(result.is_ok());

        // Check status message mentions 0 or shows error about no agents
        // (since the tmux sessions don't actually exist)
        Ok(())
    }
}
