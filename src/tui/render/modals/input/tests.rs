use super::*;
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
fn test_wrap_input_with_cursor_wraps_at_width() {
    let (lines, cursor_line) = wrap_input_with_cursor("abc", 2);
    assert_eq!(lines, vec!["ab".to_string(), "c".to_string()]);
    assert_eq!(cursor_line, 0);
}

#[test]
fn test_render_input_overlay_inserts_cursor_in_middle() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_input_overlay(frame, "Title", "Prompt", "abc", 1);
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains("a│bc"));
}

#[test]
fn test_render_input_overlay_scrollbar_renders_track_below_thumb() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let long_input = (0..30)
        .map(|idx| format!("line-{idx:02}"))
        .collect::<Vec<_>>()
        .join("\n");

    terminal
        .draw(|frame| {
            render_input_overlay(frame, "Title", "Prompt", &long_input, 0);
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    assert!(text.contains('█'));
    assert!(text.contains('░'));
}
