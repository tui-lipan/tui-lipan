//! Display-column helpers for styled text.
//!
//! These helpers deliberately operate on [`crate::style::Span`] rather than on
//! backend text types. Keeping the operations here means layout, widgets, and
//! applications agree about what a terminal column is: control characters
//! occupy no columns and characters are accumulated with `unicode-width`, the
//! same convention used by rendered terminal grids.

use std::ops::Range;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::style::{Span, Style};
use crate::utils::text::push_span_slice_styled;

/// Return the number of terminal columns occupied by `text`.
///
/// Unicode control characters, including tabs, occupy zero columns.
pub(crate) fn display_width(text: &str) -> usize {
    text.graphemes(true).map(grapheme_width).sum()
}

/// Return the display column at the UTF-8 byte offset `byte_offset`.
pub(crate) fn display_column(text: &str, byte_offset: usize) -> usize {
    let mut byte_offset = byte_offset.min(text.len());
    while byte_offset > 0 && !text.is_char_boundary(byte_offset) {
        byte_offset -= 1;
    }
    display_width(&text[..byte_offset])
}

/// Return the UTF-8 byte offset at display column `column`.
///
/// When a column lands inside a wide character, its starting byte is returned.
#[cfg(any(feature = "terminal", test))]
pub(crate) fn byte_at_display_column(text: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }

    let mut current = 0usize;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let width = grapheme_width(grapheme);
        if current.saturating_add(width) > column {
            return byte;
        }
        current = current.saturating_add(width);
    }
    text.len()
}

/// Return the display width of all spans in `spans`.
pub(crate) fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|span| display_width(&span.content)).sum()
}

/// Concatenate span contents without their styles.
pub fn line_text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.content.as_ref()).collect()
}

/// Return the display width of a styled line.
pub fn line_width(spans: &[Span]) -> usize {
    spans_width(spans)
}

/// Convert a character column in styled text to a rendered display column.
///
/// Text-facing APIs commonly expose character columns, while rendered grids
/// use display columns. Wide characters therefore advance by two display
/// columns and combining/control characters by zero.
pub fn char_col_to_display_col(spans: &[Span], char_col: usize) -> usize {
    let text = line_text(spans);
    let byte = text
        .char_indices()
        .nth(char_col)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len());
    display_width(&text[..byte])
}

/// Convert a rendered display column in styled text to a character column.
///
/// A display column inside a wide character maps to the character before that
/// cell, preserving a valid text boundary.
pub fn display_col_to_char_col(spans: &[Span], display_col: usize) -> usize {
    let text = line_text(spans);
    let mut column = 0usize;
    let mut chars = 0usize;
    for grapheme in text.graphemes(true) {
        let width = display_width(grapheme);
        if column.saturating_add(width) > display_col {
            break;
        }
        column = column.saturating_add(width);
        chars += grapheme.chars().count();
    }
    chars
}

/// Split styled spans at embedded newlines, preserving styles and row policies.
pub(crate) fn split_spans_on_newlines(spans: &[Span]) -> Vec<Vec<Span>> {
    let mut lines = Vec::new();
    let mut current = Vec::new();

    for span in spans {
        for (index, part) in span.content.split('\n').enumerate() {
            if index > 0 {
                lines.push(std::mem::take(&mut current));
            }
            if !part.is_empty() {
                current.push(Span {
                    content: Arc::from(part),
                    style: span.style,
                    row_style_policy: span.row_style_policy,
                });
            }
        }
    }
    lines.push(current);
    lines
}

/// Truncate styled spans to `max_width` columns, appending an ellipsis when
/// truncation is needed.
pub(crate) fn truncate_spans(spans: &[Span], max_width: usize) -> Vec<Span> {
    if max_width == 0 {
        return Vec::new();
    }
    if spans_width(spans) <= max_width {
        return spans.to_vec();
    }

    let ellipsis = "…";
    let target = max_width.saturating_sub(display_width(ellipsis));
    let (mut prefix, _) = take_prefix_spans(spans, target);
    let style = prefix
        .last()
        .map(|span| span.style)
        .or_else(|| spans.last().map(|span| span.style))
        .unwrap_or_default();
    let row_style_policy = prefix
        .last()
        .map(|span| span.row_style_policy)
        .or_else(|| spans.last().map(|span| span.row_style_policy))
        .unwrap_or_default();
    prefix.push(Span {
        content: Arc::from(ellipsis),
        style,
        row_style_policy,
    });
    prefix
}

