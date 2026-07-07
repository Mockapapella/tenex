use super::*;
use crate::agent::Storage;
use crate::app::Settings;
use crate::config::Config;
use semver::Version;

fn app_data() -> AppData {
    AppData::new(
        Config::default(),
        Storage::new(),
        Settings::default(),
        false,
    )
}

#[test]
fn test_changelog_max_scroll_returns_zero_for_short_content() {
    let mut data = app_data();
    data.ui.terminal_dimensions = Some((80, 24));

    let state = ChangelogMode {
        title: "Changelog".to_string(),
        lines: vec!["One".to_string(), "Two".to_string(), "Three".to_string()],
        mark_seen_version: Some(Version::new(1, 2, 3)),
    };

    assert_eq!(changelog_max_scroll(&data, &state), 0);
}

#[test]
fn test_changelog_max_scroll_accounts_for_wrapping() {
    let mut data = app_data();
    data.ui.terminal_dimensions = Some((80, 24));

    let mut lines = Vec::new();
    for _ in 0..50 {
        lines.push(
            "- This bullet is long enough that it should wrap within the modal width.".to_string(),
        );
    }

    let state = ChangelogMode {
        title: "Changelog".to_string(),
        lines,
        mark_seen_version: None,
    };

    assert!(changelog_max_scroll(&data, &state) > 0);
}

#[test]
fn test_changelog_max_scroll_returns_zero_when_inner_width_is_zero() {
    let mut data = app_data();
    data.ui.terminal_dimensions = Some((3, 24));

    let state = ChangelogMode {
        title: "Changelog".to_string(),
        lines: (0..50)
            .map(|idx| format!("- This is a long changelog entry {idx}"))
            .collect(),
        mark_seen_version: None,
    };

    assert_eq!(changelog_max_scroll(&data, &state), 0);
}

#[test]
fn test_terminal_frame_area_uses_preview_dimensions_when_missing_terminal_size() {
    let mut data = app_data();
    data.ui.terminal_dimensions = None;
    data.ui.preview_dimensions = Some((70, 20));

    let state = ChangelogMode {
        title: "Changelog".to_string(),
        lines: (0..40).map(|idx| format!("Line {idx}")).collect(),
        mark_seen_version: None,
    };

    assert!(changelog_max_scroll(&data, &state) > 0);
}

#[test]
fn test_wrap_single_line_chunks_long_word() {
    let wrapped = wrap_single_line("- abcdefghijklmnop", 8);
    assert_eq!(
        wrapped,
        vec![
            "- abcdef".to_string(),
            "  ghijkl".to_string(),
            "  mnop".to_string()
        ]
    );
}

#[test]
fn test_infer_frame_width_from_preview_width_ceils() {
    assert_eq!(infer_frame_width_from_preview_width(70), 104);
}

#[test]
fn test_wrap_single_line_returns_line_when_content_is_empty() {
    let wrapped = wrap_single_line("  -   ", 5);
    assert_eq!(wrapped, vec!["  -   ".to_string()]);
}

#[test]
fn test_wrap_single_line_returns_empty_when_width_zero() {
    assert!(wrap_single_line("line", 0).is_empty());
}

#[test]
fn test_wrap_single_line_collapses_extra_spaces_without_wrapping() {
    let wrapped = wrap_single_line("- a  b", 5);
    assert_eq!(wrapped, vec!["- a b".to_string()]);
}

#[test]
fn test_wrap_single_line_chunks_when_width_smaller_than_prefix() {
    let wrapped = wrap_single_line("    - abc", 4);
    assert_eq!(
        wrapped,
        vec!["    ".to_string(), "- ab".to_string(), "c".to_string()]
    );
}

#[test]
fn test_wrap_single_line_numbered_list_indent() {
    let wrapped = wrap_single_line("  12. one two three four", 14);
    assert_eq!(
        wrapped,
        vec![
            "  12. one two".to_string(),
            "      three".to_string(),
            "      four".to_string()
        ]
    );
}

#[test]
fn test_wrap_single_line_splits_digits_only_input() {
    let wrapped = wrap_single_line("123", 2);
    assert_eq!(wrapped, vec!["12".to_string(), "3".to_string()]);
}

#[test]
fn test_wrap_single_line_supports_asterisk_bullets() {
    let wrapped = wrap_single_line("* abc def", 6);
    assert_eq!(wrapped, vec!["* abc".to_string(), "  def".to_string()]);
}

#[test]
fn test_wrap_single_line_supports_plus_bullets() {
    let wrapped = wrap_single_line("+ abc def", 6);
    assert_eq!(wrapped, vec!["+ abc".to_string(), "  def".to_string()]);
}

#[test]
fn test_wrap_single_line_does_not_treat_non_dot_digit_prefix_as_numbered_list() {
    let wrapped = wrap_single_line("12x one two", 6);
    assert_eq!(
        wrapped,
        vec!["12x".to_string(), "one".to_string(), "two".to_string()]
    );
}

#[test]
fn test_wrap_single_line_does_not_treat_dot_without_space_as_numbered_list() {
    let wrapped = wrap_single_line("12.a one", 6);
    assert_eq!(wrapped, vec!["12.a".to_string(), "one".to_string()]);
}

#[test]
fn test_wrapped_line_count_returns_zero_when_width_zero() {
    assert_eq!(wrapped_line_count(&[String::from("line")], 0), 0);
}

#[test]
fn test_wrapped_line_count_includes_empty_lines() {
    assert_eq!(
        wrapped_line_count(&[String::new(), String::from("line")], 10),
        2
    );
}

#[test]
fn test_split_at_char_boundary_splits_ascii() {
    let (left, right) = split_at_char_boundary("abcdef", 3);
    assert_eq!(left, "abc");
    assert_eq!(right, "def");
}

#[test]
fn test_split_at_char_boundary_splits_multibyte() {
    let (left, right) = split_at_char_boundary("✅done", 1);
    assert_eq!(left, "✅");
    assert_eq!(right, "done");
}

#[test]
fn test_chunk_into_width_returns_empty_when_width_zero() {
    assert!(chunk_into_width("abcdef", 0).is_empty());
}

#[test]
fn test_wrap_single_line_wraps_indented_content() {
    let wrapped = wrap_single_line("  hello world from tenex", 10);
    assert_eq!(
        wrapped,
        vec![
            "  hello".to_string(),
            "  world".to_string(),
            "  from".to_string(),
            "  tenex".to_string()
        ]
    );
}
