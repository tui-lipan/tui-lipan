//! Shared text utilities for editing logic.

use std::borrow::Cow;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::style::Span;
use std::ops::Range;
use std::sync::Arc;

/// Custom sentinel range: base codepoint, end exclusive, display widths, labels.
type CustomSentinelTuple = (u32, u32, Vec<usize>, Vec<Arc<str>>);

/// Combined sentinel metadata for inline placeholder expansion.
///
/// Covers two independent PUA ranges:
/// - **image** sentinels at `U+E000+` - uniform placeholder width (e.g. `"[Image]"`).
/// - **custom** sentinels at `U+F000+` - per-entry label and width.
///
/// Use [`SentinelInfo::width_of`] in hot-path character-width code.
#[derive(Clone, Debug, Default)]
pub(crate) struct SentinelInfo {
    /// `Some((base, end_exclusive, uniform_width))` when image sentinels are active.
    pub image: Option<(u32, u32, usize)>,
    /// `Some((base, end_exclusive, widths, labels))` when custom sentinels are active.
    /// `widths[i]` is the display width of `labels[i]`.
    pub custom: Option<CustomSentinelTuple>,
}

impl SentinelInfo {
    /// Return the display width of `ch` if it is a sentinel character, else `None`.
    #[inline]
    pub fn width_of(&self, ch: char) -> Option<usize> {
        let cp = ch as u32;
        if let Some((base, end, w)) = self.image
            && cp >= base
            && cp < end
        {
            return Some(w);
        }
        if let Some((base, end, ref widths, _)) = self.custom
            && cp >= base
            && cp < end
        {
            let idx = (cp - base) as usize;
            return Some(widths.get(idx).copied().unwrap_or(0));
        }
        None
    }

    /// Return `true` if `ch` is any sentinel character in this info.
    #[inline]
    pub fn is_sentinel(&self, ch: char) -> bool {
        self.width_of(ch).is_some()
    }
}

#[inline]
pub(crate) fn is_wrap_break(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '/' | '|' | '-' | '_' | '.' | ':')
}

/// Returns the visual width of a character, accounting for sentinel placeholders.
pub(crate) fn char_visual_width(ch: char, sentinel: Option<&SentinelInfo>) -> usize {
    if let Some(si) = sentinel
        && let Some(w) = si.width_of(ch)
    {
        return w;
    }
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

/// Returns the visual width of a string, accounting for sentinel placeholders.
pub(crate) fn str_visual_width(s: &str, sentinel: Option<&SentinelInfo>) -> usize {
    match sentinel {
        None => UnicodeWidthStr::width(s),
        Some(si) => s.chars().map(|ch| char_visual_width(ch, Some(si))).sum(),
    }
}

/// Returns the visual width of `s`, treating `\t` as advancing to the next
/// `tab_stop` boundary starting from `start_col`.
///
/// When `tab_stop == 0`, tabs contribute their unicode-width value (i.e. 0).
pub(crate) fn str_visual_width_with_tabs(
    s: &str,
    sentinel: Option<&SentinelInfo>,
    start_col: usize,
    tab_stop: usize,
) -> usize {
    if tab_stop == 0 || !s.contains('\t') {
        return str_visual_width(s, sentinel);
    }
    let mut col = start_col;
    for ch in s.chars() {
        let w = if ch == '\t' {
            tab_stop - (col % tab_stop)
        } else {
            char_visual_width(ch, sentinel)
        };
        col += w;
    }
    col - start_col
}

/// A zero-byte visual insertion on a logical line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VirtualTextInsertion {
    /// Byte offset relative to the start of the logical line.
    pub(crate) anchor: usize,
    /// Visual width in terminal columns.
    pub(crate) width: usize,
    /// Ordering among virtual segments sharing an anchor.
    pub(crate) priority: u16,
    /// Stable source order tie-breaker.
    pub(crate) order: usize,
}

/// Visual width of `prefix` plus every inline virtual insertion anchored inside it.
///
/// Insertions at `prefix.len()` are included, so a cursor at the anchor renders
/// after the inserted virtual text while the real byte at that anchor remains
/// inert for editing and selection.
pub(crate) fn visual_col_with_virtual(
    prefix: &str,
    line_start_col: usize,
    tab_stop: usize,
    sentinel: Option<&SentinelInfo>,
    insertions: &[VirtualTextInsertion],
) -> usize {
    let buffer_width = str_visual_width_with_tabs(prefix, sentinel, line_start_col, tab_stop);
    let prefix_len = prefix.len();
    let virtual_width = insertions
        .iter()
        .take_while(|insertion| insertion.anchor <= prefix_len)
        .map(|insertion| insertion.width)
        .fold(0usize, usize::saturating_add);
    buffer_width.saturating_add(virtual_width)
}

