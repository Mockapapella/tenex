use anyhow::{Result, anyhow};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use std::path::{Path, PathBuf};
use tenex::config::test_support as config_support;
use tenex::config::{
    Action, ActionGroup, Config, get_action, get_display_description, get_display_keys,
    status_hints,
};

const ACTION_METADATA: &[(Action, &str, &str, ActionGroup)] = &[
    (Action::NewAgent, "a", "[a]dd agent", ActionGroup::Agents),
    (
        Action::NewAgentWithPrompt,
        "A",
        "[A]dd agent with prompt",
        ActionGroup::Agents,
    ),
    (
        Action::FocusPreview,
        "Enter",
        "[Enter] focus preview (Preview tab) / diff (Diff tab)",
        ActionGroup::Navigation,
    ),
    (
        Action::UnfocusPreview,
        "Ctrl+q",
        "[Ctrl+q] detach terminal / quit app",
        ActionGroup::Navigation,
    ),
    (
        Action::Kill,
        "d",
        "[d]elete agent and sub-agents",
        ActionGroup::Agents,
    ),
    (
        Action::Push,
        "Ctrl+p",
        "[Ctrl+p]ush branch to remote",
        ActionGroup::GitOps,
    ),
    (
        Action::RenameBranch,
        "r",
        "[r]ename branch",
        ActionGroup::GitOps,
    ),
    (
        Action::OpenPR,
        "Ctrl+o",
        "[Ctrl+o]pen pull request",
        ActionGroup::GitOps,
    ),
    (
        Action::SwitchTab,
        "Tab",
        "[Tab] next tab when detached",
        ActionGroup::Navigation,
    ),
    (
        Action::DiffCursorUp,
        "↑",
        "[↑] diff cursor up",
        ActionGroup::Hidden,
    ),
    (
        Action::DiffCursorDown,
        "↓",
        "[↓] diff cursor down",
        ActionGroup::Hidden,
    ),
    (
        Action::DiffToggleVisual,
        "shift+v",
        "[shift+v] block select/unselect",
        ActionGroup::Hidden,
    ),
    (
        Action::DiffDeleteLine,
        "x",
        "[x] delete diff line/hunk",
        ActionGroup::Hidden,
    ),
    (
        Action::DiffUndo,
        "Ctrl+z",
        "[Ctrl+z] undo diff edit",
        ActionGroup::Hidden,
    ),
    (
        Action::DiffRedo,
        "Ctrl+y",
        "[Ctrl+y] redo diff edit",
        ActionGroup::Hidden,
    ),
    (
        Action::NextAgent,
        "↓",
        "[↓] next item",
        ActionGroup::Navigation,
    ),
    (
        Action::PrevAgent,
        "↑",
        "[↑] prev item",
        ActionGroup::Navigation,
    ),
    (
        Action::SelectProjectHeader,
        "←",
        "[←] highlight project",
        ActionGroup::Navigation,
    ),
    (
        Action::SelectProjectFirstAgent,
        "→",
        "[→] highlight first agent",
        ActionGroup::Navigation,
    ),
    (Action::Help, "?", "[?] help", ActionGroup::Other),
    (Action::Quit, "Ctrl+q", "[Ctrl+q]uit", ActionGroup::Other),
    (
        Action::ScrollUp,
        "Ctrl+u",
        "[Ctrl+u] scroll preview/diff/commits up",
        ActionGroup::Navigation,
    ),
    (
        Action::ScrollDown,
        "Ctrl+d",
        "[Ctrl+d] scroll preview/diff/commits down",
        ActionGroup::Navigation,
    ),
    (
        Action::ScrollTop,
        "g",
        "[g]o to top",
        ActionGroup::Navigation,
    ),
    (
        Action::ScrollBottom,
        "G",
        "[G]o to bottom",
        ActionGroup::Navigation,
    ),
    (Action::Cancel, "Esc", "Cancel", ActionGroup::Hidden),
    (Action::Confirm, "y", "Confirm", ActionGroup::Hidden),
    (
        Action::SpawnChildren,
        "S",
        "[S]pawn swarm",
        ActionGroup::Agents,
    ),
    (
        Action::PlanSwarm,
        "P",
        "[P] spawn planners for selected agent",
        ActionGroup::Agents,
    ),
    (
        Action::AddChildren,
        "+",
        "[+] spawn sub-agents for selected agent",
        ActionGroup::Agents,
    ),
    (
        Action::Synthesize,
        "s",
        "[s]ynthesize sub-agent outputs",
        ActionGroup::Agents,
    ),
    (
        Action::ToggleSynthesisMark,
        "m",
        "[m]ark subtree for synthesis",
        ActionGroup::Agents,
    ),
    (
        Action::ToggleCollapse,
        "Space",
        "[Space] collapse/expand",
        ActionGroup::Navigation,
    ),
    (
        Action::Broadcast,
        "B",
        "[B]roadcast to leaf sub-agents",
        ActionGroup::Agents,
    ),
    (
        Action::ReviewSwarm,
        "R",
        "[R] spawn reviewers for selected agent",
        ActionGroup::Agents,
    ),
    (
        Action::SpawnTerminal,
        "t",
        "[t]erminal",
        ActionGroup::Terminals,
    ),
    (
        Action::SpawnTerminalPrompted,
        "T",
        "[T]erminal with command",
        ActionGroup::Terminals,
    ),
    (
        Action::Rebase,
        "Ctrl+r",
        "[Ctrl+r]ebase onto branch",
        ActionGroup::GitOps,
    ),
    (
        Action::Merge,
        "Ctrl+m",
        "[Ctrl+m]erge branch",
        ActionGroup::GitOps,
    ),
    (
        Action::SwitchBranch,
        "Ctrl+s",
        "[Ctrl+s]witch branch",
        ActionGroup::GitOps,
    ),
    (
        Action::CommandPalette,
        "/",
        "[/] commands",
        ActionGroup::Other,
    ),
];

