//! Swarm operations: spawn children, spawn review agents, synthesize

use crate::agent::{Agent, ChildConfig};
use crate::git::{self, WorktreeManager};
use crate::mux::SessionManager;
use crate::prompts;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

use super::Actions;
use crate::app::{AppData, WorktreeConflictInfo};
use crate::state::{AppMode, ConfirmAction, ConfirmingMode, ErrorModalMode};

/// Configuration for spawning child agents
pub struct SpawnConfig {
    pub root_session: String,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub parent_agent_id: uuid::Uuid,
}

#[derive(Clone, Copy)]
struct ReviewChildAgentConfig<'a> {
    root_session: &'a str,
    worktree_path: &'a Path,
    branch: &'a str,
    parent_id: uuid::Uuid,
    program: &'a str,
    review_prompt: &'a str,
    base_branch: &'a str,
    reviewer_number: usize,
    reserved_window_index: u32,
}

impl Actions {
    fn build_synthesis_read_command(
        synthesis_id: uuid::Uuid,
        descendants_count: usize,
        prompt: Option<&str>,
    ) -> String {
        let agent_word = if descendants_count == 1 {
            "agent"
        } else {
            "agents"
        };

        let mut read_command = format!(
            "Read .tenex/{synthesis_id}.md - it contains the work of {descendants_count} {agent_word}. Use it to guide your next steps."
        );
        if let Some(prompt) = prompt.map(str::trim)
            && !prompt.is_empty()
        {
            read_command.push_str("\n\nAdditional instructions:\n");
            read_command.push_str(prompt);
        }
        read_command
    }

    fn start_codex_review_flow(self, target: &str, base_branch: &str) -> Result<()> {
        let base_branch = base_branch.trim();
        if base_branch.is_empty() {
            bail!("Base branch cannot be empty for Codex review flow");
        }

        self.session_manager.send_keys(target, "/review")?;
        std::thread::sleep(Duration::from_millis(25));
        self.session_manager.send_keys_and_submit(target, "")?;
        std::thread::sleep(Duration::from_millis(25));
        self.session_manager.send_keys_and_submit(target, "")?;
        std::thread::sleep(Duration::from_millis(25));
        self.session_manager.paste_keys(target, base_branch)?;
        std::thread::sleep(Duration::from_millis(25));
        self.session_manager.send_keys_and_submit(target, "")?;
        Ok(())
    }

    fn spawn_review_child_agent(
        self,
        app_data: &AppData,
        config: ReviewChildAgentConfig<'_>,
    ) -> Result<Agent> {
        let mut child = Agent::new_child(
            String::new(), // Placeholder
            config.program.to_string(),
            config.branch.to_string(),
            config.worktree_path.to_path_buf(),
            ChildConfig {
                parent_id: config.parent_id,
                mux_session: config.root_session.to_string(),
                window_index: config.reserved_window_index,
            },
        );

        let child_title = format!("Reviewer {} ({})", config.reviewer_number, child.short_id());
        child.title.clone_from(&child_title);

        let cli = crate::conversation::detect_agent_cli(config.program);
        if cli == crate::conversation::AgentCli::Claude {
            child.conversation_id = Some(child.id.to_string());
        }

        let prompt = match cli {
            crate::conversation::AgentCli::Codex => None,
            _ => Some(config.review_prompt),
        };

        let command = crate::conversation::build_spawn_argv(
            config.program,
            prompt,
            child.conversation_id.as_deref(),
        )?;

        let started_at = SystemTime::now();
        let actual_index = self.session_manager.create_window(
            config.root_session,
            &child_title,
            config.worktree_path,
            Some(&command),
        )?;

        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            child.conversation_id = crate::conversation::try_detect_codex_session_id(
                config.worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let window_target = SessionManager::window_target(config.root_session, actual_index);
            let _ = self
                .session_manager
                .resize_window(&window_target, width, height);
        }

        if cli == crate::conversation::AgentCli::Codex {
            let window_target = SessionManager::window_target(config.root_session, actual_index);
            self.start_codex_review_flow(&window_target, config.base_branch)?;
        }

        child.window_index = Some(actual_index);
        Ok(child)
    }