/// Truncate styled spans from the start so that at most `max_width` columns
/// remain visible.
pub(crate) fn truncate_spans_start(spans: &[Span], max_width: usize) -> Vec<Span> {
    if max_width == 0 {
        return Vec::new();
    }
    if spans_width(spans) <= max_width {
        return spans.to_vec();
    }

    let mut out = Vec::new();
    let mut remaining = max_width;
    for span in spans.iter().rev() {
        if remaining == 0 {
            break;
        }
        let width = display_width(&span.content);
        if width <= remaining {
            out.push(span.clone());
            remaining -= width;
            continue;
        }

        let start = start_at_tail_width(&span.content, remaining);
        if start < span.content.len() {
            out.push(slice_span(span, start..span.content.len()));
        }
        remaining = 0;
    }
    out.reverse();
    out
}

/// Split `spans` into a prefix that fits `max_width` columns and the suffix
/// after it. The source spans are cloned only as needed for the returned pair.
pub(crate) fn take_prefix_spans(spans: &[Span], max_width: usize) -> (Vec<Span>, Vec<Span>) {
    if max_width == 0 {
        return (Vec::new(), spans.to_vec());
    }

    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut used = 0usize;

    for (index, span) in spans.iter().enumerate() {
        if used >= max_width {
            suffix.extend(spans[index..].iter().cloned());
            break;
        }

        let width = display_width(&span.content);
        if used.saturating_add(width) <= max_width {
            prefix.push(span.clone());
            used = used.saturating_add(width);
            continue;
        }

        let available = max_width.saturating_sub(used);
        let split = prefix_end_at_width(&span.content, available);
        if split > 0 {
            prefix.push(slice_span(span, 0..split));
        }
        if split < span.content.len() {
            suffix.push(slice_span(span, split..span.content.len()));
        }
        suffix.extend(spans[index + 1..].iter().cloned());
        break;
    }

    (prefix, suffix)
}

/// Return the part of a styled line intersecting the half-open display-column
/// range `start..end`.
pub fn slice_columns(spans: &[Span], start: usize, end: usize) -> Vec<Span> {
    if start >= end {
        return Vec::new();
    }
    let start = crate::utils::text::cursor_at_column(spans, start);
    let end = end_cursor_at_column(spans, end);
    crate::utils::text::collect_span_range(spans, start, end)
}

/// Apply style patches to half-open display-column ranges.
pub fn restyle_columns(spans: &[Span], ranges: &[(Range<usize>, Style)]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut column = 0usize;
    for span in spans {
        let mut run_start = 0usize;
        let mut run_style = None;
        for (byte, grapheme) in span.content.grapheme_indices(true) {
            let width = grapheme_width(grapheme);
            let end_column = column.saturating_add(width);
            let mut style = span.style;
            for (range, patch) in ranges {
                if range.start < end_column.max(column.saturating_add(1)) && range.end > column {
                    style = style.patch(*patch);
                }
            }
            if let Some(previous) = run_style
                && previous != style
            {
                push_span_slice_styled(&mut out, span, run_start..byte, previous);
                run_start = byte;
            }
            run_style = Some(style);
            column = end_column;
        }
        if let Some(style) = run_style {
            push_span_slice_styled(&mut out, span, run_start..span.content.len(), style);
        }
    }
    out
}

