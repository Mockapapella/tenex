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
mod tests {
    use super::*;
    use crate::agent::{Agent, ChildConfig, Storage};
    use crate::app::Settings;
    use crate::config::Config;
    use crate::runtime;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;

    fn create_test_data() -> (AppData, NamedTempFile) {
        let temp_file = NamedTempFile::new().expect("create temp file");
        let storage = Storage::with_path(temp_file.path().to_path_buf());
        (
            AppData::new(Config::default(), storage, Settings::default(), false),
            temp_file,
        )
    }

    fn init_test_repo_with_commit() -> TempDir {
        let temp_dir = TempDir::new().expect("create temp dir");

        let _ = crate::git::git_command()
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .expect("init repo");

        let _ = crate::git::git_command()
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()
            .expect("configure user email");

        let _ = crate::git::git_command()
            .args(["config", "user.name", "Tenex Test"])
            .current_dir(temp_dir.path())
            .output()
            .expect("configure user name");

        let _ = crate::git::git_command()
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(temp_dir.path())
            .output()
            .expect("create initial commit");

        temp_dir
    }

    fn add_root_agent(data: &mut AppData, worktree_path: PathBuf) -> uuid::Uuid {
        let agent = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            worktree_path,
        );
        let id = agent.id;
        data.storage.add(agent);
        data.selected = 1;
        id
    }

    fn add_child_agent(
        data: &mut AppData,
        parent_id: uuid::Uuid,
        mux_session: String,
    ) -> uuid::Uuid {
        let child = Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id,
                mux_session,
                window_index: 2,
                repo_root: None,
            },
        );
        let id = child.id;
        data.storage.add(child);
        id
    }

    #[test]
    fn test_actions_without_selected_agent() {
        let (mut data, _temp) = create_test_data();

        assert_eq!(
            NewAgentAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::Creating(CreatingMode)
        );
        assert_eq!(
            NewAgentWithPromptAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Prompting(PromptingMode)
        );
        assert_eq!(
            KillAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            KillAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );

        assert_eq!(
            SpawnChildrenAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::ChildCount(ChildCountMode)
        );

        assert_eq!(
            PlanSwarmAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Select an agent first (press 'a')")
        );

        assert_eq!(
            AddChildrenAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert_eq!(
            BroadcastAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            SpawnTerminalPromptedAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert_eq!(
            SpawnTerminalAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
    }

    #[test]
    fn test_actions_with_selected_agent() {
        let (mut data, _temp) = create_test_data();

        add_root_agent(&mut data, PathBuf::from("/tmp"));

        assert_eq!(
            KillAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Kill
            })
        );

        assert_eq!(
            AddChildrenAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::ChildCount(ChildCountMode)
        );
        assert_eq!(
            BroadcastAction.execute(NormalMode, &mut data).unwrap(),
            BroadcastingMode.into()
        );
        assert_eq!(
            SpawnTerminalPromptedAction
                .execute(NormalMode, &mut data)
                .unwrap(),
            TerminalPromptMode.into()
        );
        assert_eq!(
            SynthesizeAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::ErrorModal(ErrorModalMode {
                message: "Selected agent has no children to synthesize".to_string(),
            })
        );
    }

    #[test]
    fn test_plan_swarm_action_covers_selected_and_terminal_branches() {
        let (mut data, _temp) = create_test_data();

        assert_eq!(
            PlanSwarmAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Select an agent first (press 'a')")
        );

        let agent_id = add_root_agent(&mut data, PathBuf::from("/tmp"));
        assert_eq!(
            PlanSwarmAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::ChildCount(ChildCountMode)
        );
        assert_eq!(data.spawn.spawning_under, Some(agent_id));
        assert!(data.spawn.use_plan_prompt);

        data.storage
            .get_mut(agent_id)
            .expect("missing selected agent")
            .is_terminal = true;

        assert_eq!(
            PlanSwarmAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Select a non-terminal agent first (press 'a')")
        );
        assert_eq!(
            PlanSwarmAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Select a non-terminal agent first (press 'a')")
        );
    }

    #[test]
    fn test_add_children_action_terminal_in_scrolling() {
        let (mut data, _temp) = create_test_data();

        let agent_id = add_root_agent(&mut data, PathBuf::from("/tmp"));
        data.storage
            .get_mut(agent_id)
            .expect("missing selected agent")
            .is_terminal = true;

        assert_eq!(
            AddChildrenAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert_eq!(
            data.ui.status_message.as_deref(),
            Some("Cannot spawn children under a terminal")
        );
    }

    #[test]
    fn test_synthesize_action_covers_terminal_and_children_branches() {
        let (mut data, _temp) = create_test_data();
        assert_eq!(
            SynthesizeAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            SynthesizeAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );

        let (mut terminal_data, _temp) = create_test_data();
        let terminal_id = add_root_agent(&mut terminal_data, PathBuf::from("/tmp"));
        terminal_data
            .storage
            .get_mut(terminal_id)
            .expect("missing root agent")
            .is_terminal = true;
        assert_eq!(
            SynthesizeAction
                .execute(NormalMode, &mut terminal_data)
                .unwrap(),
            AppMode::ErrorModal(ErrorModalMode {
                message: "Cannot synthesize into a terminal agent".to_string(),
            })
        );
        assert_eq!(
            SynthesizeAction
                .execute(ScrollingMode, &mut terminal_data)
                .unwrap(),
            AppMode::ErrorModal(ErrorModalMode {
                message: "Cannot synthesize into a terminal agent".to_string(),
            })
        );

        let (mut no_child_data, _temp) = create_test_data();
        add_root_agent(&mut no_child_data, PathBuf::from("/tmp"));
        assert_eq!(
            SynthesizeAction
                .execute(ScrollingMode, &mut no_child_data)
                .unwrap(),
            AppMode::ErrorModal(ErrorModalMode {
                message: "Selected agent has no children to synthesize".to_string(),
            })
        );

        let (mut child_data, _temp) = create_test_data();
        let root_id = add_root_agent(&mut child_data, PathBuf::from("/tmp"));
        let root_session = child_data
            .storage
            .get(root_id)
            .expect("missing root agent")
            .mux_session
            .clone();
        add_child_agent(&mut child_data, root_id, root_session);
        assert_eq!(
            SynthesizeAction
                .execute(NormalMode, &mut child_data)
                .unwrap(),
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Synthesize
            })
        );
        assert_eq!(
            SynthesizeAction
                .execute(ScrollingMode, &mut child_data)
                .unwrap(),
            AppMode::Confirming(ConfirmingMode {
                action: ConfirmAction::Synthesize
            })
        );

        let (mut terminal_child_data, _temp) = create_test_data();
        let root_id = add_root_agent(&mut terminal_child_data, PathBuf::from("/tmp"));
        let root_session = terminal_child_data
            .storage
            .get(root_id)
            .expect("missing root agent")
            .mux_session
            .clone();
        let child_id = add_child_agent(&mut terminal_child_data, root_id, root_session);
        terminal_child_data
            .storage
            .get_mut(child_id)
            .expect("missing child agent")
            .is_terminal = true;
        assert_eq!(
            SynthesizeAction
                .execute(NormalMode, &mut terminal_child_data)
                .unwrap(),
            AppMode::ErrorModal(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            })
        );
        assert_eq!(
            SynthesizeAction
                .execute(ScrollingMode, &mut terminal_child_data)
                .unwrap(),
            AppMode::ErrorModal(ErrorModalMode {
                message: "Selected agent has no non-terminal children to synthesize".to_string(),
            })
        );
    }

    #[test]
    fn test_spawn_terminal_action_spawns_terminal_in_normal_mode() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("action-agent-term-n");
        crate::mux::set_socket_override(&socket).unwrap();

        let (mut data, _temp) = create_test_data();
        let worktree_dir = TempDir::new().unwrap();
        let root_id = add_root_agent(&mut data, worktree_dir.path().to_path_buf());
        let session = data
            .storage
            .get(root_id)
            .expect("missing root agent")
            .mux_session
            .clone();

        let manager = crate::mux::SessionManager::new();
        manager.create(&session, worktree_dir.path(), None).unwrap();

        let next = SpawnTerminalAction.execute(NormalMode, &mut data).unwrap();
        assert_eq!(next, AppMode::normal());
        let children = data.storage.children(root_id);
        assert_eq!(children.len(), 1);
        assert!(children.into_iter().all(crate::Agent::is_terminal_agent));

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_spawn_terminal_action_spawns_terminal_in_scrolling_mode() {
        let _guard = crate::test_support::lock_mux_test_environment();
        let socket = crate::test_support::unique_mux_socket_path("action-agent-term-s");
        crate::mux::set_socket_override(&socket).unwrap();

        let (mut data, _temp) = create_test_data();
        let worktree_dir = TempDir::new().unwrap();
        let root_id = add_root_agent(&mut data, worktree_dir.path().to_path_buf());
        let session = data
            .storage
            .get(root_id)
            .expect("missing root agent")
            .mux_session
            .clone();

        let manager = crate::mux::SessionManager::new();
        manager.create(&session, worktree_dir.path(), None).unwrap();

        let next = SpawnTerminalAction
            .execute(ScrollingMode, &mut data)
            .unwrap();
        assert_eq!(next, AppMode::Scrolling(ScrollingMode));
        assert_eq!(data.storage.children(root_id).len(), 1);

        let _ = manager.kill(&session);
    }

    #[test]
    fn test_spawn_terminal_action_scrolling_propagates_runtime_errors() {
        fn run_spawn_with_override(
            docker_path: PathBuf,
            forced_error: bool,
        ) -> Result<(), Box<dyn std::error::Error>> {
            runtime::with_docker_program_override_for_tests(docker_path, || {
                if forced_error {
                    return Err("forced error".into());
                }

                let (mut data, _temp) = create_test_data();
                let worktree_dir = TempDir::new().unwrap();
                let root_id = add_root_agent(&mut data, worktree_dir.path().to_path_buf());
                data.storage
                    .get_mut(root_id)
                    .expect("missing root agent")
                    .runtime = crate::agent::AgentRuntime::Docker;

                let err = SpawnTerminalAction
                    .execute(ScrollingMode, &mut data)
                    .expect_err("docker runtime should fail when docker exits non-zero");
                assert!(!err.to_string().is_empty());
                Ok(())
            })
        }

        let docker_dir = TempDir::new().unwrap();
        let docker_path = docker_dir.path().join("docker");
        std::fs::write(&docker_path, "#!/usr/bin/env sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&docker_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&docker_path, perms).unwrap();
        }

        run_spawn_with_override(docker_path.clone(), false).unwrap();
        let err = run_spawn_with_override(docker_path, true).expect_err("expected forced error");
        assert!(err.to_string().contains("forced error"));
    }

    #[test]
    fn test_toggle_collapse_action_covers_project_none_and_diff_modes() {
        let (mut data, _temp) = create_test_data();

        assert_eq!(
            ToggleCollapseAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );

        let temp_dir = TempDir::new().unwrap();
        data.cwd_project_root = Some(temp_dir.path().to_path_buf());
        data.selected = 0;

        assert!(data.ui.collapsed_projects.is_empty());
        assert_eq!(
            ToggleCollapseAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert!(data.ui.collapsed_projects.contains(temp_dir.path()));

        assert_eq!(
            ToggleCollapseAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert!(!data.ui.collapsed_projects.contains(temp_dir.path()));

        data.active_tab = Tab::Preview;
        assert_eq!(
            ToggleCollapseAction
                .execute(DiffFocusedMode, &mut data)
                .unwrap(),
            DiffFocusedMode.into()
        );

        data.active_tab = Tab::Diff;
        data.ui.set_diff_content("");
        assert_eq!(
            ToggleCollapseAction
                .execute(DiffFocusedMode, &mut data)
                .unwrap(),
            DiffFocusedMode.into()
        );
    }

    #[test]
    fn test_toggle_collapse_action_scrolling_skips_agent_without_children() {
        let (mut data, _temp) = create_test_data();
        let agent_id = add_root_agent(&mut data, PathBuf::from("/tmp"));
        data.storage
            .get_mut(agent_id)
            .expect("missing root agent")
            .collapsed = true;

        assert!(!data.storage.has_children(agent_id));
        assert_eq!(
            ToggleCollapseAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert!(
            data.storage
                .get(agent_id)
                .expect("missing root agent")
                .collapsed
        );
    }

    #[test]
    fn test_broadcast_action_covers_scrolling_branches() {
        let (mut data, _temp) = create_test_data();
        assert_eq!(
            BroadcastAction.execute(ScrollingMode, &mut data).unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );

        add_root_agent(&mut data, PathBuf::from("/tmp"));
        assert_eq!(
            BroadcastAction.execute(ScrollingMode, &mut data).unwrap(),
            BroadcastingMode.into()
        );
    }

    #[test]
    fn test_review_swarm_action_covers_branches() {
        let (mut data, _temp) = create_test_data();
        assert_eq!(
            ReviewSwarmAction.execute(NormalMode, &mut data).unwrap(),
            ReviewInfoMode.into()
        );
        assert_eq!(
            ReviewSwarmAction.execute(ScrollingMode, &mut data).unwrap(),
            ReviewInfoMode.into()
        );

        let (mut terminal_data, _temp) = create_test_data();
        let terminal_id = add_root_agent(&mut terminal_data, PathBuf::from("/tmp"));
        terminal_data
            .storage
            .get_mut(terminal_id)
            .expect("missing root agent")
            .is_terminal = true;
        assert_eq!(
            ReviewSwarmAction
                .execute(NormalMode, &mut terminal_data)
                .unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            terminal_data.ui.status_message.as_deref(),
            Some("Select a non-terminal agent for review swarm")
        );
        assert_eq!(
            ReviewSwarmAction
                .execute(ScrollingMode, &mut terminal_data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );

        let (mut non_repo_data, _temp) = create_test_data();
        let non_repo_dir = TempDir::new().unwrap();
        add_root_agent(&mut non_repo_data, non_repo_dir.path().to_path_buf());
        assert_eq!(
            ReviewSwarmAction
                .execute(NormalMode, &mut non_repo_data)
                .unwrap(),
            AppMode::normal()
        );
        assert_eq!(
            non_repo_data.ui.status_message.as_deref(),
            Some("Review swarm requires a git repository")
        );
        assert_eq!(
            ReviewSwarmAction
                .execute(ScrollingMode, &mut non_repo_data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );

        let (mut repo_data, _temp) = create_test_data();
        let repo = init_test_repo_with_commit();
        let root_id = add_root_agent(&mut repo_data, repo.path().to_path_buf());
        assert_eq!(
            ReviewSwarmAction
                .execute(NormalMode, &mut repo_data)
                .unwrap(),
            ReviewChildCountMode.into()
        );
        assert_eq!(repo_data.spawn.spawning_under, Some(root_id));
        assert_eq!(repo_data.spawn.child_count, 3);
        assert!(!repo_data.review.branches.is_empty());

        assert_eq!(
            ReviewSwarmAction
                .execute(ScrollingMode, &mut repo_data)
                .unwrap(),
            ReviewChildCountMode.into()
        );
    }

    #[test]
    fn test_review_swarm_action_propagates_branch_listing_errors() {
        let repo = init_test_repo_with_commit();
        let packed_refs = repo.path().join(".git/packed-refs");
        std::fs::write(&packed_refs, "this-is-not-a-packed-refs-file\n").unwrap();

        let (mut normal_data, _temp) = create_test_data();
        add_root_agent(&mut normal_data, repo.path().to_path_buf());
        let err = ReviewSwarmAction
            .execute(NormalMode, &mut normal_data)
            .unwrap_err();
        assert!(err.to_string().contains("Failed to list local branches"));

        let (mut scrolling_data, _temp) = create_test_data();
        add_root_agent(&mut scrolling_data, repo.path().to_path_buf());
        let err = ReviewSwarmAction
            .execute(ScrollingMode, &mut scrolling_data)
            .unwrap_err();
        assert!(err.to_string().contains("Failed to list local branches"));
    }

    #[test]
    fn test_toggle_collapse_action_toggles_when_agent_has_children() {
        let (mut data, _temp) = create_test_data();

        data.storage.add(Agent::new(
            "other".to_string(),
            "claude".to_string(),
            "feature/other".to_string(),
            PathBuf::from("/tmp"),
        ));

        let mut root = Agent::new(
            "root".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            PathBuf::from("/tmp"),
        );
        let root_id = root.id;
        root.collapsed = true;
        let root_session = root.mux_session.clone();
        data.storage.add(root);

        data.storage.add(Agent::new_child(
            "child".to_string(),
            "claude".to_string(),
            "feature/root".to_string(),
            PathBuf::from("/tmp"),
            ChildConfig {
                parent_id: root_id,
                mux_session: root_session,
                window_index: 2,
                repo_root: None,
            },
        ));

        data.selected = data
            .sidebar_items()
            .iter()
            .position(
                |item| matches!(item, SidebarItem::Agent(agent) if agent.info.agent.id == root_id),
            )
            .expect("missing root agent");

        assert!(data.storage.has_children(root_id));
        assert!(
            data.storage
                .get(root_id)
                .expect("missing root agent")
                .collapsed
        );

        assert_eq!(
            ToggleCollapseAction.execute(NormalMode, &mut data).unwrap(),
            AppMode::normal()
        );
        assert!(
            !data
                .storage
                .get(root_id)
                .expect("missing root agent")
                .collapsed
        );

        assert_eq!(
            ToggleCollapseAction
                .execute(ScrollingMode, &mut data)
                .unwrap(),
            AppMode::Scrolling(ScrollingMode)
        );
        assert!(
            data.storage
                .get(root_id)
                .expect("missing root agent")
                .collapsed
        );
    }
}
