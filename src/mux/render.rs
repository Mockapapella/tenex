//! Helpers for rendering vt100 screens to ANSI strings.

use vt100::Color;

pub fn capture_lines(parser: &mut vt100::Parser, requested: usize) -> String {
    let (rows, _cols) = parser.screen().size();
    let height = usize::from(rows);

    let original_offset = parser.screen().scrollback();
    parser.screen_mut().set_scrollback(usize::MAX);
    let scrollback_len = parser.screen().scrollback();
    let total_lines = scrollback_len.saturating_add(height);

    let requested = if requested == usize::MAX {
        total_lines
    } else {
        requested.min(total_lines)
    };

    let start_line = total_lines.saturating_sub(requested);
    let mut collected: Vec<String> = Vec::with_capacity(requested);

    if start_line >= scrollback_len {
        parser.screen_mut().set_scrollback(0);
        let rows = render_screen_rows(parser.screen());
        let screen_start = start_line.saturating_sub(scrollback_len).min(height);
        collected.extend(rows.into_iter().skip(screen_start));
        let result = collected.join("\n");
        parser.screen_mut().set_scrollback(original_offset);
        return result;
    }

    let scrollback_start = start_line;
    let first_page_start = scrollback_start.saturating_sub(scrollback_start % height);
    let mut page_start = first_page_start;
    let mut skip_within_page = scrollback_start.saturating_sub(first_page_start);

    while page_start < scrollback_len {
        let offset = scrollback_len.saturating_sub(page_start);
        parser.screen_mut().set_scrollback(offset);
        let rows = render_screen_rows(parser.screen());
        let available = scrollback_len.saturating_sub(page_start).min(height);
        let skip = skip_within_page.min(available);

        collected.extend(rows.into_iter().take(available).skip(skip));

        page_start = page_start.saturating_add(height);
        skip_within_page = 0;
    }

    parser.screen_mut().set_scrollback(0);
    let rows = render_screen_rows(parser.screen());
    collected.extend(rows);

    let result = collected.join("\n");
    parser.screen_mut().set_scrollback(original_offset);
    result
}

pub fn render_screen_rows(screen: &vt100::Screen) -> Vec<String> {
    let (rows, cols) = screen.size();
    let height = usize::from(rows);
    let mut lines = Vec::with_capacity(height);

    for row in 0..rows {
        lines.push(render_row(screen, row, cols));
    }

    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    pub fg: Color,
    pub bg: Color,
    pub modifiers: u8,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            modifiers: 0,
        }
    }
}

impl CellStyle {
    fn from_cell(cell: &vt100::Cell) -> Self {
        let mut modifiers = 0;
        if cell.bold() {
            modifiers |= MOD_BOLD;
        }
        if cell.italic() {
            modifiers |= MOD_ITALIC;
        }
        if cell.underline() {
            modifiers |= MOD_UNDERLINE;
        }
        if cell.inverse() {
            modifiers |= MOD_INVERSE;
        }

        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            modifiers,
        }
    }
}

fn render_row(screen: &vt100::Screen, row: u16, cols: u16) -> String {
    let mut out = String::new();
    let mut current_style: Option<CellStyle> = None;

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            if current_style != Some(CellStyle::default()) {
                write_sgr(&mut out, CellStyle::default());
                current_style = Some(CellStyle::default());
            }
            out.push(' ');
            continue;
        };

        if cell.is_wide_continuation() {
            continue;
        }

        let style = CellStyle::from_cell(cell);
        if current_style != Some(style) {
            write_sgr(&mut out, style);
            current_style = Some(style);
        }

        if cell.has_contents() {
            out.push_str(&cell.contents());
        } else {
            out.push(' ');
        }
    }

    out.push_str("\x1b[0m");
    out
}

fn write_sgr(out: &mut String, style: CellStyle) {
    out.push('\x1b');
    out.push('[');
    out.push('0');

    if style.modifiers & MOD_BOLD != 0 {
        out.push_str(";1");
    }
    if style.modifiers & MOD_ITALIC != 0 {
        out.push_str(";3");
    }
    if style.modifiers & MOD_UNDERLINE != 0 {
        out.push_str(";4");
    }
    if style.modifiers & MOD_INVERSE != 0 {
        out.push_str(";7");
    }

    write_color(out, style.fg, true);
    write_color(out, style.bg, false);

    out.push('m');
}

const MOD_BOLD: u8 = 0b0000_0001;
const MOD_ITALIC: u8 = 0b0000_0010;
const MOD_UNDERLINE: u8 = 0b0000_0100;
const MOD_INVERSE: u8 = 0b0000_1000;

fn write_color(out: &mut String, color: Color, foreground: bool) {
    match color {
        Color::Default => {}
        Color::Idx(index) => {
            if foreground {
                out.push_str(";38;5;");
            } else {
                out.push_str(";48;5;");
            }
            push_u8_decimal(out, index);
        }
        Color::Rgb(r, g, b) => {
            if foreground {
                out.push_str(";38;2;");
            } else {
                out.push_str(";48;2;");
            }
            push_u8_decimal(out, r);
            out.push(';');
            push_u8_decimal(out, g);
            out.push(';');
            push_u8_decimal(out, b);
        }
    }
}

fn push_u8_decimal(out: &mut String, value: u8) {
    if value >= 100 {
        out.push((b'0' + (value / 100)) as char);
        let remainder = value % 100;
        out.push((b'0' + (remainder / 10)) as char);
        out.push((b'0' + (remainder % 10)) as char);
        return;
    }

    if value >= 10 {
        out.push((b'0' + (value / 10)) as char);
        out.push((b'0' + (value % 10)) as char);
        return;
    }

    out.push((b'0' + value) as char);
}
