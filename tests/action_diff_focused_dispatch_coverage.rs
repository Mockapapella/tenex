//! Coverage tests for diff focused dispatch in non-test builds.

#[cfg(coverage)]
mod coverage {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use tenex::action::{
        dispatch_diff_focused_mode, with_forced_infallible_action_error_for_tests,
    };
    use tenex::state::DiffFocusedMode;
    use tenex::{App, Tab};

    fn diff_focused_app() -> App {
        let mut app = App::default();
        app.data.active_tab = Tab::Diff;
        app.data.ui.set_preview_dimensions(80, 1);
        app.data.ui.set_diff_content("line-0\nline-1\nline-2\n");
        app.enter_mode(DiffFocusedMode.into());
        app
    }

    #[test]
    fn test_diff_focused_dispatch_covers_non_test_bindings_and_fallbacks() {
        let mut app = diff_focused_app();

        // Covered by default bindings in non-test builds.
        dispatch_diff_focused_mode(&mut app, KeyCode::Char(' '), KeyModifiers::NONE)
            .expect("toggle collapse");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL)
            .expect("scroll up");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL)
            .expect("scroll down");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('G'), KeyModifiers::NONE)
            .expect("scroll bottom");

        // Unbound keys should no-op in diff focus.
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('~'), KeyModifiers::NONE)
            .expect("unbound key");

        // Non-diff actions should fall back to normal-mode dispatch.
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('a'), KeyModifiers::NONE)
            .expect("fallback dispatch");
    }

    #[test]
    fn test_diff_focused_dispatch_covers_forced_infallible_action_errors_in_non_test_build() {
        let mut app = diff_focused_app();

        with_forced_infallible_action_error_for_tests(|| {
            for (code, modifiers) in [
                (KeyCode::Char('q'), KeyModifiers::CONTROL),
                (KeyCode::Up, KeyModifiers::NONE),
                (KeyCode::Down, KeyModifiers::NONE),
            ] {
                let err = dispatch_diff_focused_mode(&mut app, code, modifiers)
                    .expect_err("expected forced diff focused dispatch error");
                assert!(
                    err.to_string()
                        .contains("forced infallible action error for test"),
                    "err: {err}"
                );
            }
        });
    }
}
