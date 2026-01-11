//! Persistent application data that outlives mode transitions.

use super::{AgentProgram, Settings, Tab};
use crate::agent::{Agent, Status, Storage};
use crate::app::AgentRole;
use crate::app::state::{
    CommandPaletteState, GitOpState, InputState, ModelSelectorState, ReviewState,
    SettingsMenuState, SpawnState, UiState,
};
use crate::config::Config;
use crate::state::{
    AppMode, CustomAgentCommandMode, HelpMode, ModelSelectorMode, SettingsMenuMode,
};

/// Persistent application data (everything except the current mode).
#[derive(Debug)]
pub struct AppData {
    /// Application configuration.
    pub config: Config,

    /// Agent storage.
    pub storage: Storage,

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

    /// User settings (persistent preferences).
    pub settings: Settings,

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
            selected: 0,
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
            settings,
            keyboard_enhancement_supported,
        }
    }

    /// The base command used when spawning new agents (based on user settings).
    #[must_use]
    pub(crate) fn agent_spawn_command(&self) -> String {
        match self.settings.agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.config.default_program.clone(),
            AgentProgram::Custom => {
                let custom = self.settings.custom_agent_command.trim();
                if custom.is_empty() {
                    self.config.default_program.clone()
                } else {
                    custom.to_string()
                }
            }
        }
    }

    /// The base command used when spawning planner agents (planning swarms).
    #[must_use]
    pub(crate) fn planner_agent_spawn_command(&self) -> String {
        match self.settings.planner_agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.config.default_program.clone(),
            AgentProgram::Custom => {
                let custom = self.settings.planner_custom_agent_command.trim();
                if custom.is_empty() {
                    self.config.default_program.clone()
                } else {
                    custom.to_string()
                }
            }
        }
    }

    /// The base command used when spawning review agents (review swarms).
    #[must_use]
    pub(crate) fn review_agent_spawn_command(&self) -> String {
        match self.settings.review_agent_program {
            AgentProgram::Codex => "codex".to_string(),
            AgentProgram::Claude => self.config.default_program.clone(),
            AgentProgram::Custom => {
                let custom = self.settings.review_custom_agent_command.trim();
                if custom.is_empty() {
                    self.config.default_program.clone()
                } else {
                    custom.to_string()
                }
            }
        }
    }

    /// Get the currently selected agent (from visible agents list).
    #[must_use]
    pub(crate) fn selected_agent(&self) -> Option<&Agent> {
        self.storage.visible_agent_at(self.selected)
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
        let visible_count = self.storage.visible_count();
        if visible_count == 0 {
            return;
        }

        self.selected = (self.selected + 1) % visible_count;
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    /// Move selection to the previous agent (in visible list).
    pub(crate) fn select_prev(&mut self) {
        let visible_count = self.storage.visible_count();
        if visible_count == 0 {
            return;
        }

        self.selected = self.selected.checked_sub(1).unwrap_or(visible_count - 1);
        self.ui.reset_scroll();
        self.ui.reset_diff_interaction();
        self.ensure_agent_list_scroll();
    }

    /// Ensure the agent list scroll offset keeps the selected agent visible.
    pub(crate) fn ensure_agent_list_scroll(&mut self) {
        let visible_count = self.storage.visible_count();
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
        let visible_count = self.storage.visible_count();
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
            self.git_op.set_target_branch(branch.name.clone());
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

    /// Run the currently highlighted command in the palette (fallbacks to parsing the input).
    pub(crate) fn confirm_slash_command_selection(&mut self) -> AppMode {
        let selected = self
            .filtered_slash_commands()
            .get(self.command_palette.selected)
            .copied();

        let cmd = if let Some(cmd) = selected {
            cmd
        } else {
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

            if let Some(cmd) = crate::app::state::SLASH_COMMANDS
                .iter()
                .copied()
                .find(|c| c.name.eq_ignore_ascii_case(&normalized))
            {
                cmd
            } else {
                let query = normalized.trim_start_matches('/').to_string();
                let matches: Vec<crate::app::state::SlashCommand> =
                    crate::app::state::SLASH_COMMANDS
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
            }
        };

        match cmd.name {
            "/agents" => {
                self.input.clear();
                self.model_selector.role = AgentRole::Default;
                SettingsMenuMode.into()
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

    /// Execute the currently-typed slash command (ignores the highlighted selection).
    pub fn submit_slash_command_palette(&mut self) -> AppMode {
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

        let cmd = if let Some(cmd) = crate::app::state::SLASH_COMMANDS
            .iter()
            .copied()
            .find(|c| c.name.eq_ignore_ascii_case(&normalized))
        {
            cmd
        } else {
            let query = normalized.trim_start_matches('/').to_string();
            let matches: Vec<crate::app::state::SlashCommand> = crate::app::state::SLASH_COMMANDS
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

        match cmd.name {
            "/agents" => {
                self.input.clear();
                self.model_selector.role = AgentRole::Default;
                SettingsMenuMode.into()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Storage};
    use crate::git::BranchInfo;
    use std::path::PathBuf;

    fn make_agent(title: &str) -> Agent {
        let pid = std::process::id();
        Agent::new(
            title.to_string(),
            "echo".to_string(),
            format!("tenex-app-data-test-{pid}/{title}"),
            PathBuf::from(format!("/tmp/tenex-app-data-test-{pid}/{title}")),
        )
    }

    fn make_local_branch(name: &str) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            full_name: format!("refs/heads/{name}"),
            is_remote: false,
            remote: None,
            last_commit_time: None,
        }
    }

    #[test]
    fn test_agent_spawn_command_codex() {
        let settings = Settings {
            agent_program: AgentProgram::Codex,
            ..Settings::default()
        };

        let data = AppData::new(Config::default(), Storage::default(), settings, false);
        assert_eq!(data.agent_spawn_command(), "codex");
    }

    #[test]
    fn test_agent_spawn_command_claude_uses_config_default() {
        let settings = Settings {
            agent_program: AgentProgram::Claude,
            ..Settings::default()
        };

        let config = Config::default();
        let expected = config.default_program.clone();

        let data = AppData::new(config, Storage::default(), settings, false);
        assert_eq!(data.agent_spawn_command(), expected);
    }

    #[test]
    fn test_agent_spawn_command_custom_falls_back_when_empty() {
        let settings = Settings {
            agent_program: AgentProgram::Custom,
            custom_agent_command: "   ".to_string(),
            ..Settings::default()
        };

        let config = Config::default();
        let expected = config.default_program.clone();

        let data = AppData::new(config, Storage::default(), settings, false);
        assert_eq!(data.agent_spawn_command(), expected);
    }

    #[test]
    fn test_agent_spawn_command_custom_uses_trimmed_command() {
        let settings = Settings {
            agent_program: AgentProgram::Custom,
            custom_agent_command: "  my-agent  ".to_string(),
            ..Settings::default()
        };

        let data = AppData::new(Config::default(), Storage::default(), settings, false);
        assert_eq!(data.agent_spawn_command(), "my-agent");
    }

    #[test]
    fn test_planner_agent_spawn_command_codex() {
        let settings = Settings {
            planner_agent_program: AgentProgram::Codex,
            ..Settings::default()
        };

        let data = AppData::new(Config::default(), Storage::default(), settings, false);
        assert_eq!(data.planner_agent_spawn_command(), "codex");
    }

    #[test]
    fn test_planner_agent_spawn_command_claude_uses_config_default() {
        let settings = Settings {
            planner_agent_program: AgentProgram::Claude,
            ..Settings::default()
        };

        let config = Config::default();
        let expected = config.default_program.clone();

        let data = AppData::new(config, Storage::default(), settings, false);
        assert_eq!(data.planner_agent_spawn_command(), expected);
    }

    #[test]
    fn test_planner_agent_spawn_command_custom_falls_back_when_empty() {
        let settings = Settings {
            planner_agent_program: AgentProgram::Custom,
            planner_custom_agent_command: "   ".to_string(),
            ..Settings::default()
        };

        let config = Config::default();
        let expected = config.default_program.clone();

        let data = AppData::new(config, Storage::default(), settings, false);
        assert_eq!(data.planner_agent_spawn_command(), expected);
    }

    #[test]
    fn test_planner_agent_spawn_command_custom_uses_trimmed_command() {
        let settings = Settings {
            planner_agent_program: AgentProgram::Custom,
            planner_custom_agent_command: "  my-agent  ".to_string(),
            ..Settings::default()
        };

        let data = AppData::new(Config::default(), Storage::default(), settings, false);
        assert_eq!(data.planner_agent_spawn_command(), "my-agent");
    }

    #[test]
    fn test_review_agent_spawn_command_codex() {
        let settings = Settings {
            review_agent_program: AgentProgram::Codex,
            ..Settings::default()
        };

        let data = AppData::new(Config::default(), Storage::default(), settings, false);
        assert_eq!(data.review_agent_spawn_command(), "codex");
    }

    #[test]
    fn test_review_agent_spawn_command_claude_uses_config_default() {
        let settings = Settings {
            review_agent_program: AgentProgram::Claude,
            ..Settings::default()
        };

        let config = Config::default();
        let expected = config.default_program.clone();

        let data = AppData::new(config, Storage::default(), settings, false);
        assert_eq!(data.review_agent_spawn_command(), expected);
    }

    #[test]
    fn test_review_agent_spawn_command_custom_falls_back_when_empty() {
        let settings = Settings {
            review_agent_program: AgentProgram::Custom,
            review_custom_agent_command: "   ".to_string(),
            ..Settings::default()
        };

        let config = Config::default();
        let expected = config.default_program.clone();

        let data = AppData::new(config, Storage::default(), settings, false);
        assert_eq!(data.review_agent_spawn_command(), expected);
    }

    #[test]
    fn test_review_agent_spawn_command_custom_uses_trimmed_command() {
        let settings = Settings {
            review_agent_program: AgentProgram::Custom,
            review_custom_agent_command: "  my-agent  ".to_string(),
            ..Settings::default()
        };

        let data = AppData::new(Config::default(), Storage::default(), settings, false);
        assert_eq!(data.review_agent_spawn_command(), "my-agent");
    }

    #[test]
    fn test_select_next_no_agents_is_noop() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.selected = 42;
        data.ui.agent_list_scroll = 7;
        data.ui.preview_scroll = 0;
        data.select_next();

        assert_eq!(data.selected, 42);
        assert_eq!(data.ui.agent_list_scroll, 7);
        assert_eq!(data.ui.preview_scroll, 0);
    }

    #[test]
    fn test_select_next_wraps_and_resets_scroll() {
        let mut storage = Storage::new();
        storage.add(make_agent("agent-1"));
        storage.add(make_agent("agent-2"));

        let mut data = AppData::new(Config::default(), storage, Settings::default(), false);
        data.ui.preview_scroll = 0;
        data.ui.diff_scroll = 42;

        data.select_next();
        assert_eq!(data.selected, 1);
        assert_eq!(data.ui.preview_scroll, usize::MAX);
        assert_eq!(data.ui.diff_scroll, 0);

        data.ui.preview_scroll = 0;
        data.select_next();
        assert_eq!(data.selected, 0);
        assert_eq!(data.ui.preview_scroll, usize::MAX);
    }

    #[test]
    fn test_select_prev_wraps_and_resets_scroll() {
        let mut storage = Storage::new();
        storage.add(make_agent("agent-1"));
        storage.add(make_agent("agent-2"));

        let mut data = AppData::new(Config::default(), storage, Settings::default(), false);
        data.selected = 0;
        data.ui.preview_scroll = 0;

        data.select_prev();
        assert_eq!(data.selected, 1);
        assert_eq!(data.ui.preview_scroll, usize::MAX);
    }

    #[test]
    fn test_validate_selection_clamps_to_last_visible_agent() {
        let mut storage = Storage::new();
        storage.add(make_agent("agent-1"));
        storage.add(make_agent("agent-2"));

        let mut data = AppData::new(Config::default(), storage, Settings::default(), false);
        data.selected = 100;
        data.validate_selection();
        assert_eq!(data.selected, 1);
    }

    #[test]
    fn test_ensure_agent_list_scroll_scrolls_up_when_selected_above_viewport() {
        let mut storage = Storage::new();
        for i in 0..10 {
            storage.add(make_agent(&format!("agent-{i:02}")));
        }

        let mut data = AppData::new(Config::default(), storage, Settings::default(), false);
        data.ui.set_preview_dimensions(80, 2);
        data.ui.agent_list_scroll = 5;
        data.selected = 3;

        data.ensure_agent_list_scroll();
        assert_eq!(data.ui.agent_list_scroll, 3);
    }

    #[test]
    fn test_ensure_agent_list_scroll_scrolls_down_when_selected_below_viewport() {
        let mut storage = Storage::new();
        for i in 0..10 {
            storage.add(make_agent(&format!("agent-{i:02}")));
        }

        let mut data = AppData::new(Config::default(), storage, Settings::default(), false);
        data.ui.set_preview_dimensions(80, 2);
        data.ui.agent_list_scroll = 0;
        data.selected = 9;

        data.ensure_agent_list_scroll();
        assert_eq!(data.ui.agent_list_scroll, 7);
    }

    #[test]
    fn test_confirm_rebase_merge_branch_sets_target_branch() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.review.branches = vec![make_local_branch("main")];
        data.review.selected = 0;

        assert!(data.confirm_rebase_merge_branch());
        assert_eq!(data.git_op.target_branch, "main");
    }

    #[test]
    fn test_confirm_rebase_merge_branch_returns_false_when_no_branches() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.review.branches = Vec::new();

        assert!(!data.confirm_rebase_merge_branch());
        assert!(data.git_op.target_branch.is_empty());
    }

    #[test]
    fn test_filtered_slash_commands_filters_by_prefix_without_leading_slash() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.input.buffer = "/he".to_string();

        let filtered = data.filtered_slash_commands();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "/help");
    }

    #[test]
    fn test_confirm_slash_command_selection_uses_highlighted_entry() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.input.buffer = "/".to_string();
        data.command_palette.selected = 1;
        data.ui.help_scroll = 123;

        let next = data.confirm_slash_command_selection();
        assert!(matches!(next, AppMode::Help(_)));
        assert_eq!(data.ui.help_scroll, 0);
    }

    #[test]
    fn test_confirm_slash_command_selection_unknown_sets_status() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.input.buffer = "/nope".to_string();

        let next = data.confirm_slash_command_selection();
        assert_eq!(next, AppMode::normal());
        assert!(
            data.ui
                .status_message
                .as_ref()
                .is_some_and(|msg| msg.contains("Unknown command"))
        );
    }

    #[test]
    fn test_submit_slash_command_palette_help_opens_help() {
        let mut data = AppData::new(
            Config::default(),
            Storage::default(),
            Settings::default(),
            false,
        );
        data.input.buffer = "/help".to_string();
        data.ui.help_scroll = 123;

        let next = data.submit_slash_command_palette();
        assert!(matches!(next, AppMode::Help(_)));
        assert_eq!(data.ui.help_scroll, 0);
    }
}
