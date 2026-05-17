//! Coverage tests for diff focused dispatch in non-test builds.

#[cfg(coverage)]
mod coverage {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use std::path::PathBuf;
    use tenex::action::{dispatch_diff_focused_mode, force_infallible_action_error_for_tests};
    use tenex::agent::Agent;
    use tenex::app::{DiffEdit, DiffLineMeta};
    use tenex::git::{DiffFile, DiffHunk, DiffHunkLine, DiffModel, DiffSummary, FileStatus};
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

    fn model_for_one_added_line() -> DiffModel {
        DiffModel {
            files: vec![DiffFile {
                path: PathBuf::from("file.txt"),
                status: FileStatus::Modified,
                meta: vec![
                    "diff --git a/file.txt b/file.txt".to_string(),
                    "--- a/file.txt".to_string(),
                    "+++ b/file.txt".to_string(),
                ],
                hunks: vec![DiffHunk {
                    header: "@@ -1,1 +1,1 @@".to_string(),
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![DiffHunkLine {
                        origin: '+',
                        content: "new".to_string(),
                        old_lineno: None,
                        new_lineno: Some(1),
                    }],
                }],
                additions: 1,
                deletions: 0,
            }],
            summary: DiffSummary {
                files_changed: 1,
                additions: 1,
                deletions: 0,
            },
            hash: 1,
        }
    }

    fn model_for_added_context_and_deleted_lines() -> DiffModel {
        let mut model = model_for_one_added_line();
        let hunk = &mut model.files[0].hunks[0];
        hunk.lines = vec![
            DiffHunkLine {
                origin: '+',
                content: "new".to_string(),
                old_lineno: None,
                new_lineno: Some(1),
            },
            DiffHunkLine {
                origin: ' ',
                content: "same".to_string(),
                old_lineno: Some(2),
                new_lineno: Some(2),
            },
            DiffHunkLine {
                origin: '-',
                content: "old".to_string(),
                old_lineno: Some(3),
                new_lineno: None,
            },
            DiffHunkLine {
                origin: '-',
                content: "older".to_string(),
                old_lineno: Some(4),
                new_lineno: None,
            },
        ];
        hunk.old_lines = 3;
        hunk.new_lines = 2;
        model
    }

    fn diff_app_with_agent(worktree_path: PathBuf, meta: DiffLineMeta) -> App {
        let mut app = diff_focused_app();
        app.data.storage.add(Agent::new(
            "diff agent".to_string(),
            "echo".to_string(),
            "session".to_string(),
            worktree_path,
        ));
        app.data.selected = 1;
        app.data.ui.diff_model = Some(model_for_one_added_line());
        app.data.ui.diff_line_meta = vec![meta];
        app.data.ui.diff_cursor = 0;
        app
    }

