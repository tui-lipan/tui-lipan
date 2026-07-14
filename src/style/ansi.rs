//! Lightweight SGR-only ANSI escape parser.
//!
//! Parses CSI SGR sequences (`ESC [ … m`) into [`Span`]s so that any
//! [`Text`](crate::widgets::Text) widget can render ANSI-styled strings without
//! pulling in a full terminal emulator.
//!
//! Non-SGR sequences (cursor movement, screen clear, OSC hyperlinks, etc.) are
//! silently stripped. This is a read-only, one-shot parser - not a streaming
//! terminal.

use super::color::Color;
use super::text::{RowStylePolicy, Span};
use super::theme::Style;

/// Parse an ANSI-escaped string into a list of styled spans.
///
/// Adjacent runs with identical styles are coalesced. Plain text (no escapes)
/// produces a single span with the default style. Newlines are preserved inside
/// span content - the `Text` widget's layout pass handles line breaks.
pub fn parse_ansi(input: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    parse_ansi_into(input, &mut spans);
    spans
}

/// Parse an ANSI-escaped string, appending spans into an existing vector.
///
/// Useful when building up a `RichText` incrementally.
pub fn parse_ansi_into(input: &str, out: &mut Vec<Span>) {
    let mut style = Style::default();
    let mut text_buf = String::new();
    let mut chars = input.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if ch == '\x1B' {
            // Flush accumulated text before processing the escape.
            flush_span(&mut text_buf, style, out);

            // Look ahead for the escape type.
            match chars.peek() {
                Some((_, '[')) => {
                    // CSI sequence: ESC [
                    chars.next(); // consume '['
                    let (params, final_byte) = consume_csi(&mut chars);
                    if final_byte == b'm' {
                        apply_sgr(&params, &mut style);
                    }
                    // Non-SGR CSI sequences are silently discarded.
                }
                Some((_, ']')) => {
                    // OSC sequence: ESC ] … BEL/ST
                    chars.next(); // consume ']'
                    consume_osc(&mut chars);
                }
                Some((_, c)) if (*c as u32) < 0x40 => {
                    // Two-character escape: ESC Fp (where Fp is 0x30–0x3F)
                    // e.g. ESC 7, ESC 8 - consume and discard.
                    chars.next();
                }
                Some(_) => {
                    // ESC Fe/Fs (0x40–0x7E): two-character sequence like
                    // ESC N (SS2), ESC O (SS3), ESC M (RI), etc.
                    // Consume the character and discard the sequence.
                    chars.next();
                }
                None => {
                    // Bare ESC at end of string - nothing to consume.
                }
            }
        } else {
            text_buf.push(ch);
        }
    }

    // Flush any remaining text.
    flush_span(&mut text_buf, style, out);
}

/// Consume a CSI parameter string, returning (params_bytes, final_byte).
///
/// CSI format: `ESC [ <params> <final_byte>` where params are bytes in
/// `0x20..=0x3F` and final_byte is in `0x40..=0x7E`.
fn consume_csi(
    chars: &mut std::iter::Peekable<impl Iterator<Item = (usize, char)>>,
) -> (Vec<u8>, u8) {
    let mut params = Vec::new();
    let mut final_byte: u8 = 0;

    #[allow(clippy::while_let_on_iterator)]
    while let Some((_, ch)) = chars.next() {
        let b = ch as u32 as u8;
        if (0x40..=0x7E).contains(&b) {
            final_byte = b;
            break;
        }
        // Parameter and intermediate bytes (0x20..=0x3F).
        params.push(b);
    }

    (params, final_byte)
}

/// Consume an OSC sequence until BEL (0x07) or ST (ESC \\).
fn consume_osc(chars: &mut std::iter::Peekable<impl Iterator<Item = (usize, char)>>) {
    while let Some((_, ch)) = chars.next() {
        if ch == '\x07' {
            // BEL terminates OSC.
            return;
        }
        if ch == '\x1B' {
            // Check for ST: ESC \\
            if let Some((_, '\\')) = chars.peek() {
                chars.next();
                return;
            }
        }
    }
}