/// Expand `\t` characters in `s` into spaces, aligning each tab to the next
/// `tab_stop` boundary starting from `start_col`.
///
/// Returns the original string unmodified when `tab_stop == 0` or no tabs are
/// present. Other characters (including sentinels) are passed through as-is.
pub(crate) fn expand_tabs<'a>(s: &'a str, start_col: usize, tab_stop: usize) -> Cow<'a, str> {
    if tab_stop == 0 || !s.contains('\t') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut col = start_col;
    for ch in s.chars() {
        if ch == '\t' {
            let w = tab_stop - (col % tab_stop);
            for _ in 0..w {
                out.push(' ');
            }
            col += w;
        } else {
            out.push(ch);
            col += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    Cow::Owned(out)
}

/// Replace sentinel characters in `text` with human-readable text.
///
/// - Image sentinels (`U+E000+`) are replaced using `image_placeholder` (supports `X` substitution).
/// - Custom sentinels (`U+F000+`) are replaced with their per-entry labels from `sentinel.custom`.
///
/// Returns the original text unmodified when `sentinel` is `None` or contains no sentinel chars.
pub(crate) fn replace_sentinels<'a>(
    text: &'a str,
    sentinel: Option<&SentinelInfo>,
    image_placeholder: &str,
) -> Cow<'a, str> {
    let Some(si) = sentinel else {
        return Cow::Borrowed(text);
    };
    let has_any = text.chars().any(|c| si.is_sentinel(c));
    if !has_any {
        return Cow::Borrowed(text);
    }
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        if let Some((base, end, _, ref labels)) = si.custom
            && cp >= base
            && cp < end
        {
            let idx = (cp - base) as usize;
            if let Some(label) = labels.get(idx) {
                result.push_str(label);
                continue;
            }
            // fallback: skip
        } else if let Some((base, end, _)) = si.image
            && cp >= base
            && cp < end
        {
            let index = (cp - base) as usize + 1;
            if image_placeholder.contains('X') {
                result.push_str(&image_placeholder.replace('X', index.to_string().as_str()));
            } else {
                result.push_str(image_placeholder);
            }
            continue;
        }
        result.push(ch);
    }
    Cow::Owned(result)
}

/// Check if a character is a word character (alphanumeric or underscore).
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Find the byte position at the start of the previous character.
pub(crate) fn prev_char_boundary(text: &str, cursor: usize) -> usize {
    let mut pos = cursor.min(text.len());
    if pos == 0 {
        return 0;
    }
    pos -= 1;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Find the byte position at the start of the next character.
pub(crate) fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = clamp_cursor(text, cursor);
    if cursor >= text.len() {
        return text.len();
    }
    let c = text[cursor..]
        .chars()
        .next()
        .expect("cursor < text.len() is checked above; slice is non-empty");
    cursor + c.len_utf8()
}

/// Get the character at the given byte index.
pub(crate) fn char_at(text: &str, idx: usize) -> char {
    text[idx..]
        .chars()
        .next()
        .expect("caller must pass a valid UTF-8 char boundary")
}

/// Find the byte position at the start of the previous word.
pub(crate) fn word_boundary_left(text: &str, cursor: usize) -> usize {
    let mut cur = cursor;

    // Skip whitespace.
    while cur > 0 {
        let prev = prev_char_boundary(text, cur);
        let c = char_at(text, prev);
        if !c.is_whitespace() {
            break;
        }
        cur = prev;
    }

    // Skip word characters.
    let mut moved_word = false;
    while cur > 0 {
        let prev = prev_char_boundary(text, cur);
        let c = char_at(text, prev);
        if is_word_char(c) {
            moved_word = true;
            cur = prev;
        } else {
            break;
        }
    }

    // If we didn't traverse a word, skip a punctuation run.
    if !moved_word {
        while cur > 0 {
            let prev = prev_char_boundary(text, cur);
            let c = char_at(text, prev);
            if c.is_whitespace() {
                break;
            }
            cur = prev;
        }
    }

    cur
}