    fn assert_git_apply_error(result: anyhow::Result<()>) {
        let err = result.expect_err("expected git apply error");
        assert!(
            err.to_string().contains("git apply"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_diff_focused_dispatch_covers_non_test_bindings_and_fallbacks() {
        let mut app = diff_focused_app();

        // Covered by default bindings in non-test builds.
        app.data.active_tab = Tab::Preview;
        app.data.ui.diff_visual_anchor = None;
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)
            .expect("inactive single line delete");
        dispatch_diff_focused_mode(&mut app, KeyCode::Up, KeyModifiers::NONE)
            .expect("inactive cursor up");
        dispatch_diff_focused_mode(&mut app, KeyCode::Down, KeyModifiers::NONE)
            .expect("inactive cursor down");
        app.data.ui.diff_visual_anchor = Some(0);
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)
            .expect("inactive delete");
        assert_eq!(app.data.ui.diff_visual_anchor, Some(0));

        app.data.active_tab = Tab::Diff;
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('V'), KeyModifiers::NONE)
            .expect("start visual selection");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('V'), KeyModifiers::NONE)
            .expect("clear visual selection");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('x'), KeyModifiers::NONE)
            .expect("delete without agent");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL)
            .expect("undo without edit");
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL)
            .expect("redo without edit");
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

        app.enter_mode(DiffFocusedMode.into());
        dispatch_diff_focused_mode(&mut app, KeyCode::Char('Q'), KeyModifiers::CONTROL)
            .expect("uppercase ctrl-q");
    }

    #[test]
    fn test_diff_focused_dispatch_covers_forced_infallible_action_errors_in_non_test_build() {
        let mut app = diff_focused_app();

        let _guard = force_infallible_action_error_for_tests();
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
    }

    #[test]
    fn test_diff_focused_dispatch_covers_apply_errors_in_non_test_build() {
        let tmp = tempfile::TempDir::new().expect("expected temp dir");
        let worktree_path = tmp.path().to_path_buf();
        let line_meta = DiffLineMeta::Line {
            file_idx: 0,
            hunk_idx: 0,
            line_idx: 0,
        };
        let hunk_meta = DiffLineMeta::Hunk {
            file_idx: 0,
            hunk_idx: 0,
        };

        let mut range_app = diff_app_with_agent(worktree_path.clone(), line_meta);
        range_app.data.ui.diff_visual_anchor = Some(0);
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut range_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));

        let mut missing_model_app = diff_focused_app();
        missing_model_app.data.storage.add(Agent::new(
            "diff agent".to_string(),
            "echo".to_string(),
            "session".to_string(),
            worktree_path.clone(),
        ));
        missing_model_app.data.selected = 1;
        missing_model_app.data.ui.diff_visual_anchor = Some(0);
        dispatch_diff_focused_mode(
            &mut missing_model_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )
        .expect("missing model should be reported as status");
        assert_eq!(
            missing_model_app.data.ui.status_message.as_deref(),
            Some("Diff not loaded yet")
        );

        let mut invalid_meta_app = diff_app_with_agent(
            worktree_path.clone(),
            DiffLineMeta::Line {
                file_idx: 99,
                hunk_idx: 0,
                line_idx: 0,
            },
        );
        invalid_meta_app.data.ui.diff_visual_anchor = Some(0);
        dispatch_diff_focused_mode(
            &mut invalid_meta_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )
        .expect("invalid meta should no-op");
        assert_eq!(
            invalid_meta_app.data.ui.status_message.as_deref(),
            Some("Select a changed line (+/-) to delete")
        );

        let mut info_meta_app = diff_app_with_agent(worktree_path.clone(), DiffLineMeta::Info);
        info_meta_app.data.ui.diff_visual_anchor = Some(0);
        dispatch_diff_focused_mode(&mut info_meta_app, KeyCode::Char('x'), KeyModifiers::NONE)
            .expect("info meta should no-op");
        assert_eq!(
            info_meta_app.data.ui.status_message.as_deref(),
            Some("Select a changed line (+/-) to delete")
        );

        let mut invalid_hunk_app = diff_app_with_agent(
            worktree_path.clone(),
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 99,
                line_idx: 0,
            },
        );
        invalid_hunk_app.data.ui.diff_visual_anchor = Some(0);
        dispatch_diff_focused_mode(
            &mut invalid_hunk_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )
        .expect("invalid hunk should no-op");
        assert_eq!(
            invalid_hunk_app.data.ui.status_message.as_deref(),
            Some("Select a changed line (+/-) to delete")
        );

        let mut invalid_line_app = diff_app_with_agent(
            worktree_path.clone(),
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 99,
            },
        );
        invalid_line_app.data.ui.diff_visual_anchor = Some(0);
        dispatch_diff_focused_mode(
            &mut invalid_line_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )
        .expect("invalid line should no-op");
        assert_eq!(
            invalid_line_app.data.ui.status_message.as_deref(),
            Some("Select a changed line (+/-) to delete")
        );

        let mut context_line_app = diff_app_with_agent(
            worktree_path.clone(),
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 1,
            },
        );
        context_line_app.data.ui.diff_model = Some(model_for_added_context_and_deleted_lines());
        dispatch_diff_focused_mode(
            &mut context_line_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )
        .expect("context line should be reported as status");
        assert_eq!(
            context_line_app.data.ui.status_message.as_deref(),
            Some("Select a changed line (+/-) to delete")
        );

        let mut deleted_line_app = diff_app_with_agent(
            worktree_path.clone(),
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 2,
            },
        );
        deleted_line_app.data.ui.diff_model = Some(model_for_added_context_and_deleted_lines());
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut deleted_line_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));

        let mut range_with_unselected_delete_app = diff_app_with_agent(
            worktree_path.clone(),
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 2,
            },
        );
        range_with_unselected_delete_app.data.ui.diff_model =
            Some(model_for_added_context_and_deleted_lines());
        range_with_unselected_delete_app.data.ui.diff_line_meta = vec![
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 0,
            },
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 1,
            },
            DiffLineMeta::Line {
                file_idx: 0,
                hunk_idx: 0,
                line_idx: 2,
            },
        ];
        range_with_unselected_delete_app.data.ui.diff_cursor = 2;
        range_with_unselected_delete_app.data.ui.diff_visual_anchor = Some(0);
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut range_with_unselected_delete_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));

        let mut line_app = diff_app_with_agent(worktree_path.clone(), line_meta);
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut line_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));

        let mut hunk_app = diff_app_with_agent(worktree_path.clone(), hunk_meta);
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut hunk_app,
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));

        let mut undo_app = diff_app_with_agent(worktree_path.clone(), line_meta);
        undo_app.data.ui.diff_undo.push(DiffEdit {
            patch: "not a patch".to_string(),
            applied_reverse: false,
        });
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut undo_app,
            KeyCode::Char('z'),
            KeyModifiers::CONTROL,
        ));

        let mut redo_app = diff_app_with_agent(worktree_path, line_meta);
        redo_app.data.ui.diff_redo.push(DiffEdit {
            patch: "not a patch".to_string(),
            applied_reverse: false,
        });
        assert_git_apply_error(dispatch_diff_focused_mode(
            &mut redo_app,
            KeyCode::Char('y'),
            KeyModifiers::CONTROL,
        ));
    }
}