/// Apply SGR parameters to the current style.
fn apply_sgr(params: &[u8], style: &mut Style) {
    // Split params on ';' (0x3B) or ':' (0x3A) for colon-separated variants.
    let tokens = split_sgr_params(params);

    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        match token {
            0 | -1 => {
                // 0 = reset, -1 represents empty (e.g. ESC[m is same as ESC[0m)
                *style = Style::default();
            }
            1 => {
                style.bold = Some(true);
            }
            2 => {
                style.dim = Some(true);
            }
            3 => {
                style.italic = Some(true);
            }
            4 => {
                style.underline = Some(true);
            }
            7 => {
                style.reverse = Some(true);
            }
            9 => {
                style.strikethrough = Some(true);
            }
            22 => {
                style.bold = Some(false);
                style.dim = Some(false);
            }
            23 => {
                style.italic = Some(false);
            }
            24 => {
                style.underline = Some(false);
            }
            27 => {
                style.reverse = Some(false);
            }
            29 => {
                style.strikethrough = Some(false);
            }
            30..=37 => {
                style.fg = Some(ansi_fg_color(token).into());
            }
            39 => {
                style.fg = None;
            }
            40..=47 => {
                style.bg = Some(ansi_bg_color(token).into());
            }
            49 => {
                style.bg = None;
            }
            90..=97 => {
                // Bright/bold foreground (aixterm).
                style.fg = Some(ansi_bright_fg_color(token).into());
            }
            100..=107 => {
                // Bright/bold background (aixterm).
                style.bg = Some(ansi_bright_bg_color(token).into());
            }
            38 => {
                // Extended foreground: 38;5;N or 38;2;R;G;B
                if let Some(color) = parse_extended_color(&tokens[i + 1..]) {
                    style.fg = Some(color.into());
                    // Skip consumed sub-params.
                    match tokens.get(i + 1) {
                        Some(&5) => i += 2, // 38;5;N → skip 5 and N
                        Some(&2) => i += 4, // 38;2;R;G;B → skip 2, R, G, B
                        _ => i += 1,
                    }
                }
            }
            48 => {
                // Extended background: 48;5;N or 48;2;R;G;B
                if let Some(color) = parse_extended_color(&tokens[i + 1..]) {
                    style.bg = Some(color.into());
                    match tokens.get(i + 1) {
                        Some(&5) => i += 2,
                        Some(&2) => i += 4,
                        _ => i += 1,
                    }
                }
            }
            _ => {
                // Unknown SGR code - skip silently.
            }
        }
        i += 1;
    }
}

/// Split raw CSI parameter bytes into signed integer tokens.
///
/// Handles both semicolon-separated (`38;5;42`) and colon-separated
/// (`38:5:42`) variants. An empty parameter string (e.g. `ESC[m`) is
/// treated as a single reset code (0).
fn split_sgr_params(params: &[u8]) -> Vec<i32> {
    if params.is_empty() {
        return vec![0]; // ESC[m is equivalent to ESC[0m
    }

    let mut tokens = Vec::new();
    let mut current = Vec::new();

    for &b in params {
        if b == b';' || b == b':' {
            tokens.push(parse_param_token(&current));
            current.clear();
        } else {
            current.push(b);
        }
    }
    tokens.push(parse_param_token(&current));
    tokens
}

/// Parse a single parameter token (ASCII digits) into an i32.
///
/// Empty tokens map to -1 (sentinel for "missing", treated as 0/reset by
/// callers that care).
fn parse_param_token(digits: &[u8]) -> i32 {
    if digits.is_empty() {
        return -1;
    }
    let mut val: i32 = 0;
    for &d in digits {
        if d.is_ascii_digit() {
            val = val * 10 + (d - b'0') as i32;
        } else {
            return -1; // non-digit in param - treat as unknown
        }
    }
    val
}