/// Find the byte position at the end of the next word.
pub(crate) fn word_boundary_right(text: &str, cursor: usize) -> usize {
    let mut cur = cursor;
    let len = text.len();

    // Skip whitespace.
    while cur < len {
        let c = char_at(text, cur);
        if !c.is_whitespace() {
            break;
        }
        cur = next_char_boundary(text, cur);
    }

    // Skip word characters.
    let start = cur;
    while cur < len {
        let c = char_at(text, cur);
        if is_word_char(c) {
            cur = next_char_boundary(text, cur);
        } else {
            break;
        }
    }

    // If we didn't traverse a word, skip a punctuation run.
    if cur == start {
        while cur < len {
            let c = char_at(text, cur);
            if c.is_whitespace() {
                break;
            }
            cur = next_char_boundary(text, cur);
        }
    }

    cur
}

/// Calculates the byte offset of a column in a single line.
pub(crate) fn byte_at_col(line: &str, col: usize) -> usize {
    byte_at_col_sentinel(line, col, None)
}

/// Like [`byte_at_col`] but accounts for sentinel placeholder expansion.
pub(crate) fn byte_at_col_sentinel(
    line: &str,
    col: usize,
    sentinel: Option<&SentinelInfo>,
) -> usize {
    byte_at_col_sentinel_tabs(line, col, sentinel, 0)
}

