use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn test_render_preparing_docker_modal() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_preparing_docker_modal(
                frame,
                "Building the shipped Tenex Docker worker image for first use.",
            );
        })
        .expect("draw");
}

#[test]
fn test_render_preparing_docker_modal_empty_message() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    terminal
        .draw(|frame| {
            render_preparing_docker_modal(frame, "");
        })
        .expect("draw");
}