/// Return a copy of `spans` with `overlay` painted over the display columns starting at `column`.
///
/// Unlike [`insert_at_column`], nothing shifts: the overlay covers exactly as many columns as it
/// is wide, so every column after it keeps the position it had. That is what a label anchored to
/// content underneath needs - an insert at the right-hand edge of a fixed-width line, such as a
/// terminal row, is pushed straight off it. A wide character the overlay half-covers is dropped
/// and its remaining cell padded with a space, since half a glyph cannot be drawn. The line is
/// extended only when the overlay itself reaches past its end.
pub fn overwrite_at_column(spans: &[Span], column: usize, overlay: Span) -> Vec<Span> {
    let overlay_width = display_width(overlay.content.as_ref());
    if overlay_width == 0 {
        return spans.to_vec();
    }
    let total = spans_width(spans);

    let (mut out, _) = take_prefix_spans(spans, column);
    let prefix_width = spans_width(&out);
    if prefix_width < column {
        out.push(Span::new(" ".repeat(column - prefix_width)));
    }
    push_span_slice_styled(&mut out, &overlay, 0..overlay.content.len(), overlay.style);

    let written = column.saturating_add(overlay_width);
    let tail_width = total.saturating_sub(written);
    if tail_width == 0 {
        return out;
    }
    let mut suffix = slice_columns(spans, written, total);
    if spans_width(&suffix) > tail_width {
        suffix = truncate_spans_start(&suffix, tail_width);
    }
    let suffix_width = spans_width(&suffix);
    if suffix_width < tail_width {
        out.push(Span::new(" ".repeat(tail_width - suffix_width)));
    }
    for span in &suffix {
        push_span_slice_styled(&mut out, span, 0..span.content.len(), span.style);
    }
    out
}

/// Return a copy of `spans` with `insert` placed at display column `column`.
pub fn insert_at_column(spans: &[Span], column: usize, insert: Span) -> Vec<Span> {
    let cursor = crate::utils::text::cursor_at_column(spans, column);
    let end = crate::utils::text::cursor_at_column(spans, usize::MAX);
    let mut out = crate::utils::text::collect_span_range(spans, Default::default(), cursor);
    push_span_slice_styled(&mut out, &insert, 0..insert.content.len(), insert.style);
    // `collect_span_range` already merged its own runs, so only the first span can
    // still merge with the tail of `out`. Re-pushing every span through the merge
    // path instead would reallocate the accumulated string once per span.
    let mut suffix = crate::utils::text::collect_span_range(spans, cursor, end).into_iter();
    if let Some(first) = suffix.next() {
        push_span_slice_styled(&mut out, &first, 0..first.content.len(), first.style);
    }
    out.extend(suffix);
    out
}

fn grapheme_width(grapheme: &str) -> usize {
    if grapheme.chars().all(char::is_control) {
        0
    } else {
        UnicodeWidthStr::width(grapheme)
    }
}

fn end_cursor_at_column(spans: &[Span], column: usize) -> crate::utils::text::SpanCursor {
    let mut cursor = crate::utils::text::SpanCursor::default();
    let mut used = 0usize;
    while let Some((next, _, width)) = crate::utils::text::next_char_cursor(spans, cursor) {
        if used >= column && width > 0 {
            break;
        }
        used = used.saturating_add(width);
        cursor = next;
    }
    cursor
}

fn prefix_end_at_width(text: &str, max_width: usize) -> usize {
    let mut used = 0usize;
    let mut end = 0usize;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let width = grapheme_width(grapheme);
        if used.saturating_add(width) > max_width {
            break;
        }
        used = used.saturating_add(width);
        end = byte + grapheme.len();
    }
    end
}

fn start_at_tail_width(text: &str, max_width: usize) -> usize {
    if max_width == 0 {
        return text.len();
    }
    let mut used = 0usize;
    let mut start = text.len();
    for (byte, grapheme) in text.grapheme_indices(true).rev() {
        let width = grapheme_width(grapheme);
        if used.saturating_add(width) > max_width {
            break;
        }
        used = used.saturating_add(width);
        start = byte;
    }
    start
}