/// Like [`byte_at_col_sentinel`] but treats `\t` as advancing to the next
/// `tab_stop` column (measured from the start of `line`).
pub(crate) fn byte_at_col_sentinel_tabs(
    line: &str,
    col: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> usize {
    if col == 0 {
        return 0;
    }

    let mut w = 0usize;
    for (i, ch) in line.char_indices() {
        let cw = if ch == '\t' && tab_stop > 0 {
            tab_stop - (w % tab_stop)
        } else {
            char_visual_width(ch, sentinel)
        };
        if w.saturating_add(cw) > col {
            return i;
        }
        w = w.saturating_add(cw);
    }
    line.len()
}

/// Like [`byte_at_col_sentinel_tabs`] but subtracts zero-byte virtual insertions.
/// A column that lands inside a virtual segment maps to that segment's anchor.
pub(crate) fn byte_at_col_sentinel_tabs_virtual(
    line: &str,
    col: usize,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
    insertions: &[VirtualTextInsertion],
) -> usize {
    fn consume_insertions_at(
        line_len: usize,
        insertions: &[VirtualTextInsertion],
        insertion_idx: &mut usize,
        anchor: usize,
        col: usize,
        visual_col: &mut usize,
    ) -> Option<usize> {
        while let Some(insertion) = insertions.get(*insertion_idx) {
            if insertion.anchor != anchor {
                break;
            }
            let end = visual_col.saturating_add(insertion.width);
            if col < end {
                return Some(anchor.min(line_len));
            }
            *visual_col = end;
            *insertion_idx = (*insertion_idx).saturating_add(1);
        }
        None
    }

    let mut insertion_idx = 0usize;
    let mut visual_col = 0usize;

    if let Some(anchor) = consume_insertions_at(
        line.len(),
        insertions,
        &mut insertion_idx,
        0,
        col,
        &mut visual_col,
    ) {
        return anchor;
    }
    if col == 0 {
        return 0;
    }

    for (i, ch) in line.char_indices() {
        while let Some(insertion) = insertions.get(insertion_idx) {
            if insertion.anchor < i {
                insertion_idx = insertion_idx.saturating_add(1);
            } else {
                break;
            }
        }
        if let Some(anchor) = consume_insertions_at(
            line.len(),
            insertions,
            &mut insertion_idx,
            i,
            col,
            &mut visual_col,
        ) {
            return anchor;
        }
        let cw = if ch == '\t' && tab_stop > 0 {
            tab_stop - (visual_col % tab_stop)
        } else {
            char_visual_width(ch, sentinel)
        };
        if visual_col.saturating_add(cw) > col {
            return i;
        }
        visual_col = visual_col.saturating_add(cw);
    }

    while let Some(insertion) = insertions.get(insertion_idx) {
        if insertion.anchor > line.len() {
            break;
        }
        let end = visual_col.saturating_add(insertion.width);
        if col < end {
            return insertion.anchor.min(line.len());
        }
        visual_col = end;
        insertion_idx = insertion_idx.saturating_add(1);
    }

    line.len()
}

pub(crate) fn clamp_cursor(line: &str, cursor: usize) -> usize {
    let mut c = cursor.min(line.len());
    while c > 0 && !line.is_char_boundary(c) {
        c -= 1;
    }
    c
}

pub(crate) fn end_at_width(line: &str, start: usize, width: usize) -> usize {
    end_at_width_sentinel(line, start, width, None)
}

/// Like [`end_at_width`] but accounts for sentinel placeholder expansion.
pub(crate) fn end_at_width_sentinel(
    line: &str,
    start: usize,
    width: usize,
    sentinel: Option<&SentinelInfo>,
) -> usize {
    end_at_width_sentinel_tabs(line, start, width, sentinel, 0, 0)
}

/// Like [`end_at_width_sentinel`] but treats `\t` as advancing to the next
/// `tab_stop` column from `start_col`.
pub(crate) fn end_at_width_sentinel_tabs(
    line: &str,
    start: usize,
    width: usize,
    sentinel: Option<&SentinelInfo>,
    start_col: usize,
    tab_stop: usize,
) -> usize {
    if width == 0 || start >= line.len() {
        return start.min(line.len());
    }

    let mut w = 0usize;
    let mut col = start_col;
    let mut end = start;

    for (i, ch) in line[start..].char_indices() {
        let cw = if ch == '\t' && tab_stop > 0 {
            tab_stop - (col % tab_stop)
        } else {
            char_visual_width(ch, sentinel)
        };
        if w.saturating_add(cw) > width {
            break;
        }
        w = w.saturating_add(cw);
        col = col.saturating_add(cw);
        end = start + i + ch.len_utf8();
    }

    end
}

pub(crate) fn input_viewport_start(line: &str, cursor: usize, width: u16) -> usize {
    let width = width as usize;
    if width == 0 {
        return 0;
    }

    let cursor = clamp_cursor(line, cursor);
    let cursor_col = UnicodeWidthStr::width(&line[..cursor]);

    // Keep cursor visible and reserve 1 cell for the caret.
    let start_col = cursor_col.saturating_add(1).saturating_sub(width);
    byte_at_col(line, start_col)
}

pub(crate) fn viewport(line: &str, cursor: usize, width: u16) -> (usize, usize, u16) {
    let width = width as usize;
    if width == 0 {
        return (0, 0, 0);
    }

    let cursor = clamp_cursor(line, cursor);
    let cursor_col = UnicodeWidthStr::width(&line[..cursor]);

    // Keep cursor visible and reserve 1 cell for the caret.
    let start_col = cursor_col.saturating_add(1).saturating_sub(width);
    let start = byte_at_col(line, start_col);
    let end = end_at_width(line, start, width);

    let cursor_x = UnicodeWidthStr::width(&line[start..cursor])
        .min(width.saturating_sub(1))
        .min(u16::MAX as usize) as u16;

    (start, end, cursor_x)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpanCursor {
    pub(crate) span_idx: usize,
    pub(crate) byte_idx: usize,
}

pub(crate) fn wrap_spans_for_budgets(
    spans: &[Span],
    first_budget: u16,
    cont_budget: u16,
) -> Vec<Vec<Span>> {
    if spans.is_empty() {
        return vec![Vec::new()];
    }

    let mut start = SpanCursor::default();
    if next_char_cursor(spans, start).is_none() {
        return vec![Vec::new()];
    }

    let mut out = Vec::new();

    if first_budget == 0 {
        out.push(Vec::new());
    } else {
        let mut end = take_chunk_end(spans, start, first_budget as usize);
        if end == start {
            end = next_char_cursor(spans, start)
                .map(|(next, _, _)| next)
                .unwrap_or(start);
        }
        out.push(collect_span_range(spans, start, end));
        start = end;
    }

    let cont_budget = cont_budget.max(1) as usize;
    while next_char_cursor(spans, start).is_some() {
        let mut end = take_chunk_end(spans, start, cont_budget);
        if end == start {
            end = next_char_cursor(spans, start)
                .map(|(next, _, _)| next)
                .unwrap_or(start);
        }
        out.push(collect_span_range(spans, start, end));
        start = end;
    }

    out
}

/// Count how many wrapped lines [`wrap_spans_for_budgets`] would produce, without
/// materializing (and cloning) the wrapped span vectors.
///
/// The control flow mirrors [`wrap_spans_for_budgets`] exactly, so the returned
/// count is identical to `wrap_spans_for_budgets(spans, first_budget, cont_budget).len()`.
/// This exists for the measure path, which only needs visual-line *counts* and
/// must not pay the per-line `Vec<Span>`/`Arc<str>` clone+alloc cost.
pub(crate) fn count_wrapped_lines_for_budgets(
    spans: &[Span],
    first_budget: u16,
    cont_budget: u16,
) -> usize {
    if spans.is_empty() {
        return 1;
    }

    let mut start = SpanCursor::default();
    if next_char_cursor(spans, start).is_none() {
        return 1;
    }

    let mut count = 0usize;

    if first_budget == 0 {
        count += 1;
    } else {
        let mut end = take_chunk_end(spans, start, first_budget as usize);
        if end == start {
            end = next_char_cursor(spans, start)
                .map(|(next, _, _)| next)
                .unwrap_or(start);
        }
        count += 1;
        start = end;
    }

    let cont_budget = cont_budget.max(1) as usize;
    while next_char_cursor(spans, start).is_some() {
        let mut end = take_chunk_end(spans, start, cont_budget);
        if end == start {
            end = next_char_cursor(spans, start)
                .map(|(next, _, _)| next)
                .unwrap_or(start);
        }
        count += 1;
        start = end;
    }

    count
}

pub(crate) fn take_chunk_end(spans: &[Span], start: SpanCursor, max_width: usize) -> SpanCursor {
    if max_width == 0 {
        return start;
    }

    let mut used = 0usize;
    let mut cursor = start;
    let mut last_break = None;
    let mut overflowed = false;

    while let Some((next, ch, cw)) = next_char_cursor(spans, cursor) {
        if used + cw > max_width {
            overflowed = true;
            break;
        }
        used += cw;
        cursor = next;
        if is_wrap_break(ch) {
            last_break = Some(cursor);
        }
    }

    if cursor == start {
        return start;
    }

    if !overflowed {
        return cursor;
    }

    last_break.unwrap_or(cursor)
}

pub(crate) fn next_char_cursor(
    spans: &[Span],
    cursor: SpanCursor,
) -> Option<(SpanCursor, char, usize)> {
    let mut span_idx = cursor.span_idx;
    let mut byte_idx = cursor.byte_idx;

    while let Some(span) = spans.get(span_idx) {
        let content = span.content.as_ref();
        if byte_idx >= content.len() {
            span_idx += 1;
            byte_idx = 0;
            continue;
        }

        let mut chars = content[byte_idx..].chars();
        let ch = chars.next()?;
        let next = SpanCursor {
            span_idx,
            byte_idx: byte_idx + ch.len_utf8(),
        };
        return Some((next, ch, UnicodeWidthChar::width(ch).unwrap_or(0)));
    }

    None
}

pub(crate) fn collect_span_range(spans: &[Span], start: SpanCursor, end: SpanCursor) -> Vec<Span> {
    if start == end {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut span_idx = start.span_idx;

    while span_idx < spans.len() {
        if span_idx > end.span_idx || (span_idx == end.span_idx && end.byte_idx == 0) {
            break;
        }

        let span = &spans[span_idx];
        let content = span.content.as_ref();
        let range = if span_idx == start.span_idx && span_idx == end.span_idx {
            start.byte_idx..end.byte_idx
        } else if span_idx == start.span_idx {
            start.byte_idx..content.len()
        } else if span_idx == end.span_idx {
            0..end.byte_idx
        } else {
            0..content.len()
        };

        if !range.is_empty() {
            push_span_slice(&mut out, span, range);
        }

        if span_idx == end.span_idx {
            break;
        }
        span_idx += 1;
    }

    out
}

pub(crate) fn push_span_slice(out: &mut Vec<Span>, span: &Span, range: Range<usize>) {
    if range.is_empty() {
        return;
    }

    let content = span.content.as_ref();
    let fragment = if range.start == 0 && range.end == content.len() {
        span.content.clone()
    } else {
        Arc::<str>::from(&content[range])
    };

    if let Some(last) = out.last_mut()
        && last.style == span.style
    {
        let mut merged = String::with_capacity(last.content.len() + fragment.len());
        merged.push_str(last.content.as_ref());
        merged.push_str(fragment.as_ref());
        last.content = Arc::<str>::from(merged);
        return;
    }

    out.push(Span {
        content: fragment,
        style: span.style,
        allow_row_style: span.allow_row_style,
    });
}

#[cfg(test)]
mod tests {
    use super::{
        VirtualTextInsertion, byte_at_col, byte_at_col_sentinel_tabs_virtual, clamp_cursor,
        count_wrapped_lines_for_budgets, end_at_width, expand_tabs, input_viewport_start,
        is_wrap_break, next_char_boundary, str_visual_width_with_tabs, viewport,
        visual_col_with_virtual, word_boundary_left, word_boundary_right, wrap_spans_for_budgets,
    };
    use crate::style::{Color, Span};

    #[test]
    fn clamp_cursor_snaps_to_char_boundary() {
        // "he|llo" (byte 2)
        assert_eq!(clamp_cursor("hello", 2), 2);

        // "你好" (bytes: 0..3, 3..6)
        // Try clamping at byte 1 (inside '你') -> should be 0
        assert_eq!(clamp_cursor("你好", 1), 0);
        // Try clamping at byte 2 (inside '你') -> should be 0
        assert_eq!(clamp_cursor("你好", 2), 0);
        // Try clamping at byte 4 (inside '好') -> should be 3
        assert_eq!(clamp_cursor("你好", 4), 3);

        // Beyond end
        assert_eq!(clamp_cursor("hi", 10), 2);
    }

    #[test]
    fn next_char_boundary_snaps_from_inside_character() {
        let text = "części";

        assert_eq!(next_char_boundary(text, 4), 6);
        assert_eq!(next_char_boundary(text, 5), 6);
        assert_eq!(next_char_boundary(text, text.len() + 1), text.len());
    }

    #[test]
    fn byte_at_col_handles_width() {
        assert_eq!(byte_at_col("", 0), 0);
        assert_eq!(byte_at_col("abc", 2), 2); // 'c' starts at 2

        // "你" is width 2. Bytes: [0, 1, 2]
        // col 0 -> 0
        // col 1 -> 0 (still inside first char)
        // col 2 -> 3 (start of next char)
        assert_eq!(byte_at_col("你好", 0), 0);
        assert_eq!(byte_at_col("你好", 1), 0);
        assert_eq!(byte_at_col("你好", 2), 3);
    }

    #[test]
    fn end_at_width_respects_limit() {
        // "hello"
        assert_eq!(end_at_width("hello", 0, 3), 3); // "hel"

        // "你好世界" (2 cols each)
        assert_eq!(end_at_width("你好世界", 0, 3), 3); // "你" (width 2). adding '好' (2) would be 4 > 3. So stop at 3 (end of '你').
        assert_eq!(end_at_width("你好世界", 0, 4), 6); // "你好" (width 4) -> bytes 0..6
    }

    #[test]
    fn input_viewport_scrolls_to_keep_cursor_visible() {
        let text = "abcdefgh";
        // Viewport width 4.

        // Cursor at 0: "abcd"
        // cursor_col = 0. start_col = 0+1-4 < 0 -> 0.
        assert_eq!(input_viewport_start(text, 0, 4), 0);

        // Cursor at 3 ('d'): "abcd"
        // cursor_col = 3. start_col = 3+1-4 = 0.
        assert_eq!(input_viewport_start(text, 3, 4), 0);

        // Cursor at 4 ('e'):
        // cursor_col = 4. start_col = 4+1-4 = 1.
        // byte_at_col(1) -> 1 ('b')
        // visible: "bcde" (cursor at end)
        assert_eq!(input_viewport_start(text, 4, 4), 1);
    }

    #[test]
    fn viewport_calculates_window_around_cursor() {
        let text = "abcdefgh";
        // Width 4

        // Cursor 0 -> "abcd" (0..4)
        let (start, end, cx) = viewport(text, 0, 4);
        assert_eq!(start, 0);
        assert_eq!(end, 4);
        assert_eq!(cx, 0);

        // Cursor 4 ('e') -> start 1 ("bcde")
        let (start, end, cx) = viewport(text, 4, 4);
        assert_eq!(start, 1);
        assert_eq!(end, 5);
        assert_eq!(cx, 3);

        // Cursor 8 (end) -> "fgh" (5..8)
        let (start, end, cx) = viewport(text, 8, 4);
        assert_eq!(start, 5);
        assert_eq!(end, 8);
        assert_eq!(cx, 3);
    }

    #[test]
    fn word_boundary_left_with_punctuation() {
        let text = "hello--world";
        // Cursor at byte 7 = start of "world".
        // 1) Skip whitespace: none → stays at 7.
        // 2) Skip word chars backward: char at byte 6 is '-', not a word char → moved_word=false.
        // 3) Punctuation fallback: skip all non-whitespace chars backward → goes past "--" and
        //    "hello", landing at 0.
        assert_eq!(word_boundary_left(text, 7), 0);

        // Cursor at byte 5 = start of "--".
        // 1) No whitespace.
        // 2) Skip word chars backward: 'o' at byte 4 is a word char → skips all of "hello",
        //    moved_word=true.
        // 3) Skipped (moved_word=true).
        // Result: 0 (start of "hello").
        assert_eq!(word_boundary_left(text, 5), 0);
    }

    #[test]
    fn word_boundary_right_with_punctuation() {
        let text = "hello--world";
        // Cursor at byte 0.
        // 1) Skip whitespace: 'h' is not whitespace → stays at 0.
        // 2) Skip word chars: "hello" (bytes 0..5) → cur=5.
        // 3) cur(5) != start(0) → no punctuation fallback.
        // Result: 5 (right after "hello", at the first '-').
        assert_eq!(word_boundary_right(text, 0), 5);

        // From byte 5 (start of "--").
        // 1) No whitespace.
        // 2) '-' is not a word char → cur stays at 5.
        // 3) Punctuation fallback: skip "--world" (all non-whitespace) → cur=12.
        assert_eq!(word_boundary_right(text, 5), text.len());
    }

    #[test]
    fn byte_at_col_with_wide_chars() {
        // "a你好b"
        //  'a' : byte 0,   width 1  (visual col 0)
        //  '你': bytes 1-3, width 2  (visual cols 1-2)
        //  '好': bytes 4-6, width 2  (visual cols 3-4)
        //  'b' : byte 7,   width 1  (visual col 5)
        let text = "a你好b";
        assert_eq!(byte_at_col(text, 0), 0); // start
        assert_eq!(byte_at_col(text, 1), 1); // start of '你'
        assert_eq!(byte_at_col(text, 2), 1); // still inside '你' (col 2 is its second half)
        assert_eq!(byte_at_col(text, 3), 4); // start of '好'
        assert_eq!(byte_at_col(text, 5), 7); // start of 'b'
        assert_eq!(byte_at_col(text, 6), 8); // past end
    }

    #[test]
    fn str_visual_width_with_tabs_aligns_to_tab_stop() {
        assert_eq!(str_visual_width_with_tabs("\t", None, 0, 4), 4);
        assert_eq!(str_visual_width_with_tabs("ab\t", None, 0, 4), 4);
        assert_eq!(str_visual_width_with_tabs("abcd\t", None, 0, 4), 8);
        assert_eq!(str_visual_width_with_tabs("\t", None, 2, 4), 2);
        assert_eq!(str_visual_width_with_tabs("plain", None, 0, 4), 5);
    }

    #[test]
    fn visual_col_with_virtual_counts_anchor_at_cursor() {
        let insertions = [VirtualTextInsertion {
            anchor: 1,
            width: 3,
            priority: 0,
            order: 0,
        }];

        assert_eq!(visual_col_with_virtual("", 0, 4, None, &insertions), 0);
        assert_eq!(visual_col_with_virtual("a", 0, 4, None, &insertions), 4);
        assert_eq!(visual_col_with_virtual("ab", 0, 4, None, &insertions), 5);
    }

    #[test]
    fn byte_at_col_with_virtual_clamps_inside_segment_to_anchor() {
        let insertions = [VirtualTextInsertion {
            anchor: 1,
            width: 3,
            priority: 0,
            order: 0,
        }];

        assert_eq!(
            byte_at_col_sentinel_tabs_virtual("ab", 0, None, 4, &insertions),
            0
        );
        assert_eq!(
            byte_at_col_sentinel_tabs_virtual("ab", 1, None, 4, &insertions),
            1
        );
        assert_eq!(
            byte_at_col_sentinel_tabs_virtual("ab", 2, None, 4, &insertions),
            1
        );
        assert_eq!(
            byte_at_col_sentinel_tabs_virtual("ab", 3, None, 4, &insertions),
            1
        );
        assert_eq!(
            byte_at_col_sentinel_tabs_virtual("ab", 4, None, 4, &insertions),
            1
        );
        assert_eq!(
            byte_at_col_sentinel_tabs_virtual("ab", 5, None, 4, &insertions),
            2
        );
    }

    #[test]
    fn expand_tabs_emits_spaces_to_next_stop() {
        assert_eq!(expand_tabs("\t", 0, 4).as_ref(), "    ");
        assert_eq!(expand_tabs("ab\tc", 0, 4).as_ref(), "ab  c");
        assert_eq!(expand_tabs("abc\tx", 0, 4).as_ref(), "abc x");
        assert_eq!(expand_tabs("plain", 0, 4).as_ref(), "plain");
        assert_eq!(expand_tabs("\t", 2, 4).as_ref(), "  ");
        // tab_stop=0 disables expansion (zero-width tab, original semantics).
        assert_eq!(expand_tabs("a\tb", 0, 0).as_ref(), "a\tb");
    }

    #[test]
    fn end_at_width_stops_before_wide_char() {
        // "ab你" → 'a'(1) + 'b'(1) + '你'(2) = 4 visual cols total
        let text = "ab你";

        // width=3: 'a'(1)+'b'(1)=2, adding '你'(2) → 4 > 3 → stop before '你'.
        // end = byte 2 (end of 'b').
        assert_eq!(end_at_width(text, 0, 3), 2);

        // width=4: 'a'(1)+'b'(1)+'你'(2)=4 ≤ 4 → include '你'.
        // end = byte 2 + 3 = 5 (entire string).
        assert_eq!(end_at_width(text, 0, 4), 5);

        // width=2: 'a'(1)+'b'(1)=2 ≤ 2 → include 'b'. '你'(2) → 4 > 2 → stop.
        assert_eq!(end_at_width(text, 0, 2), 2);

        // width=1: 'a'(1)=1 ≤ 1 → include 'a'. 'b'(1) → 2 > 1 → stop.
        assert_eq!(end_at_width(text, 0, 1), 1);
    }

    #[test]
    fn wrap_breaks_include_common_code_punctuation() {
        for ch in [' ', '/', '|', '-', '_', '.', ':', '\t'] {
            assert!(is_wrap_break(ch), "{ch:?} should be a wrap break");
        }
        assert!(!is_wrap_break('x'));
    }

    #[test]
    fn wrap_spans_preserves_style_when_slicing() {
        let spans = vec![
            Span::new("abc").fg(Color::Red),
            Span::new("def").fg(Color::Blue),
        ];
        let wrapped = wrap_spans_for_budgets(&spans, 4, 4);

        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0].len(), 2);
        assert_eq!(wrapped[0][0].content.as_ref(), "abc");
        assert_eq!(wrapped[0][0].style, spans[0].style);
        assert_eq!(wrapped[0][1].content.as_ref(), "d");
        assert_eq!(wrapped[0][1].style, spans[1].style);
        assert_eq!(wrapped[1].len(), 1);
        assert_eq!(wrapped[1][0].content.as_ref(), "ef");
        assert_eq!(wrapped[1][0].style, spans[1].style);
    }

    #[test]
    fn wrap_spans_keeps_zero_first_budget_behavior() {
        let spans = vec![Span::new("ab cd")];
        let wrapped = wrap_spans_for_budgets(&spans, 0, 3);

        assert_eq!(wrapped.len(), 3);
        assert!(wrapped[0].is_empty());
        assert_eq!(wrapped[1][0].content.as_ref(), "ab ");
        assert_eq!(wrapped[2][0].content.as_ref(), "cd");
    }

    #[test]
    fn wrap_spans_breaks_on_newline_when_overflowing_later() {
        let spans = vec![Span::new("ab\ncd")];
        let wrapped = wrap_spans_for_budgets(&spans, 2, 2);

        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0][0].content.as_ref(), "ab\n");
        assert_eq!(wrapped[1][0].content.as_ref(), "cd");
    }

    #[test]
    fn wrap_spans_forces_progress_for_unbreakable_wide_chars() {
        let spans = vec![Span::new("你a")];
        let wrapped = wrap_spans_for_budgets(&spans, 1, 1);

        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0][0].content.as_ref(), "你");
        assert_eq!(wrapped[1][0].content.as_ref(), "a");
    }

    #[test]
    fn count_wrapped_lines_matches_wrap_len_across_inputs_and_budgets() {
        let cases: Vec<Vec<Span>> = vec![
            vec![],
            vec![Span::new("")],
            vec![Span::new("short")],
            vec![Span::new("ab cd ef gh ij")],
            vec![Span::new("ab\ncd\nef")],
            vec![Span::new("你好世界 abc")],
            vec![Span::new("你a")],
            vec![
                Span::new("abc").fg(Color::Red),
                Span::new("def").fg(Color::Blue),
            ],
            vec![
                Span::new("a very "),
                Span::new("long line "),
                Span::new("of spans"),
            ],
        ];
        for spans in &cases {
            for first in [0u16, 1, 2, 3, 5, 8, 40] {
                for cont in [1u16, 2, 3, 5, 40] {
                    let expected = wrap_spans_for_budgets(spans, first, cont).len();
                    let actual = count_wrapped_lines_for_budgets(spans, first, cont);
                    assert_eq!(
                        actual, expected,
                        "count mismatch for first={first} cont={cont} spans={spans:?}"
                    );
                }
            }
        }
    }
}
