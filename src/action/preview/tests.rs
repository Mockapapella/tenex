use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::config::Config;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use tempfile::NamedTempFile;

#[test]
fn test_keycode_to_input_sequence_char_variants() {
    assert_eq!(
        keycode_to_input_sequence(KeyCode::Char('a'), KeyModifiers::NONE).as_deref(),
        Some("a")
    );

    assert_eq!(
        keycode_to_input_sequence(KeyCode::Char('a'), KeyModifiers::CONTROL)
            .map(std::string::String::into_bytes),
        Some(vec![0x01])
    );

    assert_eq!(
        keycode_to_input_sequence(KeyCode::Char('a'), KeyModifiers::ALT)
            .map(std::string::String::into_bytes),
        Some(vec![0x1b, b'a'])
    );

    assert_eq!(
        keycode_to_input_sequence(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )
        .map(std::string::String::into_bytes),
        Some(vec![0x1b, 0x01])
    );
}

#[test]
fn test_keycode_to_input_sequence_applies_modifiers() {
    assert_eq!(
        keycode_to_input_sequence(KeyCode::Up, KeyModifiers::CONTROL).as_deref(),
        Some("\u{1b}[1;5A")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::Up, KeyModifiers::ALT).as_deref(),
        Some("\u{1b}[1;3A")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::Up, KeyModifiers::CONTROL | KeyModifiers::ALT)
            .as_deref(),
        Some("\u{1b}[1;7A")
    );

    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(1), KeyModifiers::CONTROL).as_deref(),
        Some("\u{1b}[1;5P")
    );

    assert_eq!(
        keycode_to_input_sequence(KeyCode::PageUp, KeyModifiers::ALT).as_deref(),
        Some("\u{1b}[5;3~")
    );
}

#[test]
fn test_keycode_to_input_sequence_covers_more_special_keys() {
    assert_eq!(
        keycode_to_input_sequence(KeyCode::Tab, KeyModifiers::NONE).as_deref(),
        Some("\t")
    );

    assert_eq!(
        keycode_to_input_sequence(KeyCode::BackTab, KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[Z")
    );

    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(2), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}OQ")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(3), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}OR")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(4), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}OS")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(5), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[15~")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(6), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[17~")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(7), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[18~")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(8), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[19~")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(9), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[20~")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(10), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[21~")
    );
    assert_eq!(
        keycode_to_input_sequence(KeyCode::F(11), KeyModifiers::NONE).as_deref(),
        Some("\u{1b}[23~")
    );

    assert!(keycode_to_input_sequence(KeyCode::F(13), KeyModifiers::NONE).is_none());
}

#[test]
fn test_apply_modifier_falls_back_for_unknown_escape_sequences() {
    assert_eq!(apply_modifier(b"\x1b[1", 5), b"\x1b[1".to_vec());
    assert_eq!(apply_modifier(b"\x1b[123", 5), b"\x1b[123".to_vec());
    assert_eq!(apply_modifier(b"\x1bO", 5), b"\x1bO".to_vec());
    assert_eq!(apply_modifier(b"hello", 5), b"hello".to_vec());
}

#[test]
fn test_keycode_to_input_sequence_alt_prefix_for_non_escape_base() {
    assert_eq!(
        keycode_to_input_sequence(KeyCode::Enter, KeyModifiers::ALT)
            .map(std::string::String::into_bytes),
        Some(vec![0x1b, b'\r'])
    );
}

#[test]
fn test_preview_focused_actions_buffer_and_exit() {
    let temp_file = NamedTempFile::new().unwrap();
    let storage = Storage::with_path(temp_file.path().to_path_buf());
    let mut data = AppData::new(Config::default(), storage, Settings::default(), false);

    let mut batched_keys = Vec::new();
    assert_eq!(
        ForwardKeystrokeAction {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            batched_keys: &mut batched_keys,
        }
        .execute(PreviewFocusedMode, &mut data)
        .unwrap(),
        PreviewFocusedMode.into()
    );
    assert_eq!(batched_keys, vec!["a".to_string()]);

    let mut batched_keys = Vec::new();
    assert_eq!(
        ForwardKeystrokeAction {
            code: KeyCode::Char('é'),
            modifiers: KeyModifiers::CONTROL,
            batched_keys: &mut batched_keys,
        }
        .execute(PreviewFocusedMode, &mut data)
        .unwrap(),
        PreviewFocusedMode.into()
    );
    assert!(batched_keys.is_empty());

    assert_eq!(
        UnfocusPreviewAction
            .execute(PreviewFocusedMode, &mut data)
            .unwrap(),
        AppMode::normal()
    );
}
