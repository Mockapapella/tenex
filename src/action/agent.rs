use crate::action::ValidIn;
use crate::app::{AppData, ConfirmAction, Mode};
use crate::git;
use crate::state::{ConfirmingMode, ModeUnion, NormalMode, ScrollingMode};
use anyhow::{Context, Result};

/// Normal-mode action: enter agent creation mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct NewAgentAction;

impl ValidIn<NormalMode> for NewAgentAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, _app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        Ok(ModeUnion::Legacy(Mode::Creating))
    }
}

impl ValidIn<ScrollingMode> for NewAgentAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ScrollingMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::Legacy(Mode::Creating))
    }
}

/// Normal-mode action: enter agent creation mode with an initial prompt.
#[derive(Debug, Clone, Copy, Default)]
pub struct NewAgentWithPromptAction;

impl ValidIn<NormalMode> for NewAgentWithPromptAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, _app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        Ok(ModeUnion::Legacy(Mode::Prompting))
    }
}

impl ValidIn<ScrollingMode> for NewAgentWithPromptAction {
    type NextState = ModeUnion;

    fn execute(
        self,
        _state: ScrollingMode,
        _app_data: &mut AppData<'_>,
    ) -> Result<Self::NextState> {
        Ok(ModeUnion::Legacy(Mode::Prompting))
    }
}

/// Normal-mode action: kill the selected agent (enters confirmation).
#[derive(Debug, Clone, Copy, Default)]
pub struct KillAction;

impl ValidIn<NormalMode> for KillAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ConfirmingMode {
                action: ConfirmAction::Kill,
            }
            .into())
        } else {
            Ok(ModeUnion::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for KillAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
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
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.spawn.start_spawning_root();
        Ok(ModeUnion::Legacy(Mode::ChildCount))
    }
}

impl ValidIn<ScrollingMode> for SpawnChildrenAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        app_data.spawn.start_spawning_root();
        Ok(ModeUnion::Legacy(Mode::ChildCount))
    }
}

/// Normal-mode action: start the swarm planner (child-count picker).
#[derive(Debug, Clone, Copy, Default)]
pub struct PlanSwarmAction;

impl ValidIn<NormalMode> for PlanSwarmAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let Some(agent_id) = app_data.selected_agent().map(|a| a.id) else {
            app_data.set_status("Select an agent first (press 'a')");
            return Ok(ModeUnion::normal());
        };

        app_data.spawn.start_planning_swarm_under(agent_id);
        Ok(ModeUnion::Legacy(Mode::ChildCount))
    }
}

impl ValidIn<ScrollingMode> for PlanSwarmAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let Some(agent_id) = app_data.selected_agent().map(|a| a.id) else {
            app_data.set_status("Select an agent first (press 'a')");
            return Ok(ScrollingMode.into());
        };

        app_data.spawn.start_planning_swarm_under(agent_id);
        Ok(ModeUnion::Legacy(Mode::ChildCount))
    }
}

/// Normal-mode action: add children under the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct AddChildrenAction;

impl ValidIn<NormalMode> for AddChildrenAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if let Some(agent_id) = app_data.selected_agent().map(|a| a.id) {
            app_data.spawn.start_spawning_under(agent_id);
            Ok(ModeUnion::Legacy(Mode::ChildCount))
        } else {
            Ok(ModeUnion::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for AddChildrenAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if let Some(agent_id) = app_data.selected_agent().map(|a| a.id) {
            app_data.spawn.start_spawning_under(agent_id);
            Ok(ModeUnion::Legacy(Mode::ChildCount))
        } else {
            Ok(ScrollingMode.into())
        }
    }
}

/// Normal-mode action: synthesize children into the selected agent (enters confirmation).
#[derive(Debug, Clone, Copy, Default)]
pub struct SynthesizeAction;

impl ValidIn<NormalMode> for SynthesizeAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ModeUnion::normal());
        };

        if app_data.storage.has_children(agent.id) {
            Ok(ConfirmingMode {
                action: ConfirmAction::Synthesize,
            }
            .into())
        } else {
            Ok(ModeUnion::Legacy(Mode::ErrorModal(
                "Selected agent has no children to synthesize".to_string(),
            )))
        }
    }
}

