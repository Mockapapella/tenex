//! Preview focused mode key handling
//!
//! Handles key events when the preview pane is focused, forwarding
//! keystrokes to the PTY backend.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use tenex::app::App;

/// Handle key events in `PreviewFocused` mode
pub fn handle_preview_focused_mode(
    app: &mut App,
    code: KeyCode,
    modifiers: KeyModifiers,
    batched_keys: &mut Vec<String>,
) {
    // Ctrl+q exits preview focus mode (same key quits app when not focused)
    if code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL) {
        app.exit_mode();
        return;
    }

    // Collect keys for batched sending (done after event drain loop)
    if let Some(sequence) = keycode_to_input_sequence(code, modifiers) {
        batched_keys.push(sequence);
    }
}

/// Convert a `KeyCode` and modifiers to input escape sequences.
pub fn keycode_to_input_sequence(code: KeyCode, modifiers: KeyModifiers) -> Option<String> {
    let is_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let is_alt = modifiers.contains(KeyModifiers::ALT);

    if let KeyCode::Char(c) = code {
        let mut bytes = Vec::new();
        if is_ctrl {
            let upper = c.to_ascii_uppercase();
            if upper.is_ascii() {
                bytes.push((upper as u8) & 0x1f);
            } else {
                return None;
            }
        } else {
            bytes.extend_from_slice(c.to_string().as_bytes());
        }

        if is_alt {
            let mut with_alt = Vec::with_capacity(bytes.len() + 1);
            with_alt.push(0x1b);
            with_alt.extend_from_slice(&bytes);
            bytes = with_alt;
        }

        return String::from_utf8(bytes).ok();
    }

    let base = base_sequence(code)?;
    if is_alt && !base.starts_with(b"\x1b") {
        let mut bytes = Vec::with_capacity(base.len() + 1);
        bytes.push(0x1b);
        bytes.extend_from_slice(base);
        return String::from_utf8(bytes).ok();
    }
    let param = modifier_param(is_ctrl, is_alt);
    let sequence = param.map_or_else(|| base.to_vec(), |param| apply_modifier(base, param));

    String::from_utf8(sequence).ok()
}

const fn base_sequence(code: KeyCode) -> Option<&'static [u8]> {
    match code {
        KeyCode::Enter => Some(b"\r"),
        KeyCode::Esc => Some(b"\x1b"),
        KeyCode::Backspace => Some(&[0x7f]),
        KeyCode::Tab => Some(b"\t"),
        KeyCode::BackTab => Some(b"\x1b[Z"),
        KeyCode::Up => Some(b"\x1b[A"),
        KeyCode::Down => Some(b"\x1b[B"),
        KeyCode::Left => Some(b"\x1b[D"),
        KeyCode::Right => Some(b"\x1b[C"),
        KeyCode::Home => Some(b"\x1b[H"),
        KeyCode::End => Some(b"\x1b[F"),
        KeyCode::PageUp => Some(b"\x1b[5~"),
        KeyCode::PageDown => Some(b"\x1b[6~"),
        KeyCode::Delete => Some(b"\x1b[3~"),
        KeyCode::Insert => Some(b"\x1b[2~"),
        KeyCode::F(1) => Some(b"\x1bOP"),
        KeyCode::F(2) => Some(b"\x1bOQ"),
        KeyCode::F(3) => Some(b"\x1bOR"),
        KeyCode::F(4) => Some(b"\x1bOS"),
        KeyCode::F(5) => Some(b"\x1b[15~"),
        KeyCode::F(6) => Some(b"\x1b[17~"),
        KeyCode::F(7) => Some(b"\x1b[18~"),
        KeyCode::F(8) => Some(b"\x1b[19~"),
        KeyCode::F(9) => Some(b"\x1b[20~"),
        KeyCode::F(10) => Some(b"\x1b[21~"),
        KeyCode::F(11) => Some(b"\x1b[23~"),
        KeyCode::F(12) => Some(b"\x1b[24~"),
        _ => None,
    }
}

const fn modifier_param(is_ctrl: bool, is_alt: bool) -> Option<u8> {
    match (is_ctrl, is_alt) {
        (false, false) => None,
        (true, false) => Some(5),
        (false, true) => Some(3),
        (true, true) => Some(7),
    }
}

fn apply_modifier(base: &[u8], param: u8) -> Vec<u8> {
    if base.starts_with(b"\x1b[") {
        let rest = &base[2..];
        if rest.len() == 1 && rest[0].is_ascii_alphabetic() {
            let code = rest[0] as char;
            return format!("\x1b[1;{param}{code}").into_bytes();
        }
        if rest.ends_with(b"~") {
            let digits = &rest[..rest.len() - 1];
            let number = String::from_utf8_lossy(digits);
            return format!("\x1b[{number};{param}~").into_bytes();
        }
    }

    if base.starts_with(b"\x1bO") && base.len() == 3 {
        let code = base[2] as char;
        return format!("\x1b[1;{param}{code}").into_bytes();
    }

    base.to_vec()
}