fn slice_span(span: &Span, range: std::ops::Range<usize>) -> Span {
    Span {
        content: if range.start == 0 && range.end == span.content.len() {
            span.content.clone()
        } else {
            Arc::from(&span.content[range])
        },
        style: span.style,
        row_style_policy: span.row_style_policy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, RowStylePolicy};

    #[test]
    fn display_width_skips_controls_and_counts_wide_text() {
        assert_eq!(display_width("a\0你\u{7}b"), 4);
        assert_eq!(display_width("e\u{301}"), 1);
        assert_eq!(display_width("👩‍💻"), 2);
    }

    #[test]
    fn display_column_and_byte_mapping_use_terminal_columns() {
        assert_eq!(display_column("a你b", 4), 3);
        assert_eq!(display_column("a你b", 2), 1);
        assert_eq!(byte_at_display_column("a你b", 1), 1);
        assert_eq!(byte_at_display_column("a你b", 2), 1);
        assert_eq!(byte_at_display_column("a你b", 3), 4);
        assert_eq!(byte_at_display_column("a👩‍💻b", 2), 1);
        assert_eq!(byte_at_display_column("a👩‍💻b", 3), 12);
    }

    #[test]
    fn split_preserves_row_style_policy() {
        let span = Span::new("a\nb").row_style_policy(RowStylePolicy::Disabled);
        let lines = split_spans_on_newlines(&[span]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].row_style_policy, RowStylePolicy::Disabled);
        assert_eq!(lines[1][0].content.as_ref(), "b");
    }

    #[test]
    fn truncation_does_not_split_wide_graphemes() {
        let spans = [Span::new("a你b").fg(Color::Red)];
        let truncated = truncate_spans(&spans, 2);
        assert_eq!(truncated[0].content.as_ref(), "a");
        assert_eq!(truncated[1].content.as_ref(), "…");
    }

    #[test]
    fn insertion_uses_display_columns() {
        let spans = insert_at_column(&[Span::new("a你b")], 3, Span::new("!"));
        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "a你!b"
        );
    }

    #[test]
    fn slicing_includes_a_wide_grapheme_when_the_range_touches_one_cell() {
        let sliced = slice_columns(&[Span::new("a你b")], 1, 2);
        assert_eq!(line_text(&sliced), "你");
    }

    #[test]
    fn restyling_uses_display_columns() {
        let styled = restyle_columns(
            &[Span::new("a你b")],
            &[(1..3, Style::new().bg(Color::Blue))],
        );
        assert_eq!(styled.len(), 3);
        assert_eq!(styled[1].content.as_ref(), "你");
        assert_eq!(styled[1].style.bg, Some(Color::Blue.into()));
    }

    #[test]
    fn character_and_display_column_bridges_handle_wide_and_zero_width_chars() {
        let spans = [Span::new("a界e\u{301}b")];
        assert_eq!(char_col_to_display_col(&spans, 2), 3);
        assert_eq!(char_col_to_display_col(&spans, 4), 4);
        assert_eq!(display_col_to_char_col(&spans, 2), 1);
        assert_eq!(display_col_to_char_col(&spans, 3), 2);

        let joined = [Span::new("a👩‍💻b")];
        assert_eq!(char_col_to_display_col(&joined, 4), 3);
        assert_eq!(display_col_to_char_col(&joined, 2), 1);
        assert_eq!(display_col_to_char_col(&joined, 3), 4);
    }

    #[test]
    fn whole_span_restyle_reuses_content_arc() {
        let source = Span::new("whole");
        let styled = restyle_columns(
            std::slice::from_ref(&source),
            &[(0..5, Style::new().fg(Color::Blue))],
        );
        assert!(Arc::ptr_eq(&source.content, &styled[0].content));
    }

    #[test]
    fn overwrite_keeps_line_width_and_pads_half_covered_wide_chars() {
        let spans = [Span::new("abcdef")];
        let painted = overwrite_at_column(&spans, 2, Span::new("XY").style(Style::new().bold()));
        assert_eq!(line_text(&painted), "abXYef");
        assert_eq!(line_width(&painted), 6);

        // Half of a wide character cannot be drawn: the overlay takes the cell it covers and the
        // other one becomes a space, so every later column stays where it was.
        let wide = [Span::new("a界界b")];
        let over_wide = overwrite_at_column(&wide, 2, Span::new("X"));
        assert_eq!(line_text(&over_wide), "a X界b");
        assert_eq!(line_width(&wide), line_width(&over_wide));

        // Past the end the line grows, since there is nothing left to cover.
        let past = overwrite_at_column(&spans, 8, Span::new("Z"));
        assert_eq!(line_text(&past), "abcdef  Z");
    }

    #[test]
    fn insertion_merges_adjacent_equal_styles_and_respects_hard_wide_cut() {
        let style = Style::new().fg(Color::Red);
        let spans = [Span::new("a界b").style(style)];
        let inserted = insert_at_column(&spans, 2, Span::new("!").style(style));
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].content.as_ref(), "a!界b");
    }
}
