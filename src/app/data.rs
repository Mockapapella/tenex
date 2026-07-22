//! Persistent application data that outlives mode transitions.

use super::{AgentProgram, Settings, Tab};
use crate::agent::{Agent, Status, Storage};
use crate::app::AgentRole;
use crate::app::SidebarItem;
use crate::app::state::{
    CommandPaletteState, GitOpState, InputState, ModelSelectorState, ReviewState,
    SettingsMenuState, SlashCommand, SpawnState, UiState,
};
use crate::config::Config;
use crate::state::{
    AppMode, ChangelogMode, CustomAgentCommandMode, ErrorModalMode, HelpMode, ModelSelectorMode,
    PreparingDockerMode, SettingsMenuMode,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynthesisTargets {
    pub marked: bool,
    pub capture_agent_ids: Vec<Uuid>,
    pub teardown_root_ids: Vec<Uuid>,
    pub teardown_agent_ids: Vec<Uuid>,
}

/// Persistent application data (everything except the current mode).
#[derive(Debug)]
pub struct AppData {
    /// Application configuration.
    pub config: Config,

    /// Agent storage.
    pub storage: Storage,

    /// Repository/workspace root for the process CWD (used to show an empty project header).
    pub cwd_project_root: Option<PathBuf>,

    /// Currently selected agent index (in visible agents list).
    pub selected: usize,

    /// Currently active tab in the detail pane.
    pub active_tab: Tab,

    /// Whether the application should quit.
    pub should_quit: bool,

    /// Input state (buffer, cursor, scroll).
    pub input: InputState,

    /// UI state (scroll positions, preview content, dimensions).
    pub ui: UiState,

    /// Git operation state (push, rename, PR).
    pub git_op: GitOpState,

    /// Review state (branch selection).
    pub review: ReviewState,

    /// Slash command palette state (`/`).
    pub command_palette: CommandPaletteState,

    /// Settings menu state (`/agents`).
    pub settings_menu: SettingsMenuState,

    /// Model selector state (`/agents`).
    pub model_selector: ModelSelectorState,

    /// Spawn state (child agent spawning).
    pub spawn: SpawnState,

    /// Transient synthesis marks for visible non-terminal descendants.
    pub(crate) synthesis_marks: Vec<Uuid>,

    /// User settings (persistent preferences).
    pub settings: Settings,

    /// Deferred changelog modal to show once the app returns to normal mode.
    pub pending_changelog: Option<crate::state::ChangelogMode>,

    /// Whether the terminal supports the keyboard enhancement protocol.
    pub keyboard_enhancement_supported: bool,
}

impl AppData {
    /// Create a new `AppData` with the given config, storage, and settings.
    #[must_use]
    pub const fn new(
        config: Config,
        storage: Storage,
        settings: Settings,
        keyboard_enhancement_supported: bool,
    ) -> Self {
        Self {
            config,
            storage,
            cwd_project_root: None,
            selected: 1,
            active_tab: Tab::Preview,
            should_quit: false,
            input: InputState::new(),
            ui: UiState::new(),
            git_op: GitOpState::new(),
            review: ReviewState::new(),
            command_palette: CommandPaletteState::new(),
            settings_menu: SettingsMenuState::new(),
            model_selector: ModelSelectorState::new(),
            spawn: SpawnState::new(),
            synthesis_marks: Vec::new(),
            settings,
            pending_changelog: None,
            keyboard_enhancement_supported,
        }
    }

    /// The base command used when spawning new agents (based on user settings).
    #[must_use]
    pub(crate) fn agent_spawn_command(&self) -> String {
        match self.settings.agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.config.default_program.clone(),
            AgentProgram::Custom => custom_agent_command_or_default(
                &self.settings.custom_agent_command,
                &self.config.default_program,
            ),
        }
    }

    /// The base command used when spawning planner agents (planning swarms).
    #[must_use]
    pub(crate) fn planner_agent_spawn_command(&self) -> String {
        match self.settings.planner_agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.config.default_program.clone(),
            AgentProgram::Custom => custom_agent_command_or_default(
                &self.settings.planner_custom_agent_command,
                &self.config.default_program,
            ),
        }
    }

    /// The base command used when spawning review agents (review swarms).
    #[must_use]
    pub(crate) fn review_agent_spawn_command(&self) -> String {
        match self.settings.review_agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.config.default_program.clone(),
            AgentProgram::Custom => custom_agent_command_or_default(
                &self.settings.review_custom_agent_command,
                &self.config.default_program,
            ),
        }
    }

    /// Get the currently selected agent (from visible agents list).
    #[must_use]
    pub(crate) fn selected_agent(&self) -> Option<&Agent> {
        match self.selected_sidebar_item() {
            Some(SidebarItem::Agent(agent)) => Some(agent.info.agent),
            _ => None,
        }
    }

    /// Check if there are any running agents.
    #[must_use]
    pub(crate) fn has_running_agents(&self) -> bool {
        self.storage.iter().any(|a| a.status == Status::Running)
    }

    /// Set a status message to display.
    pub(crate) fn set_status(&mut self, message: impl Into<String>) {
        self.ui.set_status(message);
    }

    pub(crate) fn synthesis_target_descendants(&self, parent_id: Uuid) -> Vec<&Agent> {
        self.storage
            .descendants(parent_id)
            .into_iter()
            .filter(|agent| !agent.is_terminal_agent())
            .collect()
    }

    pub(crate) fn synthesis_targets_for(&self, parent_id: Uuid) -> SynthesisTargets {
        let descendants = self.storage.descendants(parent_id);
        let marked_root_ids: Vec<Uuid> = descendants
            .iter()
            .filter(|agent| !agent.is_terminal_agent())
            .filter(|agent| self.synthesis_marks.contains(&agent.id))
            .map(|agent| agent.id)
            .collect();

        if marked_root_ids.is_empty() {
            let fallback_root_ids = descendants
                .iter()
                .filter(|agent| !agent.is_terminal_agent())
                .map(|agent| agent.id)
                .collect();
            self.synthesis_targets_from_roots(parent_id, fallback_root_ids, false)
        } else {
            self.synthesis_targets_from_roots(parent_id, marked_root_ids, true)
        }
    }

    fn synthesis_targets_from_roots(
        &self,
        parent_id: Uuid,
        root_ids: Vec<Uuid>,
        marked: bool,
    ) -> SynthesisTargets {
        let descendants = self.storage.descendants(parent_id);
        let subtree_ids = self.synthesis_subtree_ids(&root_ids);
        let capture_agent_ids = descendants
            .iter()
            .filter(|agent| subtree_ids.contains(&agent.id))
            .filter(|agent| !agent.is_terminal_agent())
            .map(|agent| agent.id)
            .collect();
        let teardown_agent_ids = descendants
            .iter()
            .filter(|agent| subtree_ids.contains(&agent.id))
            .map(|agent| agent.id)
            .collect();

        SynthesisTargets {
            marked,
            capture_agent_ids,
            teardown_root_ids: root_ids,
            teardown_agent_ids,
        }
    }

    fn synthesis_subtree_ids(&self, root_ids: &[Uuid]) -> HashSet<Uuid> {
        let mut subtree_ids = HashSet::new();
        for root_id in root_ids {
            subtree_ids.insert(*root_id);
            subtree_ids.extend(self.storage.descendant_ids(*root_id));
        }
        subtree_ids
    }

    pub(crate) fn is_synthesis_marked(&self, agent_id: Uuid) -> bool {
        self.synthesis_marks.contains(&agent_id)
    }

    pub(crate) fn clear_synthesis_marks(&mut self) {
        self.synthesis_marks.clear();
    }

    pub(crate) fn toggle_selected_synthesis_mark(&mut self) -> bool {
        let agent_id = match self.selected_sidebar_item() {
            Some(SidebarItem::Agent(agent)) => agent.info.agent.id,
            Some(SidebarItem::Project(_)) | None => return false,
        };
        self.toggle_synthesis_mark(agent_id)
    }

    pub(crate) fn toggle_synthesis_mark(&mut self, agent_id: Uuid) -> bool {
        if !self.is_synthesis_mark_eligible(agent_id) {
            return false;
        }

        if self.is_synthesis_marked(agent_id) {
            if let Some(index) = self
                .synthesis_marks
                .iter()
                .position(|marked_id| *marked_id == agent_id)
            {
                self.synthesis_marks.remove(index);
            }
        } else {
            self.synthesis_marks.push(agent_id);
        }
        true
    }

    fn is_synthesis_mark_eligible(&self, agent_id: Uuid) -> bool {
        let Some(agent) = self.storage.get(agent_id) else {
            return false;
        };
        let Some(parent_id) = agent.parent_id else {
            return false;
        };

        self.synthesis_target_descendants(parent_id)
            .into_iter()
            .any(|target| target.id == agent_id)
    }

    pub(crate) fn marked_synthesis_descendant_counts(&self) -> HashMap<Uuid, usize> {
        let mut counts = HashMap::new();

        for marked_id in &self.synthesis_marks {
            if !self.is_synthesis_mark_eligible(*marked_id) {
                continue;
            }

            let mut current_id = *marked_id;
            while let Some(agent) = self.storage.get(current_id) {
                let Some(parent_id) = agent.parent_id else {
                    break;
                };
                *counts.entry(parent_id).or_insert(0) += 1;
                current_id = parent_id;
            }
        }

        counts
    }

    pub(crate) fn select_cwd_project(&mut self) {
        let Some(cwd_root) = self.cwd_project_root.as_deref() else {
            return;
        };

        let items = self.sidebar_items();
        let mut header_index = 0usize;
        let mut first_agent_index: Option<usize> = None;

        for (idx, item) in items.iter().enumerate() {
            match item {
                SidebarItem::Project(project) if project.root == cwd_root => {
                    header_index = idx;
                }
                SidebarItem::Agent(agent) => {
                    let agent_root = agent
                        .info
                        .agent
                        .repo_root
                        .as_deref()
                        .unwrap_or(agent.info.agent.worktree_path.as_path());
                    if agent_root == cwd_root {
                        first_agent_index.get_or_insert(idx);
                    }
                }
                SidebarItem::Project(_) => {}
            }
        }

        let target = first_agent_index.unwrap_or(header_index);

        if target == self.selected {
            return;
        }

        self.selected = target;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    pub(crate) fn select_project_header(&mut self) {
        let Some(project_root) = self.selected_project_root() else {
            return;
        };

        let items = self.sidebar_items();
        let mut target = self.selected;
        for (idx, item) in items.iter().enumerate() {
            match item {
                SidebarItem::Project(project) if project.root == project_root => {
                    target = idx;
                    break;
                }
                SidebarItem::Project(_) | SidebarItem::Agent(_) => {}
            }
        }

        if target == self.selected {
            return;
        }

        self.selected = target;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    pub(crate) fn select_first_agent_in_selected_project(&mut self) {
        let Some(SidebarItem::Project(project)) = self.selected_sidebar_item() else {
            return;
        };
        let project_root = project.root;

        let items = self.sidebar_items();
        let mut in_project = false;
        let mut target: Option<usize> = None;

        for (idx, item) in items.iter().enumerate() {
            match item {
                SidebarItem::Project(project) => {
                    in_project = project.root == project_root;
                }
                SidebarItem::Agent(_) if in_project => {
                    target = Some(idx);
                    break;
                }
                SidebarItem::Agent(_) => {}
            }
        }

        let Some(target) = target else {
            return;
        };

        self.selected = target;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    /// Switch between detail pane tabs (forward).
    pub(crate) fn switch_tab(&mut self) {
        self.active_tab = match self.active_tab {
            Tab::Preview => Tab::Diff,
            Tab::Diff => Tab::Commits,
            Tab::Commits => Tab::Preview,
        };
        self.ui.reset_scroll();
    }

    /// Move selection to the next agent (in visible list).
    pub(crate) fn select_next(&mut self) {
        let items = self.sidebar_items();
        if items.is_empty() {
            return;
        }

        let next = if self.selected >= items.len() {
            0
        } else {
            (self.selected + 1) % items.len()
        };

        if next == self.selected {
            return;
        }

        self.selected = next;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    /// Move selection to the previous agent (in visible list).
    pub(crate) fn select_prev(&mut self) {
        let items = self.sidebar_items();
        if items.is_empty() {
            return;
        }

        let prev = if self.selected == 0 || self.selected >= items.len() {
            items.len() - 1
        } else {
            self.selected - 1
        };

        if prev == self.selected {
            return;
        }

        self.selected = prev;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    /// Select a specific agent by its ID, if present.
    pub(crate) fn select_agent_by_id(&mut self, agent_id: Uuid) {
        let items = self.sidebar_items();
        let Some(target) = items.iter().enumerate().find_map(|(idx, item)| match item {
            SidebarItem::Agent(agent) if agent.info.agent.id == agent_id => Some(idx),
            _ => None,
        }) else {
            return;
        };

        if target == self.selected {
            return;
        }

        self.selected = target;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    /// Ensure the agent list scroll offset keeps the selected agent visible.
    pub(crate) fn ensure_agent_list_scroll(&mut self) {
        let visible_count = self.sidebar_len();
        if visible_count == 0 {
            self.ui.agent_list_scroll = 0;
            return;
        }

        // `preview_dimensions` stores the preview inner height, which is `frame_height - 4`.
        // The agent list inner height is `frame_height - 3` (one line taller, because it has no tab bar).
        let preview_inner_height = usize::from(self.ui.preview_dimensions.map_or(20, |(_, h)| h));
        let viewport_height = preview_inner_height.saturating_add(1);
        let max_scroll = visible_count.saturating_sub(viewport_height);

        let mut scroll = self.ui.agent_list_scroll.min(max_scroll);

        if self.selected < scroll {
            scroll = self.selected;
        } else {
            let bottom = scroll.saturating_add(viewport_height).saturating_sub(1);
            if self.selected > bottom {
                scroll = self
                    .selected
                    .saturating_sub(viewport_height.saturating_sub(1));
            }
        }

        self.ui.agent_list_scroll = scroll.min(max_scroll);
    }

    /// Ensure the selection index is valid for the current visible agents.
    pub(crate) fn validate_selection(&mut self) {
        let visible_count = self.sidebar_len();
        if visible_count == 0 {
            self.selected = 0;
        } else if self.selected >= visible_count {
            self.selected = visible_count - 1;
        }
        self.ensure_agent_list_scroll();
    }

    /// Scroll up in the active pane by the given amount.
    pub(crate) fn scroll_up(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => self.ui.scroll_preview_up(amount),
            Tab::Diff => self.ui.scroll_diff_up(amount),
            Tab::Commits => self.ui.scroll_commits_up(amount),
        }
    }

    /// Scroll down in the active pane by the given amount.
    pub(crate) fn scroll_down(&mut self, amount: usize) {
        match self.active_tab {
            Tab::Preview => self.ui.scroll_preview_down(amount),
            Tab::Diff => self.ui.scroll_diff_down(amount),
            Tab::Commits => self.ui.scroll_commits_down(amount),
        }
    }

    /// Scroll to the top of the active pane.
    pub(crate) fn scroll_to_top(&mut self) {
        match self.active_tab {
            Tab::Preview => self.ui.preview_to_top(),
            Tab::Diff => self.ui.diff_to_top(),
            Tab::Commits => self.ui.commits_to_top(),
        }
    }

    /// Scroll to the bottom of the active pane.
    pub(crate) const fn scroll_to_bottom(&mut self, content_lines: usize, visible_lines: usize) {
        match self.active_tab {
            Tab::Preview => self.ui.preview_to_bottom(content_lines, visible_lines),
            Tab::Diff => self.ui.diff_to_bottom(content_lines, visible_lines),
            Tab::Commits => self.ui.commits_to_bottom(content_lines, visible_lines),
        }
    }

    /// Increment child count (for `ChildCountMode`).
    pub(crate) const fn increment_child_count(&mut self) {
        self.spawn.increment_child_count();
    }

    /// Decrement child count (minimum 1).
    pub(crate) const fn decrement_child_count(&mut self) {
        self.spawn.decrement_child_count();
    }

    /// Select next branch in filtered list.
    pub(crate) fn select_next_branch(&mut self) {
        self.review.select_next();
    }

    /// Select previous branch in filtered list.
    pub(crate) fn select_prev_branch(&mut self) {
        self.review.select_prev();
    }

    /// Handle character input in branch filter.
    pub(crate) fn handle_branch_filter_char(&mut self, c: char) {
        self.review.handle_filter_char(c);
    }

    /// Handle backspace in branch filter.
    pub(crate) fn handle_branch_filter_backspace(&mut self) {
        self.review.handle_filter_backspace();
    }

    /// Confirm branch selection and set `review.base_branch`.
    pub(crate) fn confirm_branch_selection(&mut self) -> bool {
        self.review.confirm_selection()
    }

    /// Confirm branch selection for rebase/merge and set `git_op.target_branch`.
    pub(crate) fn confirm_rebase_merge_branch(&mut self) -> bool {
        if let Some(branch) = self.review.selected_branch() {
            let target = if branch.is_remote {
                branch.remote.as_deref().map_or_else(
                    || branch.name.clone(),
                    |remote| format!("{remote}/{}", branch.name),
                )
            } else {
                branch.name.clone()
            };
            self.git_op.set_target_branch(target);
            true
        } else {
            false
        }
    }

    /// Select next model/program in filtered list.
    pub(crate) fn select_next_model_program(&mut self) {
        self.model_selector.select_next();
    }

    /// Select previous model/program in filtered list.
    pub(crate) fn select_prev_model_program(&mut self) {
        self.model_selector.select_prev();
    }

    /// Handle typing in the `/agents` filter.
    pub(crate) fn handle_model_filter_char(&mut self, c: char) {
        self.model_selector.handle_filter_char(c);
    }

    /// Handle backspace in the `/agents` filter.
    pub(crate) fn handle_model_filter_backspace(&mut self) {
        self.model_selector.handle_filter_backspace();
    }

    /// Confirm the current `/agents` selection and return the next mode.
    pub(crate) fn confirm_model_program_selection(&mut self) -> AppMode {
        let Some(program) = self.model_selector.selected_program() else {
            return AppMode::normal();
        };

        let role = self.model_selector.role;

        match program {
            AgentProgram::Custom => CustomAgentCommandMode.into(),
            other => {
                match role {
                    AgentRole::Default => {
                        self.settings.agent_program = other;
                    }
                    AgentRole::Planner => {
                        self.settings.planner_agent_program = other;
                    }
                    AgentRole::Review => {
                        self.settings.review_agent_program = other;
                    }
                }

                if let Err(err) = self.settings.save() {
                    return crate::state::ErrorModalMode {
                        message: format!("Failed to save settings: {err}"),
                    }
                    .into();
                }

                self.set_status(format!("{} set to {}", role.menu_label(), other.label()));
                AppMode::normal()
            }
        }
    }

    /// Return the list of slash commands filtered by the current palette input.
    #[must_use]
    pub(crate) fn filtered_slash_commands(&self) -> Vec<crate::app::state::SlashCommand> {
        let raw = self.input.buffer.trim();
        let query = raw
            .strip_prefix('/')
            .unwrap_or(raw)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        crate::app::state::SLASH_COMMANDS
            .iter()
            .copied()
            .filter(|cmd| {
                query.is_empty()
                    || cmd
                        .name
                        .trim_start_matches('/')
                        .to_ascii_lowercase()
                        .starts_with(&query)
            })
            .collect()
    }

    fn run_slash_command_by_name(&mut self, command_name: &str) -> AppMode {
        match command_name {
            "/agents" => {
                self.input.clear();
                self.model_selector.role = AgentRole::Default;
                SettingsMenuMode.into()
            }
            "/toggle_docker" => self.toggle_docker_for_new_roots(),
            "/changelog" => {
                self.input.clear();
                match crate::release_notes::current_version()
                    .and_then(|version| crate::release_notes::changelog_lines_for_version(&version))
                {
                    Ok(lines) => ChangelogMode {
                        title: "Changelog".to_string(),
                        lines,
                        mark_seen_version: None,
                    }
                    .into(),
                    Err(e) => {
                        self.set_status(format!("Failed to load changelog: {e}"));
                        AppMode::normal()
                    }
                }
            }
            "/help" => {
                self.ui.help_scroll = 0;
                HelpMode.into()
            }
            other => {
                self.set_status(format!("Unknown command: {other}"));
                AppMode::normal()
            }
        }
    }

    fn run_typed_slash_command(&mut self, commands: &[SlashCommand]) -> AppMode {
        let typed = self
            .input
            .buffer
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();

        if typed.is_empty() || typed == "/" {
            return AppMode::normal();
        }

        let normalized = if typed.starts_with('/') {
            typed.to_ascii_lowercase()
        } else {
            format!("/{typed}").to_ascii_lowercase()
        };

        let cmd = if let Some(cmd) = commands
            .iter()
            .copied()
            .find(|c| c.name.eq_ignore_ascii_case(&normalized))
        {
            cmd
        } else {
            let query = normalized.trim_start_matches('/').to_string();
            let matches: Vec<SlashCommand> = commands
                .iter()
                .copied()
                .filter(|c| {
                    c.name
                        .trim_start_matches('/')
                        .to_ascii_lowercase()
                        .starts_with(&query)
                })
                .collect();

            match matches.as_slice() {
                [] => {
                    self.set_status(format!("Unknown command: {typed}"));
                    return AppMode::normal();
                }
                [single] => *single,
                _ => {
                    self.set_status(format!("Ambiguous command: {typed}"));
                    return AppMode::normal();
                }
            }
        };

        self.run_slash_command_by_name(cmd.name)
    }

    /// Select the next slash command in the filtered list.
    pub(crate) fn select_next_slash_command(&mut self) {
        let count = self.filtered_slash_commands().len();
        if count > 0 {
            self.command_palette.selected = (self.command_palette.selected + 1) % count;
        } else {
            self.command_palette.selected = 0;
        }
    }

    /// Select the previous slash command in the filtered list.
    pub(crate) fn select_prev_slash_command(&mut self) {
        let count = self.filtered_slash_commands().len();
        if count > 0 {
            self.command_palette.selected = self
                .command_palette
                .selected
                .checked_sub(1)
                .unwrap_or(count - 1);
        } else {
            self.command_palette.selected = 0;
        }
    }

    /// Select the next settings menu item.
    pub(crate) const fn select_next_settings_menu_item(&mut self) {
        self.settings_menu.select_next();
    }

    /// Select the previous settings menu item.
    pub(crate) const fn select_prev_settings_menu_item(&mut self) {
        self.settings_menu.select_prev();
    }

    /// Confirm the current settings menu selection and return the next mode.
    pub(crate) fn confirm_settings_menu_selection(&mut self) -> AppMode {
        self.model_selector.role = self.settings_menu.selected_role();
        ModelSelectorMode.into()
    }

    /// Reset the slash command selection back to the first entry.
    pub(crate) const fn reset_slash_command_selection(&mut self) {
        self.command_palette.selected = 0;
    }

    fn docker_runtime_programs(&self) -> [String; 3] {
        [
            self.agent_spawn_command(),
            self.planner_agent_spawn_command(),
            self.review_agent_spawn_command(),
        ]
    }

    fn persist_docker_for_new_roots(
        &mut self,
        previous: bool,
        enabled: bool,
        status_message: impl Into<String>,
    ) -> AppMode {
        self.settings.docker_for_new_roots = enabled;

        if let Err(err) = self.settings.save() {
            self.settings.docker_for_new_roots = previous;
            return ErrorModalMode {
                message: format!("Failed to save settings: {err}"),
            }
            .into();
        }

        self.input.clear();
        self.set_status(status_message);
        AppMode::normal()
    }

    pub(crate) fn toggle_docker_for_new_roots(&mut self) -> AppMode {
        let previous = self.settings.docker_for_new_roots;
        if previous {
            return self.persist_docker_for_new_roots(
                previous,
                false,
                "Docker for new root agents: OFF",
            );
        }

        let programs = self.docker_runtime_programs();
        let program_refs = programs.iter().map(String::as_str).collect::<Vec<_>>();
        match crate::runtime::inspect_docker_runtime(&self.settings, &program_refs) {
            Ok(crate::runtime::DockerPreparation::Ready) => self.persist_docker_for_new_roots(
                previous,
                true,
                "Docker for new root agents: ON",
            ),
            Ok(crate::runtime::DockerPreparation::NeedsImageBuild) => PreparingDockerMode {
                message: "Building the shipped Tenex Docker worker image. This can take a minute the first time, and the image will be reused for future root agents.".to_string(),
            }
            .into(),
            Err(err) => ErrorModalMode {
                message: format!("Cannot enable Docker for new root agents: {err}"),
            }
            .into(),
        }
    }

    pub(crate) fn finish_preparing_docker_for_new_roots(&mut self) -> AppMode {
        let programs = self.docker_runtime_programs();
        let program_refs = programs.iter().map(String::as_str).collect::<Vec<_>>();
        if let Err(err) = crate::runtime::prepare_docker_runtime(&self.settings, &program_refs) {
            return ErrorModalMode {
                message: format!("Cannot enable Docker for new root agents: {err}"),
            }
            .into();
        }

        self.persist_docker_for_new_roots(
            false,
            true,
            "Docker for new root agents: ON. Worker image ready and will be reused for future root agents.",
        )
    }

    /// Run the currently highlighted command in the palette (fallbacks to parsing the input).
    pub(crate) fn confirm_slash_command_selection(&mut self) -> AppMode {
        let selected = self
            .filtered_slash_commands()
            .get(self.command_palette.selected)
            .copied();
        if let Some(cmd) = selected {
            return self.run_slash_command_by_name(cmd.name);
        }

        self.run_typed_slash_command(crate::app::state::SLASH_COMMANDS)
    }

    /// Execute the currently-typed slash command (ignores the highlighted selection).
    pub fn submit_slash_command_palette(&mut self) -> AppMode {
        self.run_typed_slash_command(crate::app::state::SLASH_COMMANDS)
    }

    /// Insert a character into the input buffer.
    pub(crate) fn handle_char(&mut self, c: char) {
        self.input.insert_char(c);
    }

    /// Handle backspace in the input buffer.
    pub(crate) fn handle_backspace(&mut self) {
        self.input.backspace();
    }

    /// Handle delete in the input buffer.
    pub(crate) fn handle_delete(&mut self) {
        self.input.delete();
    }
}

fn custom_agent_command_or_default(custom: &str, default_program: &str) -> String {
    let custom = custom.trim();
    if custom.is_empty() {
        default_program.to_string()
    } else {
        custom.to_string()
    }
}
