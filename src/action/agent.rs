use crate::action::ValidIn;
use crate::app::{Actions, AppData, SidebarItem, Tab};
use crate::git;
use crate::state::{
    AppMode, BroadcastingMode, ChildCountMode, ConfirmAction, ConfirmingMode, CreatingMode,
    DiffFocusedMode, ErrorModalMode, NormalMode, PromptingMode, ReviewChildCountMode,
    ReviewInfoMode, ScrollingMode, TerminalPromptMode,
};
use anyhow::Result;

/// Normal-mode action: enter agent creation mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct NewAgentAction;

impl ValidIn<NormalMode> for NewAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(CreatingMode.into())
    }
}

impl ValidIn<ScrollingMode> for NewAgentAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(CreatingMode.into())
    }
}

/// Normal-mode action: enter agent creation mode with an initial prompt.
#[derive(Debug, Clone, Copy, Default)]
pub struct NewAgentWithPromptAction;

impl ValidIn<NormalMode> for NewAgentWithPromptAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(PromptingMode.into())
    }
}

impl ValidIn<ScrollingMode> for NewAgentWithPromptAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, _app_data: &mut AppData) -> Result<Self::NextState> {
        Ok(PromptingMode.into())
    }
}

/// Normal-mode action: kill the selected agent (enters confirmation).
#[derive(Debug, Clone, Copy, Default)]
pub struct KillAction;

impl ValidIn<NormalMode> for KillAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into())
        } else {
            Ok(AppMode::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for KillAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into())
        } else {
            Ok(ScrollingMode.into())
        }
    }
}

/// Normal-mode action: start spawning child agents from the root.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnChildrenAction;

impl ValidIn<NormalMode> for SpawnChildrenAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.spawn.start_spawning_root();
        app_data.spawn.root_repo_path = app_data.selected_project_root();
        Ok(ChildCountMode.into())
    }
}

impl ValidIn<ScrollingMode> for SpawnChildrenAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        app_data.spawn.start_spawning_root();
        app_data.spawn.root_repo_path = app_data.selected_project_root();
        Ok(ChildCountMode.into())
    }
}

/// Normal-mode action: start the swarm planner (child-count picker).
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanSwarmAction;

impl ValidIn<NormalMode> for PlanSwarmAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            app_data.set_status("Select an agent first (press 'a')");
            return Ok(AppMode::normal());
        };

        if agent.is_terminal_agent() {
            app_data.set_status("Select a non-terminal agent first (press 'a')");
            return Ok(AppMode::normal());
        }

        app_data.spawn.start_planning_swarm_under(agent.id);
        Ok(ChildCountMode.into())
    }
}

impl ValidIn<ScrollingMode> for PlanSwarmAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            app_data.set_status("Select an agent first (press 'a')");
            return Ok(ScrollingMode.into());
        };

        if agent.is_terminal_agent() {
            app_data.set_status("Select a non-terminal agent first (press 'a')");
            return Ok(ScrollingMode.into());
        }

        app_data.spawn.start_planning_swarm_under(agent.id);
        Ok(ChildCountMode.into())
    }
}

/// Normal-mode action: add children under the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct AddChildrenAction;

impl ValidIn<NormalMode> for AddChildrenAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(AppMode::normal());
        };

        if agent.is_terminal_agent() {
            app_data.set_status("Cannot spawn children under a terminal");
            return Ok(AppMode::normal());
        }

        app_data.spawn.start_spawning_under(agent.id);
        Ok(ChildCountMode.into())
    }
}

impl ValidIn<ScrollingMode> for AddChildrenAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ScrollingMode.into());
        };

        if agent.is_terminal_agent() {
            app_data.set_status("Cannot spawn children under a terminal");
            return Ok(ScrollingMode.into());
        }

        app_data.spawn.start_spawning_under(agent.id);
        Ok(ChildCountMode.into())
    }
}

/// Normal-mode action: synthesize children into the selected agent (enters confirmation).
#[derive(Debug, Clone, Copy, Default)]
pub struct SynthesizeAction;

impl ValidIn<NormalMode> for SynthesizeAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(AppMode::normal());
        };

        if agent.is_terminal_agent() {
            return Ok(ErrorModalMode {
                message: "Cannot synthesize into a terminal agent".to_string(),
            }
            .into());
        }

        if !app_data.storage.has_children(agent.id) {
            Ok(ErrorModalMode {
                message: "Selected agent has no children to synthesize".to_string(),
            }
            .into())
        } else if app_data
            .storage
            .descendants(agent.id)
            .into_iter()
            .any(|a| !a.is_terminal_agent())
        {
            Ok(ConfirmingMode {
                action: ConfirmAction::Synthesize,
            }
            .into())
        } else {
            Ok(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            }
            .into())
        }
    }
}

impl ValidIn<ScrollingMode> for SynthesizeAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ScrollingMode.into());
        };

        if agent.is_terminal_agent() {
            return Ok(ErrorModalMode {
                message: "Cannot synthesize into a terminal agent".to_string(),
            }
            .into());
        }

        if !app_data.storage.has_children(agent.id) {
            Ok(ErrorModalMode {
                message: "Selected agent has no children to synthesize".to_string(),
            }
            .into())
        } else if app_data
            .storage
            .descendants(agent.id)
            .into_iter()
            .any(|a| !a.is_terminal_agent())
        {
            Ok(ConfirmingMode {
                action: ConfirmAction::Synthesize,
            }
            .into())
        } else {
            Ok(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            }
            .into())
        }
    }
}