#[test]
fn test_keybindings() {
    assert_eq!(
        get_action(KeyCode::Char('a'), KeyModifiers::NONE),
        Some(Action::NewAgent)
    );
    // Plain 'q' no longer quits - only Ctrl+q does
    assert_eq!(get_action(KeyCode::Char('q'), KeyModifiers::NONE), None);
    assert_eq!(
        get_action(KeyCode::Enter, KeyModifiers::NONE),
        Some(Action::FocusPreview)
    );
    // Ctrl+q maps to Quit (but exits preview focus when in PreviewFocused mode)
    assert_eq!(
        get_action(KeyCode::Char('q'), KeyModifiers::CONTROL),
        Some(Action::Quit)
    );
    assert_eq!(
        get_action(KeyCode::Char('s'), KeyModifiers::CONTROL),
        Some(Action::SwitchBranch)
    );
    assert_eq!(
        get_action(KeyCode::Char('m'), KeyModifiers::NONE),
        Some(Action::ToggleSynthesisMark)
    );
}

#[test]
fn test_modifier_keys() {
    assert_eq!(
        get_action(KeyCode::Char('u'), KeyModifiers::CONTROL),
        Some(Action::ScrollUp)
    );
    // Some terminals report Ctrl+<char> as uppercase or with redundant SHIFT.
    assert_eq!(
        get_action(KeyCode::Char('U'), KeyModifiers::CONTROL),
        Some(Action::ScrollUp)
    );
    assert_eq!(
        get_action(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        ),
        Some(Action::ScrollUp)
    );
    assert_eq!(
        get_action(KeyCode::Char('d'), KeyModifiers::CONTROL),
        Some(Action::ScrollDown)
    );
    assert_eq!(
        get_action(KeyCode::Char('D'), KeyModifiers::CONTROL),
        Some(Action::ScrollDown)
    );
    assert_eq!(
        get_action(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        ),
        Some(Action::ScrollDown)
    );
    assert_eq!(
        get_action(KeyCode::Char('p'), KeyModifiers::CONTROL),
        Some(Action::Push)
    );
    assert_eq!(
        get_action(KeyCode::Char('r'), KeyModifiers::NONE),
        Some(Action::RenameBranch)
    );
    assert_eq!(
        get_action(KeyCode::Char('o'), KeyModifiers::CONTROL),
        Some(Action::OpenPR)
    );
    assert_eq!(
        get_action(KeyCode::Char('r'), KeyModifiers::CONTROL),
        Some(Action::Rebase)
    );
    assert_eq!(
        get_action(KeyCode::Char('m'), KeyModifiers::CONTROL),
        Some(Action::Merge)
    );
    assert_eq!(
        get_action(KeyCode::Char('M'), KeyModifiers::CONTROL),
        Some(Action::Merge)
    );
}

