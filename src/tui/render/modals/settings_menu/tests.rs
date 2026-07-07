use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::config::Config;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
fn test_render_settings_menu_overlay_renders_content() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let mut app = App::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    );
    app.data.settings_menu.selected = 1;

    terminal
        .draw(|frame| {
            render_settings_menu_overlay(frame, &app);
        })
        .expect("draw");

    assert!(!terminal.backend().buffer().content.is_empty());
}