    /// Spawn child agents under a parent (or create new root with children)
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_children(self, app_data: &mut AppData, task: Option<&str>) -> Result<AppMode> {
        let count = app_data.spawn.child_count;
        let parent_id = app_data.spawn.spawning_under;

        info!(
            count,
            ?parent_id,
            task_len = task.map_or(0, str::len),
            "Spawning child agents"
        );

        let spawn_config = if let Some(pid) = parent_id {
            Self::get_existing_parent_config(app_data, pid)?
        } else {
            match self.create_new_root_for_swarm(app_data, task, count)? {
                Some(config) => config,
                None => {
                    return Ok(ConfirmingMode {
                        action: ConfirmAction::WorktreeConflict,
                    }
                    .into());
                }
            }
        };

        self.spawn_child_agents(app_data, &spawn_config, count, task)?;

        // Expand the parent to show children
        if let Some(parent) = app_data.storage.get_mut(spawn_config.parent_agent_id) {
            parent.collapsed = false;
        }

        app_data.storage.save()?;
        info!(count, parent_id = %spawn_config.parent_agent_id, "Child agents spawned successfully");
        app_data.set_status(format!("Spawned {count} child agents"));
        Ok(AppMode::normal())
    }

    /// Get spawn configuration from an existing parent agent
    fn get_existing_parent_config(app_data: &AppData, pid: uuid::Uuid) -> Result<SpawnConfig> {
        let parent = app_data
            .storage
            .get(pid)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        if parent.is_terminal_agent() {
            bail!("Cannot spawn children under a terminal");
        }

        let root = app_data
            .storage
            .root_ancestor(pid)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        Ok(SpawnConfig {
            root_session: root.mux_session.clone(),
            worktree_path: root.worktree_path.clone(),
            branch: root.branch.clone(),
            parent_agent_id: pid,
        })
    }