#[test]
fn test_uppercase_keybindings() {
    // Uppercase 'S' should trigger SpawnChildren
    assert_eq!(
        get_action(KeyCode::Char('S'), KeyModifiers::SHIFT),
        Some(Action::SpawnChildren)
    );
    // Also works without SHIFT modifier (some terminals don't send it)
    assert_eq!(
        get_action(KeyCode::Char('S'), KeyModifiers::NONE),
        Some(Action::SpawnChildren)
    );

    // Lowercase 's' should trigger Synthesize
    assert_eq!(
        get_action(KeyCode::Char('s'), KeyModifiers::NONE),
        Some(Action::Synthesize)
    );

    // Uppercase 'A' should trigger NewAgentWithPrompt
    assert_eq!(
        get_action(KeyCode::Char('A'), KeyModifiers::SHIFT),
        Some(Action::NewAgentWithPrompt)
    );

    // Uppercase 'G' should trigger ScrollBottom
    assert_eq!(
        get_action(KeyCode::Char('G'), KeyModifiers::SHIFT),
        Some(Action::ScrollBottom)
    );

    // Some terminals (notably with Kitty keyboard protocol) report lowercase + SHIFT.
    assert_eq!(
        get_action(KeyCode::Char('g'), KeyModifiers::SHIFT),
        Some(Action::ScrollBottom)
    );
}

#[test]
fn test_unknown_key() {
    assert_eq!(get_action(KeyCode::Char('v'), KeyModifiers::NONE), None);
    assert_eq!(
        get_action(KeyCode::Char('V'), KeyModifiers::NONE),
        Some(Action::DiffToggleVisual)
    );
}

#[test]
fn test_action_keys() {
    assert_eq!(Action::NewAgent.keys(), "a");
    assert_eq!(Action::SpawnChildren.keys(), "S");
    assert_eq!(Action::ToggleSynthesisMark.keys(), "m");
    assert_eq!(Action::NextAgent.keys(), "↓");
    assert_eq!(Action::DiffToggleVisual.keys(), "shift+v");
}

#[test]
fn test_status_hints() {
    let hints = status_hints();
    assert_eq!(hints, "[?]help  [/]commands");
}

#[test]
fn test_action_description() {
    assert_eq!(Action::NewAgent.description(), "[a]dd agent");
    assert_eq!(Action::SpawnChildren.description(), "[S]pawn swarm");
    assert_eq!(Action::NextAgent.description(), "[↓] next item");
    assert_eq!(Action::PrevAgent.description(), "[↑] prev item");
    assert_eq!(
        Action::PlanSwarm.description(),
        "[P] spawn planners for selected agent"
    );
    assert_eq!(
        Action::ToggleSynthesisMark.description(),
        "[m]ark subtree for synthesis"
    );
    assert_eq!(
        Action::DiffToggleVisual.description(),
        "[shift+v] block select/unselect"
    );
}

#[test]
fn test_action_group_titles_cover_hidden_group() {
    assert_eq!(ActionGroup::Agents.title(), "Agents");
    assert_eq!(ActionGroup::Terminals.title(), "Terminals");
    assert_eq!(ActionGroup::GitOps.title(), "Git Ops");
    assert_eq!(ActionGroup::Navigation.title(), "Navigation");
    assert_eq!(ActionGroup::Other.title(), "Other");
    assert_eq!(ActionGroup::Hidden.title(), "");
}

#[test]
fn test_action_metadata_covers_every_action() {
    for &(action, keys, description, group) in ACTION_METADATA {
        assert_eq!(action.keys(), keys, "{action:?}");
        assert_eq!(action.description(), description, "{action:?}");
        assert_eq!(action.group(), group, "{action:?}");
        assert_eq!(get_display_keys(action, false), keys, "{action:?}");
        assert_eq!(
            get_display_description(action, false),
            description,
            "{action:?}"
        );
    }

    assert!(Action::ALL_FOR_HELP.contains(&Action::ToggleSynthesisMark));
}

