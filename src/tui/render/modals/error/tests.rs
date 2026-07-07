use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn test_render_error_modal_short_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_error_modal(frame, "Test error message");
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_error_modal_long_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
            .draw(|frame| {
                render_error_modal(
                    frame,
                    "This is a very long error message that should be word wrapped across multiple lines in the modal dialog box",
                );
            })
            .expect("draw");
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_success_modal_short_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_success_modal(frame, "Operation completed");
        })
        .expect("draw");
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_success_modal_long_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
            .draw(|frame| {
                render_success_modal(
                    frame,
                    "This is a very long success message that should be word wrapped across multiple lines in the modal dialog box",
                );
            })
            .expect("draw");
    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_error_modal_empty_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_error_modal(frame, "");
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}

#[test]
fn test_render_success_modal_empty_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_success_modal(frame, "");
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}
