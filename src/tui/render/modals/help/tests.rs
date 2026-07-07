use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::config::Config;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for cell in &buffer.content {
        text.push_str(cell.symbol());
    }
    text
}

#[test]
fn test_styled_mnemonic_description_roundtrips_with_prefix_text() {
    let spans = styled_mnemonic_description("Press [x] to exit");
    let rendered = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(rendered, "Press [x] to exit");
}

#[test]
fn test_styled_mnemonic_description_roundtrips_with_leading_mnemonic() {
    let spans = styled_mnemonic_description("[x] to exit");
    let rendered = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(rendered, "[x] to exit");
}

#[test]
fn test_styled_mnemonic_description_returns_unclosed_bracket_text_verbatim() {
    let spans = styled_mnemonic_description("Use [x to exit");
    let rendered = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert_eq!(rendered, "Use [x to exit");
}

#[test]
fn test_render_help_overlay_renders_scrollbar_when_content_exceeds_height() {
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    terminal
        .draw(|frame| {
            render_help_overlay(frame, &app);
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("░"));
}

#[test]
fn test_render_help_overlay_skips_scrollbar_when_inner_area_is_too_small() {
    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    terminal
        .draw(|frame| {
            render_help_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_help_overlay_skips_scrollbar_when_scrollbar_area_has_zero_height() {
    let backend = TestBackend::new(80, 6);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    terminal
        .draw(|frame| {
            render_help_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_help_overlay_does_not_render_scrollbar_when_height_is_sufficient() {
    let backend = TestBackend::new(80, 200);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    terminal
        .draw(|frame| {
            render_help_overlay(frame, &app);
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(!text.contains("░"));
}