    /// Create a new root agent for a swarm, returns None if worktree conflict
    fn create_new_root_for_swarm(
        self,
        app_data: &mut AppData,
        task: Option<&str>,
        count: usize,
    ) -> Result<Option<SpawnConfig>> {
        let root_title = Self::generate_root_title(task);
        let branch = app_data.config.generate_branch_name(&root_title);
        let worktree_path = app_data.config.worktree_dir.join(&branch);
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;

        let worktree_mgr = WorktreeManager::new(&repo);

        if worktree_mgr.exists(&branch) {
            Self::setup_worktree_conflict(
                app_data,
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

        let program = app_data.agent_spawn_command();
        let mut root_agent = Agent::new(
            root_title,
            program.clone(),
            branch.clone(),
            worktree_path.clone(),
        );
        let cli = crate::conversation::detect_agent_cli(&program);
        if cli == crate::conversation::AgentCli::Claude {
            root_agent.conversation_id = Some(root_agent.id.to_string());
        }
        let session_prefix = app_data.storage.instance_session_prefix();
        root_agent.mux_session = format!("{session_prefix}{}", root_agent.short_id());

        let root_session = root_agent.mux_session.clone();
        let root_id = root_agent.id;

        let command = crate::conversation::build_spawn_argv(
            &program,
            None,
            root_agent.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        self.session_manager
            .create(&root_session, &worktree_path, Some(&command))?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            root_agent.conversation_id = crate::conversation::try_detect_codex_session_id(
                &worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let _ = self
                .session_manager
                .resize_window(&root_session, width, height);
        }

        app_data.storage.add(root_agent);
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
        app_data: &mut AppData,
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

        app_data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
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
    }

    /// Spawn the actual child agents
    fn spawn_child_agents(
        self,
        app_data: &mut AppData,
        config: &SpawnConfig,
        count: usize,
        task: Option<&str>,
    ) -> Result<()> {
        let start_window_index = app_data
            .storage
            .reserve_window_indices(config.parent_agent_id);
        let program = if app_data.spawn.use_plan_prompt {
            app_data.planner_agent_spawn_command()
        } else {
            app_data.agent_spawn_command()
        };
        let child_prompt =
            task.map(|t| Self::build_child_prompt(t, app_data.spawn.use_plan_prompt));

        for i in 0..count {
            let window_index = start_window_index + u32::try_from(i).unwrap_or(0);
            self.spawn_single_child(
                app_data,
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
        app_data: &mut AppData,
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
            ChildConfig {
                parent_id: config.parent_agent_id,
                mux_session: config.root_session.clone(),
                window_index,
            },
        );

        let child_title = if app_data.spawn.use_plan_prompt && child_prompt.is_some() {
            format!("Planner {} ({})", index + 1, child.short_id())
        } else {
            format!("Agent {} ({})", index + 1, child.short_id())
        };
        let mut child = child;
        child.title.clone_from(&child_title);
        let cli = crate::conversation::detect_agent_cli(program);
        if cli == crate::conversation::AgentCli::Claude {
            child.conversation_id = Some(child.id.to_string());
        }

        let command = crate::conversation::build_spawn_argv(
            program,
            child_prompt,
            child.conversation_id.as_deref(),
        )?;
        let started_at = SystemTime::now();
        let actual_index = self.session_manager.create_window(
            &config.root_session,
            &child_title,
            &config.worktree_path,
            Some(&command),
        )?;
        if cli == crate::conversation::AgentCli::Codex {
            let exclude_ids: HashSet<String> = app_data
                .storage
                .iter()
                .filter_map(|stored| stored.conversation_id.clone())
                .collect();
            child.conversation_id = crate::conversation::try_detect_codex_session_id(
                &config.worktree_path,
                started_at,
                &exclude_ids,
                Duration::from_millis(500),
            );
        }

        if let Some((width, height)) = app_data.ui.preview_dimensions {
            let window_target = SessionManager::window_target(&config.root_session, actual_index);
            let _ = self
                .session_manager
                .resize_window(&window_target, width, height);
        }

        child.window_index = Some(actual_index);
        app_data.storage.add(child);

        Ok(())
    }

    /// Spawn child agents for an existing root agent
    ///
    /// This is a helper used by both `spawn_children` and `reconnect_to_worktree`
    pub(crate) fn spawn_children_for_root(
        self,
        app_data: &mut AppData,
        config: &SpawnConfig,
        count: usize,
        task: &str,
    ) -> Result<()> {
        self.spawn_child_agents(app_data, config, count, Some(task))?;

        // Expand the parent to show children
        if let Some(parent) = app_data.storage.get_mut(config.parent_agent_id) {
            parent.collapsed = false;
        }

        Ok(())
    }

    /// Spawn review agents for the selected agent against a base branch
    ///
    /// # Errors
    ///
    /// Returns an error if spawning fails
    pub fn spawn_review_agents(self, app_data: &mut AppData) -> Result<()> {
        let count = app_data.spawn.child_count;
        let parent_id = app_data
            .spawn
            .spawning_under
            .ok_or_else(|| anyhow::anyhow!("No agent selected for review"))?;
        let parent = app_data
            .storage
            .get(parent_id)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;
        if parent.is_terminal_agent() {
            bail!("Cannot spawn review agents under a terminal");
        }
        let base_branch = app_data
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
        let root = app_data
            .storage
            .root_ancestor(parent_id)
            .ok_or_else(|| anyhow::anyhow!("Parent agent not found"))?;

        let root_session = root.mux_session.clone();
        let worktree_path = root.worktree_path.clone();
        let branch = root.branch.clone();

        // Build the review prompt
        let review_prompt = prompts::build_review_prompt(&base_branch);

        // Reserve window indices
        let start_window_index = app_data.storage.reserve_window_indices(parent_id);
        let program = app_data.review_agent_spawn_command();

        // Create review child agents
        for i in 0..count {
            let offset = u32::try_from(i).map_or(u32::MAX, |value| value);
            let window_index = start_window_index.saturating_add(offset);
            let child = self.spawn_review_child_agent(
                app_data,
                ReviewChildAgentConfig {
                    root_session: root_session.as_str(),
                    worktree_path: worktree_path.as_path(),
                    branch: branch.as_str(),
                    parent_id,
                    program: program.as_str(),
                    review_prompt: review_prompt.as_str(),
                    base_branch: base_branch.as_str(),
                    reviewer_number: i + 1,
                    reserved_window_index: window_index,
                },
            )?;
            app_data.storage.add(child);
        }

        // Expand the parent to show children
        if let Some(parent) = app_data.storage.get_mut(parent_id) {
            parent.collapsed = false;
        }

        app_data.storage.save()?;
        info!(count, parent_id = %parent_id, base_branch, "Review agents spawned successfully");
        app_data.set_status(format!(
            "Spawned {count} review agents against {base_branch}"
        ));

        // Clear review state
        app_data.review.clear();

        Ok(())
    }

    /// Synthesize children into the parent agent
    ///
    /// Writes synthesis content to `.tenex/<id>.md` and tells the parent to read it.
    ///
    /// # Errors
    ///
    /// Returns an error if synthesis fails
    pub fn synthesize(self, app_data: &mut AppData) -> Result<AppMode> {
        self.synthesize_with_prompt(app_data, None)
    }

    /// Synthesize children into the parent agent with optional extra instructions.
    ///
    /// Writes synthesis content to `.tenex/<id>.md` and tells the parent to read it.
    ///
    /// # Errors
    ///
    /// Returns an error if synthesis fails
    pub fn synthesize_with_prompt(
        self,
        app_data: &mut AppData,
        prompt: Option<&str>,
    ) -> Result<AppMode> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ErrorModalMode {
                message: "No agent selected".to_string(),
            }
            .into());
        };

        if agent.is_terminal_agent() {
            return Ok(ErrorModalMode {
                message: "Cannot synthesize into a terminal agent".to_string(),
            }
            .into());
        }

        if !app_data.storage.has_children(agent.id) {
            warn!(agent_id = %agent.id, title = %agent.title, "No children to synthesize");
            return Ok(ErrorModalMode {
                message: "Selected agent has no children to synthesize".to_string(),
            }
            .into());
        }

        let parent_id = agent.id;
        let parent_session = agent.mux_session.clone();
        let parent_title = agent.title.clone();
        let parent_program = agent.program.clone();
        let worktree_path = agent.worktree_path.clone();
        // Determine the correct target for the parent
        // If the parent has a window_index, it's a child agent running in a window
        let parent_target = agent.window_index.map_or_else(
            || parent_session.clone(),
            |window_idx| SessionManager::window_target(&parent_session, window_idx),
        );

        info!(%parent_id, %parent_title, "Synthesizing descendants into parent");

        // Collect findings from all descendants (children, grandchildren, etc.)
        // Filter out terminal agents - they are interactive shells, not research agents
        let descendants: Vec<_> = app_data
            .storage
            .descendants(parent_id)
            .into_iter()
            .filter(|d| !d.is_terminal_agent())
            .collect();

        if descendants.is_empty() {
            warn!(agent_id = %parent_id, title = %parent_title, "No non-terminal children to synthesize");
            return Ok(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            }
            .into());
        }

        let mut findings: Vec<(String, String)> = Vec::new();

        for descendant in &descendants {
            // Capture terminal output from descendant's window
            let target = descendant.window_index.map_or_else(
                || descendant.mux_session.clone(),
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
            if let Some(idx) = window_idx
                && let Err(e) = self.session_manager.kill_window(&parent_session, idx)
            {
                warn!(
                    session = %parent_session,
                    window_index = idx,
                    error = %e,
                    "Failed to kill descendant mux window during synthesis cleanup"
                );
            }
            // Remove from storage (remove_with_descendants handles nested removal)
            app_data.storage.remove(descendant_id);
        }

        // Now tell the parent to read the file
        let read_command =
            Self::build_synthesis_read_command(synthesis_id, descendants_count, prompt);
        self.session_manager.send_keys_and_submit_for_program(
            &parent_target,
            &parent_program,
            &read_command,
        )?;

        app_data.validate_selection();
        app_data.storage.save()?;
        info!(%parent_title, descendants_count, "Synthesis complete");
        app_data.set_status("Synthesized findings into parent agent");
        Ok(AppMode::normal())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use crate::agent::Storage;
    use crate::app::Settings;
    use crate::config::Config;
    use crate::state::AppMode;
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
        );
        let root_id = root.id;
        app.data.storage.add(root);

        // Calling spawn_children_for_root should fail because the session doesn't exist
        let spawn_config = SpawnConfig {
            root_session: "nonexistent-session".to_string(),
            worktree_path: PathBuf::from("/tmp"),
            branch: "test-branch".to_string(),
            parent_agent_id: root_id,
        };
        let result = handler.spawn_children_for_root(&mut app.data, &spawn_config, 2, "test task");

        // This should error because the session doesn't exist
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_synthesize_no_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        // Should return error modal with no agent selected
        let next = handler.synthesize(&mut app.data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
        Ok(())
    }

    #[test]
    fn test_synthesize_no_children() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        app.data.storage.add(Agent::new(
            "test".to_string(),
            "claude".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        ));

        // Should set error when agent has no children
        let next = handler.synthesize(&mut app.data)?;
        app.apply_mode(next);
        assert!(app.data.ui.last_error.is_some());
        assert!(
            app.data
                .ui
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
        app.data.spawn.spawning_under = None;
        app.data.review.base_branch = Some("main".to_string());

        let result = handler.spawn_review_agents(&mut app.data);
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
        );
        let agent_id = agent.id;
        app.data.storage.add(agent);

        // spawning_under set but no base branch - should error
        app.data.spawn.spawning_under = Some(agent_id);
        app.data.review.base_branch = None;

        let result = handler.spawn_review_agents(&mut app.data);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_spawn_children_rejects_terminal_parent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        let terminal_id = terminal.id;
        app.data.storage.add(terminal);

        app.data.spawn.spawning_under = Some(terminal_id);

        match handler.spawn_children(&mut app.data, Some("test task")) {
            Ok(_) => return Err("expected terminal parent to be rejected".into()),
            Err(err) => assert!(err.to_string().contains("terminal")),
        }
        Ok(())
    }