/// Parse extended color after `38` or `48`.
///
/// Expects the sub-slice starting right after the `38`/`48` token.
fn parse_extended_color(tokens: &[i32]) -> Option<Color> {
    match tokens.first()? {
        5 => {
            // 256-color: 5;N
            let n = *tokens.get(1)?;
            if (0..=255).contains(&n) {
                Some(Color::Indexed(n as u8))
            } else {
                None
            }
        }
        2 => {
            // Truecolor: 2;R;G;B
            let r = (*tokens.get(1)?).clamp(0, 255) as u8;
            let g = (*tokens.get(2)?).clamp(0, 255) as u8;
            let b = (*tokens.get(3)?).clamp(0, 255) as u8;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Map ANSI standard foreground code (30–37) to `Color`.
fn ansi_fg_color(code: i32) -> Color {
    match code {
        30 => Color::Black,
        31 => Color::Red,
        32 => Color::Green,
        33 => Color::Yellow,
        34 => Color::Blue,
        35 => Color::Magenta,
        36 => Color::Cyan,
        37 => Color::Gray, // "White" in ANSI is mapped to Gray (light gray)
        _ => Color::Reset,
    }
}

/// Map ANSI standard background code (40–47) to `Color`.
fn ansi_bg_color(code: i32) -> Color {
    match code {
        40 => Color::Black,
        41 => Color::Red,
        42 => Color::Green,
        43 => Color::Yellow,
        44 => Color::Blue,
        45 => Color::Magenta,
        46 => Color::Cyan,
        47 => Color::Gray,
        _ => Color::Reset,
    }
}

/// Map aixterm bright foreground code (90–97) to `Color`.
fn ansi_bright_fg_color(code: i32) -> Color {
    match code {
        90 => Color::DarkGray,
        91 => Color::LightRed,
        92 => Color::LightGreen,
        93 => Color::LightYellow,
        94 => Color::LightBlue,
        95 => Color::LightMagenta,
        96 => Color::LightCyan,
        97 => Color::White,
        _ => Color::Reset,
    }
}

/// Map aixterm bright background code (100–107) to `Color`.
fn ansi_bright_bg_color(code: i32) -> Color {
    match code {
        100 => Color::DarkGray,
        101 => Color::LightRed,
        102 => Color::LightGreen,
        103 => Color::LightYellow,
        104 => Color::LightBlue,
        105 => Color::LightMagenta,
        106 => Color::LightCyan,
        107 => Color::White,
        _ => Color::Reset,
    }
}

/// Write foreground SGR codes for `color` into `out`.
pub fn write_fg_sgr(out: &mut String, color: Color) {
    match color {
        Color::Reset | Color::Transparent | Color::Backdrop => {}
        Color::Black => out.push_str("\x1b[30m"),
        Color::Red => out.push_str("\x1b[31m"),
        Color::Green => out.push_str("\x1b[32m"),
        Color::Yellow => out.push_str("\x1b[33m"),
        Color::Blue => out.push_str("\x1b[34m"),
        Color::Magenta => out.push_str("\x1b[35m"),
        Color::Cyan => out.push_str("\x1b[36m"),
        Color::Gray => out.push_str("\x1b[37m"),
        Color::DarkGray => out.push_str("\x1b[90m"),
        Color::LightRed => out.push_str("\x1b[91m"),
        Color::LightGreen => out.push_str("\x1b[92m"),
        Color::LightYellow => out.push_str("\x1b[93m"),
        Color::LightBlue => out.push_str("\x1b[94m"),
        Color::LightMagenta => out.push_str("\x1b[95m"),
        Color::LightCyan => out.push_str("\x1b[96m"),
        Color::White => out.push_str("\x1b[97m"),
        Color::Indexed(i) => {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[38;5;{i}m");
        }
        Color::Rgb(r, g, b) => {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
        }
    }
}

/// Write background SGR codes for `color` into `out`.
pub fn write_bg_sgr(out: &mut String, color: Color) {
    match color {
        Color::Reset | Color::Transparent | Color::Backdrop => {}
        Color::Black => out.push_str("\x1b[40m"),
        Color::Red => out.push_str("\x1b[41m"),
        Color::Green => out.push_str("\x1b[42m"),
        Color::Yellow => out.push_str("\x1b[43m"),
        Color::Blue => out.push_str("\x1b[44m"),
        Color::Magenta => out.push_str("\x1b[45m"),
        Color::Cyan => out.push_str("\x1b[46m"),
        Color::Gray => out.push_str("\x1b[47m"),
        Color::DarkGray => out.push_str("\x1b[100m"),
        Color::LightRed => out.push_str("\x1b[101m"),
        Color::LightGreen => out.push_str("\x1b[102m"),
        Color::LightYellow => out.push_str("\x1b[103m"),
        Color::LightBlue => out.push_str("\x1b[104m"),
        Color::LightMagenta => out.push_str("\x1b[105m"),
        Color::LightCyan => out.push_str("\x1b[106m"),
        Color::White => out.push_str("\x1b[107m"),
        Color::Indexed(i) => {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[48;5;{i}m");
        }
        Color::Rgb(r, g, b) => {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[48;2;{r};{g};{b}m");
        }
    }
}

/// Write underline-color SGR codes for `color` into `out`.
pub fn write_underline_color_sgr(out: &mut String, color: Color) {
    match color {
        Color::Reset | Color::Transparent | Color::Backdrop => {}
        Color::Indexed(i) => {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[58;5;{i}m");
        }
        Color::Rgb(r, g, b) => {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[58;2;{r};{g};{b}m");
        }
        _ => {}
    }
}

/// Write text-modifier SGR codes into `out`.
pub fn write_text_modifiers_sgr(
    out: &mut String,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    reverse: bool,
    strikethrough: bool,
) {
    if bold {
        out.push_str("\x1b[1m");
    }
    if dim {
        out.push_str("\x1b[2m");
    }
    if italic {
        out.push_str("\x1b[3m");
    }
    if underline {
        out.push_str("\x1b[4m");
    }
    if reverse {
        out.push_str("\x1b[7m");
    }
    if strikethrough {
        out.push_str("\x1b[9m");
    }
}

/// Foreground, background, modifiers, and underline color for [`write_cell_style_sgr`].
pub struct CellStyleSgr {
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Underline color.
    pub underline_color: Color,
    /// Bold modifier.
    pub bold: bool,
    /// Dim modifier.
    pub dim: bool,
    /// Italic modifier.
    pub italic: bool,
    /// Underline modifier.
    pub underline: bool,
    /// Reverse-video modifier.
    pub reverse: bool,
    /// Strikethrough modifier.
    pub strikethrough: bool,
}

/// Reset SGR and apply foreground, background, modifiers, and underline color.
pub fn write_cell_style_sgr(out: &mut String, style: CellStyleSgr) {
    out.push_str("\x1b[0m");
    write_fg_sgr(out, style.fg);
    write_bg_sgr(out, style.bg);
    write_text_modifiers_sgr(
        out,
        style.bold,
        style.dim,
        style.italic,
        style.underline,
        style.reverse,
        style.strikethrough,
    );
    write_underline_color_sgr(out, style.underline_color);
}

/// Emit a span if there is accumulated text, coalescing with the previous span
/// when styles match.
///
/// On coalesce, the new text is appended directly into the previous span's
/// `Arc<str>` content, avoiding an intermediate `String` → `Arc<str>` round-trip
/// for the new fragment.
fn flush_span(text_buf: &mut String, style: Style, out: &mut Vec<Span>) {
    if text_buf.is_empty() {
        return;
    }

    // Coalesce with previous span if styles match - append text_buf into the
    // existing Arc<str> without converting text_buf to Arc<str> first.
    if let Some(last) = out.last_mut()
        && last.style == style
    {
        let combined: std::sync::Arc<str> = format!("{}{}", last.content, text_buf).into();
        last.content = combined;
        text_buf.clear();
        return;
    }

    let content: std::sync::Arc<str> = text_buf.as_str().into();
    text_buf.clear();
    out.push(Span {
        content,
        style,
        row_style_policy: RowStylePolicy::Full,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Paint;

    fn p(color: Color) -> Option<Paint> {
        Some(Paint::Solid(color))
    }

    #[test]
    fn empty_string_produces_no_spans() {
        let spans = parse_ansi("");
        assert!(spans.is_empty());
    }

    #[test]
    fn plain_text_produces_single_default_span() {
        let spans = parse_ansi("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "hello world");
        assert_eq!(spans[0].style, Style::default());
    }

    #[test]
    fn red_then_reset() {
        let spans = parse_ansi("\x1b[31mred\x1b[0m plain");
        assert_eq!(spans.len(), 2);
        assert_eq!(&*spans[0].content, "red");
        assert_eq!(spans[0].style.fg, p(Color::Red));
        assert_eq!(&*spans[1].content, " plain");
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn bold_underline_rgb_fg() {
        let spans = parse_ansi("\x1b[1;4;38;2;255;128;0mX");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "X");
        assert_eq!(spans[0].style.bold, Some(true));
        assert_eq!(spans[0].style.underline, Some(true));
        assert_eq!(spans[0].style.fg, p(Color::Rgb(255, 128, 0)));
    }

    #[test]
    fn indexed_256_color() {
        let spans = parse_ansi("\x1b[38;5;42mX");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, p(Color::Indexed(42)));
    }

    #[test]
    fn bright_foreground() {
        let spans = parse_ansi("\x1b[92mX");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, p(Color::LightGreen));
    }

    #[test]
    fn bright_background() {
        let spans = parse_ansi("\x1b[104mX");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.bg, p(Color::LightBlue));
    }

    #[test]
    fn malformed_esc_stripped_text_preserved() {
        // Truncated ESC at end of string.
        let spans = parse_ansi("hello\x1b");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "hello");
    }

    #[test]
    fn unknown_csi_stripped() {
        // ESC[2J is a screen-clear sequence, not SGR - should be stripped.
        let spans = parse_ansi("\x1b[2Jhello");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "hello");
        assert_eq!(spans[0].style, Style::default());
    }

    #[test]
    fn colon_separated_extended_color() {
        // Colon-separated variant: 38:5:42 should parse identically to 38;5;42
        let spans = parse_ansi("\x1b[38:5:42mX");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, p(Color::Indexed(42)));
    }

    #[test]
    fn colon_separated_rgb() {
        let spans = parse_ansi("\x1b[38:2:255:128:0mX");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, p(Color::Rgb(255, 128, 0)));
    }

    #[test]
    fn reset_code_0() {
        let spans = parse_ansi("\x1b[1;31mbold red\x1b[0mdefault");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.bold, Some(true));
        assert_eq!(spans[0].style.fg, p(Color::Red));
        assert_eq!(spans[1].style.bold, None);
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn empty_sgr_is_reset() {
        // ESC[m is equivalent to ESC[0m
        let spans = parse_ansi("\x1b[31mred\x1b[mdefault");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.fg, p(Color::Red));
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn italic_and_strikethrough() {
        let spans = parse_ansi("\x1b[3;9mtext\x1b[23;29m");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "text");
        assert_eq!(spans[0].style.italic, Some(true));
        assert_eq!(spans[0].style.strikethrough, Some(true));
    }

    #[test]
    fn dim_and_bold_off_with_22() {
        let spans = parse_ansi("\x1b[1;2mbold dim\x1b[22mafter");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.bold, Some(true));
        assert_eq!(spans[0].style.dim, Some(true));
        // Code 22 turns off both bold and dim.
        assert_eq!(spans[1].style.bold, Some(false));
        assert_eq!(spans[1].style.dim, Some(false));
    }

    #[test]
    fn reverse_on_off() {
        let spans = parse_ansi("\x1b[7mrev\x1b[27mnormal");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.reverse, Some(true));
        assert_eq!(spans[1].style.reverse, Some(false));
    }

    #[test]
    fn fg_default_39() {
        let spans = parse_ansi("\x1b[31mred\x1b[39mno fg");
        assert_eq!(spans[0].style.fg, p(Color::Red));
        assert_eq!(spans[1].style.fg, None);
    }

    #[test]
    fn bg_default_49() {
        let spans = parse_ansi("\x1b[44mblue bg\x1b[49mno bg");
        assert_eq!(spans[0].style.bg, p(Color::Blue));
        assert_eq!(spans[1].style.bg, None);
    }

    #[test]
    fn osc_sequence_stripped() {
        // OSC title sequence: ESC]0;titleBEL
        let spans = parse_ansi("\x1b]0;window title\x07visible");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "visible");
    }

    #[test]
    fn osc_sequence_st_stripped() {
        // OSC with ST terminator: ESC]8;;urlST
        let spans = parse_ansi("\x1b]8;;https://example.com\x1b\\visible");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "visible");
    }

    #[test]
    fn coalesces_adjacent_same_style() {
        let spans = parse_ansi("\x1b[31ma\x1b[31mb");
        // Both have the same style, so they should be coalesced.
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "ab");
    }

    #[test]
    fn newlines_preserved_in_span_content() {
        let spans = parse_ansi("line1\nline2");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "line1\nline2");
    }

    #[test]
    fn parse_ansi_into_appends() {
        let mut spans = vec![Span::new("existing")];
        parse_ansi_into("\x1b[32mgreen", &mut spans);
        assert_eq!(spans.len(), 2);
        assert_eq!(&*spans[0].content, "existing");
        assert_eq!(spans[1].style.fg, p(Color::Green));
    }

    #[test]
    fn bg_extended_256() {
        let spans = parse_ansi("\x1b[48;5;200mX");
        assert_eq!(spans[0].style.bg, p(Color::Indexed(200)));
    }

    #[test]
    fn bg_extended_rgb() {
        let spans = parse_ansi("\x1b[48;2;10;20;30mX");
        assert_eq!(spans[0].style.bg, p(Color::Rgb(10, 20, 30)));
    }

    #[test]
    fn standard_16_colors_fg() {
        let codes: &[(i32, Color)] = &[
            (30, Color::Black),
            (31, Color::Red),
            (32, Color::Green),
            (33, Color::Yellow),
            (34, Color::Blue),
            (35, Color::Magenta),
            (36, Color::Cyan),
            (37, Color::Gray),
        ];
        for &(code, ref expected) in codes {
            let spans = parse_ansi(&format!("\x1b[{}mX", code));
            assert_eq!(spans[0].style.fg, p(*expected));
        }
    }

    #[test]
    fn standard_16_colors_bg() {
        let codes: &[(i32, Color)] = &[
            (40, Color::Black),
            (41, Color::Red),
            (42, Color::Green),
            (43, Color::Yellow),
            (44, Color::Blue),
            (45, Color::Magenta),
            (46, Color::Cyan),
            (47, Color::Gray),
        ];
        for &(code, ref expected) in codes {
            let spans = parse_ansi(&format!("\x1b[{}mX", code));
            assert_eq!(spans[0].style.bg, p(*expected));
        }
    }

    #[test]
    fn two_char_escape_sequence() {
        // ESC N (SS2) - should be silently consumed.
        let spans = parse_ansi("\x1bNhello");
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].content, "hello");
    }

    #[test]
    fn complex_real_world_output() {
        // Simulates something like: bold red "error:" then reset, then plain text.
        let input = "\x1b[1;31merror:\x1b[0m file not found";
        let spans = parse_ansi(input);
        assert_eq!(spans.len(), 2);
        assert_eq!(&*spans[0].content, "error:");
        assert_eq!(spans[0].style.bold, Some(true));
        assert_eq!(spans[0].style.fg, p(Color::Red));
        assert_eq!(&*spans[1].content, " file not found");
        assert_eq!(spans[1].style, Style::default());
    }
}