#[test]
fn test_all_keybinding_entries_resolve_to_actions() {
    let none = KeyModifiers::NONE;
    let shift = KeyModifiers::SHIFT;
    let control = KeyModifiers::CONTROL;
    let cases = [
        (KeyCode::Char('a'), none, Action::NewAgent),
        (KeyCode::Char('A'), none, Action::NewAgentWithPrompt),
        (KeyCode::Char('A'), shift, Action::NewAgentWithPrompt),
        (KeyCode::Enter, none, Action::FocusPreview),
        (KeyCode::Char('q'), control, Action::Quit),
        (KeyCode::Char('d'), none, Action::Kill),
        (KeyCode::Char('S'), none, Action::SpawnChildren),
        (KeyCode::Char('S'), shift, Action::SpawnChildren),
        (KeyCode::Char('P'), none, Action::PlanSwarm),
        (KeyCode::Char('P'), shift, Action::PlanSwarm),
        (KeyCode::Char('+'), none, Action::AddChildren),
        (KeyCode::Char('+'), shift, Action::AddChildren),
        (KeyCode::Char('s'), none, Action::Synthesize),
        (KeyCode::Char('m'), none, Action::ToggleSynthesisMark),
        (KeyCode::Char(' '), none, Action::ToggleCollapse),
        (KeyCode::Char('B'), none, Action::Broadcast),
        (KeyCode::Char('B'), shift, Action::Broadcast),
        (KeyCode::Char('R'), none, Action::ReviewSwarm),
        (KeyCode::Char('R'), shift, Action::ReviewSwarm),
        (KeyCode::Char('t'), none, Action::SpawnTerminal),
        (KeyCode::Char('T'), none, Action::SpawnTerminalPrompted),
        (KeyCode::Char('T'), shift, Action::SpawnTerminalPrompted),
        (KeyCode::Down, none, Action::NextAgent),
        (KeyCode::Up, none, Action::PrevAgent),
        (KeyCode::Left, none, Action::SelectProjectHeader),
        (KeyCode::Right, none, Action::SelectProjectFirstAgent),
        (KeyCode::Tab, none, Action::SwitchTab),
        (KeyCode::Char('V'), none, Action::DiffToggleVisual),
        (KeyCode::Char('x'), none, Action::DiffDeleteLine),
        (KeyCode::Char('z'), control, Action::DiffUndo),
        (KeyCode::Char('y'), control, Action::DiffRedo),
        (KeyCode::Char('u'), control, Action::ScrollUp),
        (KeyCode::Char('d'), control, Action::ScrollDown),
        (KeyCode::Char('g'), none, Action::ScrollTop),
        (KeyCode::Char('G'), none, Action::ScrollBottom),
        (KeyCode::Char('G'), shift, Action::ScrollBottom),
        (KeyCode::Char('?'), none, Action::Help),
        (KeyCode::Char('?'), shift, Action::Help),
        (KeyCode::Char('/'), none, Action::CommandPalette),
        (KeyCode::Char('/'), shift, Action::CommandPalette),
        (KeyCode::Char('p'), control, Action::Push),
        (KeyCode::Char('r'), none, Action::RenameBranch),
        (KeyCode::Char('o'), control, Action::OpenPR),
        (KeyCode::Char('r'), control, Action::Rebase),
        (KeyCode::Char('m'), control, Action::Merge),
        (KeyCode::Char('n'), control, Action::Merge),
        (KeyCode::Char('s'), control, Action::SwitchBranch),
        (KeyCode::Esc, none, Action::Cancel),
        (KeyCode::Char('y'), none, Action::Confirm),
    ];

    for (code, modifiers, action) in cases {
        assert_eq!(get_action(code, modifiers), Some(action), "{code:?}");
    }

    assert_eq!(
        get_action(KeyCode::Char('a'), KeyModifiers::SHIFT),
        Some(Action::NewAgentWithPrompt)
    );
    assert_eq!(
        get_action(KeyCode::Char('v'), KeyModifiers::SHIFT),
        Some(Action::DiffToggleVisual)
    );
    assert_eq!(get_action(KeyCode::F(1), KeyModifiers::SHIFT), None);
}

#[test]
fn test_action_description_covers_hidden_actions() {
    assert_eq!(Action::DiffCursorUp.description(), "[↑] diff cursor up");
    assert_eq!(Action::DiffCursorDown.description(), "[↓] diff cursor down");
    assert_eq!(
        Action::DiffDeleteLine.description(),
        "[x] delete diff line/hunk"
    );
    assert_eq!(Action::DiffUndo.description(), "[Ctrl+z] undo diff edit");
    assert_eq!(Action::DiffRedo.description(), "[Ctrl+y] redo diff edit");
    assert_eq!(Action::Cancel.description(), "Cancel");
    assert_eq!(Action::Confirm.description(), "Confirm");
    assert_eq!(Action::Quit.description(), "[Ctrl+q]uit");
    assert_eq!(
        Action::ScrollUp.description(),
        "[Ctrl+u] scroll preview/diff/commits up"
    );
    assert_eq!(
        Action::ScrollDown.description(),
        "[Ctrl+d] scroll preview/diff/commits down"
    );
    assert_eq!(Action::ScrollTop.description(), "[g]o to top");
    assert_eq!(Action::ScrollBottom.description(), "[G]o to bottom");
}