/// Normal-mode action: toggle collapse state of the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToggleCollapseAction;

impl ValidIn<NormalMode> for ToggleCollapseAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        match app_data.selected_sidebar_item() {
            Some(SidebarItem::Project(project)) => {
                app_data.ui.toggle_project_collapsed(&project.root);
                app_data.ensure_agent_list_scroll();
            }
            Some(SidebarItem::Agent(agent)) => {
                let agent_id = agent.info.agent.id;
                if app_data.storage.has_children(agent_id) {
                    for stored_agent in &mut app_data.storage.agents {
                        if stored_agent.id == agent_id {
                            stored_agent.collapsed = !stored_agent.collapsed;
                            break;
                        }
                    }
                    app_data.ensure_agent_list_scroll();
                }
            }
            None => {}
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for ToggleCollapseAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        match app_data.selected_sidebar_item() {
            Some(SidebarItem::Project(project)) => {
                app_data.ui.toggle_project_collapsed(&project.root);
                app_data.ensure_agent_list_scroll();
            }
            Some(SidebarItem::Agent(agent)) => {
                let agent_id = agent.info.agent.id;
                if app_data.storage.has_children(agent_id) {
                    for stored_agent in &mut app_data.storage.agents {
                        if stored_agent.id == agent_id {
                            stored_agent.collapsed = !stored_agent.collapsed;
                            break;
                        }
                    }
                    app_data.ensure_agent_list_scroll();
                }
            }
            None => {}
        }
        Ok(ScrollingMode.into())
    }
}

impl ValidIn<DiffFocusedMode> for ToggleCollapseAction {
    type NextState = AppMode;

    fn execute(self, _state: DiffFocusedMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.active_tab == Tab::Diff {
            let _ = app_data.ui.toggle_diff_fold_at_cursor();
        }
        Ok(DiffFocusedMode.into())
    }
}

/// Normal-mode action: enter broadcasting mode for the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct BroadcastAction;

impl ValidIn<NormalMode> for BroadcastAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(BroadcastingMode.into())
        } else {
            Ok(AppMode::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for BroadcastAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(BroadcastingMode.into())
        } else {
            Ok(ScrollingMode.into())
        }
    }
}

/// Normal-mode action: start the review swarm flow.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReviewSwarmAction;

impl ValidIn<NormalMode> for ReviewSwarmAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(selected) = app_data.selected_agent() else {
            return Ok(ReviewInfoMode.into());
        };

        let selected_id = selected.id;
        let selected_worktree_path = selected.worktree_path.clone();
        let selected_is_terminal = selected.is_terminal_agent();

        if selected_is_terminal {
            app_data.set_status("Select a non-terminal agent for review swarm");
            return Ok(AppMode::normal());
        }

        // Store the selected agent's ID for later use.
        app_data.spawn.spawning_under = Some(selected_id);

        // Fetch branches for the selector.
        let Ok(repo) = git::open_repository(&selected_worktree_path) else {
            app_data.set_status("Review swarm requires a git repository");
            return Ok(AppMode::normal());
        };
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.review.start(branches);
        app_data.spawn.child_count = 3;

        Ok(ReviewChildCountMode.into())
    }
}

impl ValidIn<ScrollingMode> for ReviewSwarmAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        let Some(selected) = app_data.selected_agent() else {
            return Ok(ReviewInfoMode.into());
        };

        let selected_id = selected.id;
        let selected_worktree_path = selected.worktree_path.clone();
        let selected_is_terminal = selected.is_terminal_agent();

        if selected_is_terminal {
            app_data.set_status("Select a non-terminal agent for review swarm");
            return Ok(ScrollingMode.into());
        }

        // Store the selected agent's ID for later use.
        app_data.spawn.spawning_under = Some(selected_id);

        // Fetch branches for the selector.
        let Ok(repo) = git::open_repository(&selected_worktree_path) else {
            app_data.set_status("Review swarm requires a git repository");
            return Ok(ScrollingMode.into());
        };
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.review.start(branches);
        app_data.spawn.child_count = 3;

        Ok(ReviewChildCountMode.into())
    }
}

/// Normal-mode action: spawn a terminal under the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnTerminalAction;

impl ValidIn<NormalMode> for SpawnTerminalAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Actions::new().spawn_terminal(app_data, None)?;
        }
        Ok(AppMode::normal())
    }
}

impl ValidIn<ScrollingMode> for SpawnTerminalAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Actions::new().spawn_terminal(app_data, None)?;
        }
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: prompt for a terminal startup command.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnTerminalPromptedAction;

impl ValidIn<NormalMode> for SpawnTerminalPromptedAction {
    type NextState = AppMode;

    fn execute(self, _state: NormalMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(TerminalPromptMode.into())
        } else {
            Ok(AppMode::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for SpawnTerminalPromptedAction {
    type NextState = AppMode;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(TerminalPromptMode.into())
        } else {
            Ok(ScrollingMode.into())
        }
    }
}

#[cfg(test)]
mod tests;
