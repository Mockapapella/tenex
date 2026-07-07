use super::*;

#[test]
fn test_push_u8_decimal_formats_values() {
    let mut out = String::new();
    push_u8_decimal(&mut out, 7);
    assert_eq!(out, "7");

    out.clear();
    push_u8_decimal(&mut out, 42);
    assert_eq!(out, "42");

    out.clear();
    push_u8_decimal(&mut out, 123);
    assert_eq!(out, "123");
}

#[test]
fn test_render_screen_rows_basic() {
    let mut parser = vt100::Parser::new(2, 3, 0);
    parser.process(b"\x1b[31mA\x1b[0m");
    let lines = render_screen_rows(parser.screen());
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains('A'));
    assert!(lines[0].ends_with("\x1b[0m"));
}

#[test]
fn test_capture_lines_includes_tail_lines() {
    use std::fmt::Write as _;

    let mut parser = vt100::Parser::new(5, 20, 1_000);

    let mut output = String::new();
    for i in 0..29 {
        let _ = write!(&mut output, "L{i}\r\n");
    }
    output.push_str("L29");
    parser.process(output.as_bytes());

    let captured = capture_lines(&mut parser, usize::MAX);
    let lines: Vec<&str> = captured.lines().collect();
    assert!(lines.len() >= 10);

    let tail_start = lines.len().saturating_sub(4);
    let tail = &lines[tail_start..];

    assert!(lines.first().is_some_and(|line| line.contains("L0")));
    assert!(lines.last().is_some_and(|line| line.contains("L29")));

    assert!(tail[0].contains("L26"));
    assert!(tail[1].contains("L27"));
    assert!(tail[2].contains("L28"));
    assert!(tail[3].contains("L29"));
}

#[test]
fn test_render_row_writes_default_style_for_out_of_bounds_cells() {
    let mut parser = vt100::Parser::new(1, 1, 0);
    parser.process(b"A");

    let line = render_row(parser.screen(), 0, 3);
    assert!(line.contains('A'));
    assert!(line.contains("\x1b[0m"));
}

#[test]
fn test_render_row_resets_style_for_out_of_bounds_cells() {
    let mut parser = vt100::Parser::new(1, 1, 0);
    parser.process(b"\x1b[31mA");

    let line = render_row(parser.screen(), 0, 3);
    assert!(line.contains("A\x1b[0m  \x1b[0m"));
}

#[test]
fn test_render_row_skips_wide_continuation_cells() {
    let mut parser = vt100::Parser::new(1, 4, 0);
    parser.process("你".as_bytes());

    let lines = render_screen_rows(parser.screen());
    assert!(lines[0].contains("你"));
}

#[test]
fn test_render_row_emits_modifiers_and_colors() {
    let mut parser = vt100::Parser::new(1, 4, 0);
    parser.process(b"\x1b[1;3;4;7;38;5;42;48;2;1;2;3mA");
    parser.process(b"\x1b[38;2;4;5;6;48;5;7mB\x1b[0m");

    let lines = render_screen_rows(parser.screen());
    let line = &lines[0];
    assert!(line.contains("38;5;42"));
    assert!(line.contains("48;2;1;2;3"));
    assert!(line.contains("38;2;4;5;6"));
    assert!(line.contains("48;5;7"));
    assert!(line.contains(";1"));
    assert!(line.contains(";3"));
    assert!(line.contains(";4"));
    assert!(line.contains(";7"));
}