#[test]
fn test_action_keys_cover_hidden_actions() {
    assert_eq!(Action::DiffCursorUp.keys(), "↑");
    assert_eq!(Action::DiffCursorDown.keys(), "↓");
    assert_eq!(Action::DiffDeleteLine.keys(), "x");
    assert_eq!(Action::DiffUndo.keys(), "Ctrl+z");
    assert_eq!(Action::DiffRedo.keys(), "Ctrl+y");
    assert_eq!(Action::Cancel.keys(), "Esc");
    assert_eq!(Action::Confirm.keys(), "y");
    assert_eq!(Action::ScrollUp.keys(), "Ctrl+u");
    assert_eq!(Action::ScrollDown.keys(), "Ctrl+d");
    assert_eq!(Action::ScrollTop.keys(), "g");
    assert_eq!(Action::ScrollBottom.keys(), "G");
}

#[test]
fn test_action_group_classifies_hidden_actions() {
    assert_eq!(Action::DiffCursorUp.group(), ActionGroup::Hidden);
    assert_eq!(Action::DiffUndo.group(), ActionGroup::Hidden);
    assert_eq!(Action::DiffRedo.group(), ActionGroup::Hidden);
}

#[test]
fn test_display_helpers_apply_merge_remap() {
    assert_eq!(get_display_keys(Action::Merge, true), "Ctrl+n");
    assert_eq!(
        get_display_description(Action::Merge, true),
        "[Ctrl+n] merge branch"
    );

    assert_eq!(get_display_keys(Action::Merge, false), "Ctrl+m");
    assert_eq!(
        get_display_description(Action::Merge, false),
        "[Ctrl+m]erge branch"
    );

    assert_eq!(get_display_keys(Action::Help, true), "?");
    assert_eq!(get_display_description(Action::Help, true), "[?] help");
}

#[test]
fn test_default_config() {
    let config = Config::default();
    #[cfg(windows)]
    assert_eq!(
        config_support::default_agent_program(true),
        "powershell -NoProfile -Command \"Start-Sleep -Seconds 3600\""
    );
    #[cfg(not(windows))]
    assert_eq!(
        config_support::default_agent_program(true),
        "sh -c 'sleep 3600'"
    );
    assert_eq!(
        config.default_program,
        config_support::default_agent_program(false)
    );
    assert_eq!(config.branch_prefix, "agent/");
    assert!(!config.auto_yes);
    assert_eq!(config.poll_interval_ms, 100);
}

#[test]
fn test_default_agent_program_non_test_returns_claude() {
    assert_eq!(
        config_support::default_agent_program(false),
        "claude --allow-dangerously-skip-permissions"
    );
}

#[test]
fn test_generate_branch_name() {
    let config = Config::default();

    assert_eq!(
        config.generate_branch_name("Fix Auth Bug"),
        "agent/fix-auth-bug"
    );
    assert_eq!(
        config.generate_branch_name("hello_world"),
        "agent/hello-world"
    );
    assert_eq!(config.generate_branch_name("  spaces  "), "agent/spaces");
}

#[test]
fn test_generate_branch_name_truncation() {
    let config = Config::default();
    let long_title = "a".repeat(100);
    let branch = config.generate_branch_name(&long_title);
    assert!(branch.len() <= 57);
}

#[test]
fn test_state_path() {
    let state_path = Config::default_state_path();
    assert_eq!(
        state_path.file_name().and_then(|p| p.to_str()),
        Some("state.json")
    );
    assert!(state_path.to_string_lossy().contains(".tenex"));
}

#[test]
fn test_state_path_relative_env_resolves_from_cwd() -> Result<()> {
    let expected = std::env::current_dir()?.join("state.json");
    let cwd = expected
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("expected state path parent"))?;
    assert_eq!(
        config_support::resolve_state_path_override_with_cwd(
            PathBuf::from("state.json"),
            Some(cwd)
        ),
        expected
    );
    Ok(())
}

#[test]
fn test_state_path_relative_env_falls_back_when_cwd_is_missing() {
    assert_eq!(
        config_support::resolve_state_path_override_with_cwd(PathBuf::from("state.json"), None),
        PathBuf::from("state.json")
    );
}

#[test]
fn test_state_path_from_env_value_ignores_blank_value() {
    assert!(config_support::state_path_from_env_value("   ").is_none());
}