impl ValidIn<ScrollingMode> for SynthesizeAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let Some(agent) = app_data.selected_agent() else {
            return Ok(ScrollingMode.into());
        };

        if app_data.storage.has_children(agent.id) {
            Ok(ConfirmingMode {
                action: ConfirmAction::Synthesize,
            }
            .into())
        } else {
            Ok(ModeUnion::Legacy(Mode::ErrorModal(
                "Selected agent has no children to synthesize".to_string(),
            )))
        }
    }
}

/// Normal-mode action: toggle collapse state of the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToggleCollapseAction;

impl ValidIn<NormalMode> for ToggleCollapseAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if let Some(agent) = app_data.selected_agent() {
            let agent_id = agent.id;
            if app_data.storage.has_children(agent_id)
                && let Some(agent) = app_data.storage.get_mut(agent_id)
            {
                agent.collapsed = !agent.collapsed;
                app_data.storage.save()?;
            }
        }
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ScrollingMode> for ToggleCollapseAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if let Some(agent) = app_data.selected_agent() {
            let agent_id = agent.id;
            if app_data.storage.has_children(agent_id)
                && let Some(agent) = app_data.storage.get_mut(agent_id)
            {
                agent.collapsed = !agent.collapsed;
                app_data.storage.save()?;
            }
        }
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: enter broadcasting mode for the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct BroadcastAction;

impl ValidIn<NormalMode> for BroadcastAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ModeUnion::Legacy(Mode::Broadcasting))
        } else {
            Ok(ModeUnion::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for BroadcastAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ModeUnion::Legacy(Mode::Broadcasting))
        } else {
            Ok(ScrollingMode.into())
        }
    }
}

/// Normal-mode action: start the review swarm flow.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReviewSwarmAction;

impl ValidIn<NormalMode> for ReviewSwarmAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let selected = app_data.selected_agent();
        if selected.is_none() {
            return Ok(ModeUnion::Legacy(Mode::ReviewInfo));
        }

        // Store the selected agent's ID for later use.
        let agent_id = selected.map(|a| a.id);
        app_data.spawn.spawning_under = agent_id;

        // Fetch branches for the selector.
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.review.start(branches);
        app_data.spawn.child_count = 3;

        Ok(ModeUnion::Legacy(Mode::ReviewChildCount))
    }
}

impl ValidIn<ScrollingMode> for ReviewSwarmAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        let selected = app_data.selected_agent();
        if selected.is_none() {
            return Ok(ModeUnion::Legacy(Mode::ReviewInfo));
        }

        // Store the selected agent's ID for later use.
        let agent_id = selected.map(|a| a.id);
        app_data.spawn.spawning_under = agent_id;

        // Fetch branches for the selector.
        let repo_path = std::env::current_dir().context("Failed to get current directory")?;
        let repo = git::open_repository(&repo_path)?;
        let branch_mgr = git::BranchManager::new(&repo);
        let branches = branch_mgr.list_for_selector()?;

        app_data.review.start(branches);
        app_data.spawn.child_count = 3;

        Ok(ModeUnion::Legacy(Mode::ReviewChildCount))
    }
}

/// Normal-mode action: spawn a terminal under the selected agent.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnTerminalAction;

impl ValidIn<NormalMode> for SpawnTerminalAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            app_data.actions.spawn_terminal(app_data.app, None)?;
        }
        Ok(ModeUnion::normal())
    }
}

impl ValidIn<ScrollingMode> for SpawnTerminalAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            app_data.actions.spawn_terminal(app_data.app, None)?;
        }
        Ok(ScrollingMode.into())
    }
}

/// Normal-mode action: prompt for a terminal startup command.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnTerminalPromptedAction;

impl ValidIn<NormalMode> for SpawnTerminalPromptedAction {
    type NextState = ModeUnion;

    fn execute(self, _state: NormalMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ModeUnion::Legacy(Mode::TerminalPrompt))
        } else {
            Ok(ModeUnion::normal())
        }
    }
}

impl ValidIn<ScrollingMode> for SpawnTerminalPromptedAction {
    type NextState = ModeUnion;

    fn execute(self, _state: ScrollingMode, app_data: &mut AppData<'_>) -> Result<Self::NextState> {
        if app_data.selected_agent().is_some() {
            Ok(ModeUnion::Legacy(Mode::TerminalPrompt))
        } else {
            Ok(ScrollingMode.into())
        }
    }
}