    #[test]
    fn test_spawn_review_agents_rejects_terminal_parent() -> Result<(), Box<dyn std::error::Error>>
    {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        let terminal_id = terminal.id;
        app.data.storage.add(terminal);

        app.data.spawn.spawning_under = Some(terminal_id);
        app.data.review.base_branch = Some("main".to_string());

        match handler.spawn_review_agents(&mut app.data) {
            Ok(()) => return Err("expected terminal parent to be rejected".into()),
            Err(err) => assert!(err.to_string().contains("terminal")),
        }
        Ok(())
    }

    #[test]
    fn test_synthesize_terminal_agent() -> Result<(), Box<dyn std::error::Error>> {
        let handler = Actions::new();
        let (mut app, _temp) = create_test_app()?;

        let mut terminal = Agent::new(
            "terminal".to_string(),
            "terminal".to_string(),
            "muster/test".to_string(),
            PathBuf::from("/tmp"),
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        let next = handler.synthesize(&mut app.data)?;
        assert!(matches!(next, AppMode::ErrorModal(_)));
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
        );
        root.collapsed = false;
        let root_id = root.id;
        let root_session = root.mux_session.clone();
        app.data.storage.add(root);

        // Add a regular child agent
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session.clone(),
                window_index: 2,
            },
        );
        app.data.storage.add(child);

        // Add a terminal child (is_terminal = true)
        let mut terminal = Agent::new_child(
            "Terminal 1".to_string(),
            "terminal".to_string(),
            "tenex/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 3,
            },
        );
        terminal.is_terminal = true;
        app.data.storage.add(terminal);

        // Broadcast should only target the non-terminal child (1 agent)
        // Since mux sessions don't exist, it will fail but we can check
        // it attempts to send to the right number of agents
        let result = handler.broadcast_to_leaves(&mut app.data, "test");

        // The broadcast will "succeed" with 0 sent (sessions don't exist)
        // but importantly it should NOT error and should report 0 (not try terminal)
        assert!(result.is_ok());

        // Check status message mentions 0 or shows error about no agents
        // (since the mux sessions don't actually exist)
        Ok(())
    }
}
