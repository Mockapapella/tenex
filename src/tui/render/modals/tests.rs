use super::*;
use crate::agent::{Agent, Storage};
use crate::app::{MuxdVersionMismatchInfo, Settings, WorktreeConflictInfo};
use crate::config::Config;
use crate::state::{
    BranchSelectorMode, BroadcastingMode, ChangelogMode, ChildCountMode, ChildPromptMode,
    CommandPaletteMode, ConfirmAction, ConfirmPushForPRMode, ConfirmPushMode, ConfirmingMode,
    CreatingMode, CustomAgentCommandMode, ErrorModalMode, HelpMode, KeyboardRemapPromptMode,
    MergeBranchSelectorMode, ModelSelectorMode, PreparingDockerMode, PromptingMode,
    RebaseBranchSelectorMode, ReconnectPromptMode, RenameBranchMode, ReviewChildCountMode,
    ReviewInfoMode, SettingsMenuMode, SuccessModalMode, SwitchBranchSelectorMode,
    TerminalPromptMode, UpdatePromptMode,
};
use crate::update::UpdateInfo;
use semver::Version;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn create_test_app() -> (App, NamedTempFile) {
    let temp_file = NamedTempFile::new().unwrap();
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    (
        App::new(Config::default(), storage, Settings::default(), false),
        temp_file,
    )
}

fn add_agent(app: &mut App, title: &str) -> uuid::Uuid {
    let pid = std::process::id();
    let agent = Agent::new(
        title.to_string(),
        "echo".to_string(),
        format!("tenex-modal-rect-test-{pid}/{title}"),
        PathBuf::from(format!("/tmp/tenex-modal-rect-test-{pid}/{title}")),
    );
    let id = agent.id;
    app.data.storage.add(agent);
    id
}

#[test]
fn modal_rect_for_mode_returns_none_in_normal() {
    let (app, _tmp) = create_test_app();
    let frame = Rect::new(0, 0, 80, 24);
    assert!(modal_rect_for_mode(&app, frame).is_none());
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Exhaustive coverage exercise for modal rectangle calculation"
)]
fn modal_rect_for_mode_covers_all_modal_variants() {
    let (mut app, _tmp) = create_test_app();
    let frame = Rect::new(0, 0, 120, 40);

    app.apply_mode(HelpMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(CommandPaletteMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    // Exercise the scrollbar path in `text_input_rect` (more than 20 lines).
    let long_input = (0..30)
        .map(|i| format!("line-{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text_input_modes = [
        CreatingMode.into(),
        PromptingMode.into(),
        ChildPromptMode.into(),
        BroadcastingMode.into(),
        ReconnectPromptMode.into(),
        TerminalPromptMode.into(),
        CustomAgentCommandMode.into(),
    ];
    for mode in text_input_modes {
        app.apply_mode(mode);
        app.data.input.buffer = long_input.clone();
        app.data.input.cursor = app.data.input.buffer.len();
        assert!(modal_rect_for_mode(&app, frame).is_some());
    }

    // Exercise the non-scrollbar path and middle-cursor rendering.
    app.apply_mode(CreatingMode.into());
    app.data.input.buffer = "hello".to_string();
    app.data.input.cursor = 2;
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(ChildCountMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(ReviewChildCountMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(ReviewInfoMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(BranchSelectorMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(RebaseBranchSelectorMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(MergeBranchSelectorMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(SwitchBranchSelectorMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(ModelSelectorMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(SettingsMenuMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    // Confirm push: cover both (agent missing) and (agent present) height paths.
    app.apply_mode(ConfirmPushMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());
    let id = add_agent(&mut app, "agent-0");
    app.data.git_op.agent_id = Some(id);
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(RenameBranchMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(ConfirmPushForPRMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    let info = UpdateInfo {
        current_version: Version::new(1, 0, 0),
        latest_version: Version::new(1, 0, 1),
    };
    app.apply_mode(UpdatePromptMode { info }.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ChangelogMode {
            title: "What's New".to_string(),
            lines: vec!["hello".to_string()],
            mark_seen_version: None,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(KeyboardRemapPromptMode.into());
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ErrorModalMode {
            message: "this is a long error message to wrap across multiple lines".to_string(),
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        PreparingDockerMode {
            message: "building the shipped docker image and reusing it for future roots"
                .to_string(),
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        SuccessModalMode {
            message: "this is a long success message to wrap across multiple lines".to_string(),
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    // Confirming: cover all actions and both selected/not-selected paths where relevant.
    app.data.storage.clear();
    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    let _ = add_agent(&mut app, "agent-1");
    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::Kill,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::InterruptAgent,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::Synthesize,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::Reset,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.data.ui.muxd_version_mismatch = Some(MuxdVersionMismatchInfo {
        socket: "tenex-mux-test.sock".to_string(),
        daemon_version: "tenex-mux/0.0.0".to_string(),
        expected_version: "tenex-mux/0.0.1".to_string(),
        env_mux_socket: None,
    });
    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::RestartMuxDaemon,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::Quit,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::SwitchBranch,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());

    app.data.spawn.worktree_conflict = Some(WorktreeConflictInfo {
        title: "wt".to_string(),
        prompt: None,
        branch: "wt-branch".to_string(),
        worktree_path: PathBuf::from("/tmp/wt"),
        repo_root: PathBuf::from("/tmp"),
        existing_branch: Some("main".to_string()),
        existing_commit: Some("abc1234".to_string()),
        current_branch: "main".to_string(),
        current_commit: "def5678".to_string(),
        swarm_child_count: None,
    });
    app.apply_mode(
        ConfirmingMode {
            action: ConfirmAction::WorktreeConflict,
        }
        .into(),
    );
    assert!(modal_rect_for_mode(&app, frame).is_some());
}
