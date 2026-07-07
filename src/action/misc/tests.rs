use super::*;
use crate::agent::{Agent, Status, Storage};
use std::path::PathBuf;

fn make_agent(title: &str, status: Status) -> Agent {
    let pid = std::process::id();
    let mut agent = Agent::new(
        title.to_string(),
        "echo".to_string(),
        format!("tenex-action-misc-test-{pid}/{title}"),
        PathBuf::from(format!("/tmp/tenex-action-misc-test-{pid}/{title}")),
    );
    agent.set_status(status);
    agent
}

#[test]
fn test_help_action_resets_help_scroll_in_normal() {
    let mut data = AppData::new(
        crate::config::Config::default(),
        Storage::default(),
        crate::app::Settings::default(),
        false,
    );
    data.ui.help_scroll = 123;

    let next = HelpAction.execute(NormalMode, &mut data).unwrap();
    assert_eq!(next, HelpMode.into());
    assert_eq!(data.ui.help_scroll, 0);
}

#[test]
fn test_quit_action_enters_confirming_when_running_agents() {
    let mut storage = Storage::new();
    storage.add(make_agent("running", Status::Running));
    let mut data = AppData::new(
        crate::config::Config::default(),
        storage,
        crate::app::Settings::default(),
        false,
    );

    let next = QuitAction.execute(NormalMode, &mut data).unwrap();
    assert_eq!(
        next,
        ConfirmingMode {
            action: ConfirmAction::Quit
        }
        .into()
    );
    assert!(!data.should_quit);
}

#[test]
fn test_quit_action_sets_should_quit_when_no_running_agents() {
    let mut storage = Storage::new();
    storage.add(make_agent("starting", Status::Starting));
    let mut data = AppData::new(
        crate::config::Config::default(),
        storage,
        crate::app::Settings::default(),
        false,
    );

    let next = QuitAction.execute(NormalMode, &mut data).unwrap();
    assert_eq!(next, AppMode::normal());
    assert!(data.should_quit);
}

#[test]
fn test_quit_action_scrolling_returns_scrolling_when_quitting() {
    let mut storage = Storage::new();
    storage.add(make_agent("starting", Status::Starting));
    let mut data = AppData::new(
        crate::config::Config::default(),
        storage,
        crate::app::Settings::default(),
        false,
    );

    let next = QuitAction.execute(ScrollingMode, &mut data).unwrap();
    assert_eq!(next, ScrollingMode.into());
    assert!(data.should_quit);
}

#[test]
fn test_quit_action_scrolling_enters_confirming_when_running_agents() {
    let mut storage = Storage::new();
    storage.add(make_agent("running", Status::Running));
    let mut data = AppData::new(
        crate::config::Config::default(),
        storage,
        crate::app::Settings::default(),
        false,
    );

    let next = QuitAction.execute(ScrollingMode, &mut data).unwrap();
    assert_eq!(
        next,
        ConfirmingMode {
            action: ConfirmAction::Quit
        }
        .into()
    );
    assert!(!data.should_quit);
}

#[test]
fn test_command_palette_action_enters_command_palette_mode() {
    let mut data = AppData::new(
        crate::config::Config::default(),
        Storage::default(),
        crate::app::Settings::default(),
        false,
    );

    let next = CommandPaletteAction.execute(NormalMode, &mut data).unwrap();
    assert_eq!(next, CommandPaletteMode.into());
}

#[test]
fn test_cancel_action_clears_input_in_normal() {
    let mut data = AppData::new(
        crate::config::Config::default(),
        Storage::default(),
        crate::app::Settings::default(),
        false,
    );
    data.input.buffer = "hello".to_string();
    data.input.cursor = data.input.buffer.len();

    let next = CancelAction.execute(NormalMode, &mut data).unwrap();
    assert_eq!(next, AppMode::normal());
    assert!(data.input.buffer.is_empty());
    assert_eq!(data.input.cursor, 0);
}

#[test]
fn test_cancel_action_does_not_clear_input_in_scrolling() {
    let mut data = AppData::new(
        crate::config::Config::default(),
        Storage::default(),
        crate::app::Settings::default(),
        false,
    );
    data.input.buffer = "hello".to_string();
    data.input.cursor = data.input.buffer.len();

    let next = CancelAction.execute(ScrollingMode, &mut data).unwrap();
    assert_eq!(next, AppMode::normal());
    assert_eq!(data.input.buffer, "hello");
    assert_eq!(data.input.cursor, 5);
}