#[test]
fn test_state_path_from_env_var_returns_none_when_env_missing() {
    assert!(config_support::state_path_from_env_var(Err(std::env::VarError::NotPresent)).is_none());
}

#[test]
fn test_state_path_from_env_var_returns_none_for_blank_value() {
    assert!(config_support::state_path_from_env_var(Ok("   ".to_string())).is_none());
}

#[test]
fn test_state_path_from_env_var_accepts_absolute_path() {
    let expected = std::env::temp_dir().join("state.json");
    assert_eq!(
        config_support::state_path_from_env_var(Ok(expected.to_string_lossy().to_string())),
        Some(expected)
    );
}

#[test]
fn test_resolve_state_path_override_with_cwd_prefers_absolute_candidate() {
    let absolute = std::env::temp_dir().join("state.json");
    let cwd = std::env::temp_dir().join("other");
    assert_eq!(
        config_support::resolve_state_path_override_with_cwd(absolute.clone(), Some(cwd)),
        absolute
    );
}

#[test]
fn test_resolve_state_path_override_joins_current_dir_for_relative_path() -> Result<()> {
    let expected = std::env::current_dir()?.join("state.json");
    assert_eq!(
        config_support::resolve_state_path_override("state.json"),
        expected
    );
    Ok(())
}

#[test]
fn test_default_instance_root_from_none_falls_back_to_dot_tenex() {
    assert_eq!(
        config_support::default_instance_root_from(None),
        PathBuf::from(".").join(".tenex")
    );
}

#[test]
fn test_default_instance_root_from_some_appends_tenex_dir() {
    let home = std::env::temp_dir().join("tenex-home");
    assert_eq!(
        config_support::default_instance_root_from(Some(home.clone())),
        home.join(".tenex")
    );
}

#[test]
fn test_instance_root_from_state_path_falls_back_when_parent_missing() {
    #[cfg(windows)]
    let root = Path::new("C:\\");
    #[cfg(not(windows))]
    let root = Path::new("/");

    assert_eq!(
        config_support::instance_root_from_state_path(root),
        PathBuf::from(".")
    );
}

#[test]
fn test_instance_root_from_state_path_returns_parent_directory() {
    let state_path = std::env::temp_dir().join("state.json");
    assert_eq!(
        config_support::instance_root_from_state_path(&state_path),
        std::env::temp_dir()
    );
}

#[test]
fn test_project_dir_name_defaults_when_repo_root_missing_file_name() {
    assert_eq!(config_support::project_dir_name(Path::new("")), "project");
}

#[test]
fn test_worktree_leaf_dir_name_strips_prefix_and_replaces_slashes() {
    assert_eq!(
        config_support::worktree_leaf_dir_name("agent/feature/foo", "agent/"),
        "feature-foo"
    );
}

#[test]
fn test_worktree_leaf_dir_name_falls_back_to_tenex_prefix() {
    assert_eq!(
        config_support::worktree_leaf_dir_name("tenex/feature/foo", "agent/"),
        "feature-foo"
    );
}

#[test]
fn test_worktree_leaf_dir_name_preserves_branch_when_no_prefix_matches() {
    assert_eq!(
        config_support::worktree_leaf_dir_name("feature/foo", "agent/"),
        "feature-foo"
    );
}

#[test]
fn test_worktree_path_for_repo_root_uses_project_name_and_leaf_name() {
    let mut config = Config::default();
    let worktree_dir = std::env::temp_dir().join("tenex-worktrees");
    config.worktree_dir = worktree_dir.clone();

    assert_eq!(
        config.worktree_path_for_repo_root(Path::new("repo"), "agent/feature/foo"),
        worktree_dir.join("repo").join("feature-foo")
    );
}

#[test]
fn test_generate_branch_name_special_chars() {
    let config = Config::default();

    // Test various special characters
    assert_eq!(
        config.generate_branch_name("fix@#$%bug"),
        "agent/fix----bug"
    );
    assert_eq!(
        config.generate_branch_name("hello/world"),
        "agent/hello-world"
    );
}

#[test]
fn test_default_worktree_dir_has_path() {
    let config = Config::default();
    assert!(config.worktree_dir.to_string_lossy().contains("worktrees"));
}

#[test]
fn test_config_clone() {
    let config = Config::default();
    let cloned = config.clone();
    assert_eq!(config, cloned);
}

#[test]
fn test_config_debug() {
    let config = Config::default();
    let debug = format!("{config:?}");
    assert!(debug.contains("Config"));
}
