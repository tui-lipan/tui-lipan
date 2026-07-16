//! Text helper functions for cursor positioning and word selection.

use unicode_width::UnicodeWidthStr;

use crate::style::{Rect, ScrollbarVariant};
use crate::utils::text::{SentinelInfo, char_visual_width, str_visual_width_with_tabs};
use crate::widgets::{
    TextAreaVirtualText, TextAreaVisualLine, VirtualTextLayoutCtx,
    inline_virtual_insertions_for_line, layout_line_with_inline_virtual_text,
    text_area_cursor_reserve, text_area_total_gutter_width,
};

fn is_blank_line(text: &str) -> bool {
    text.trim().is_empty()
}

fn line_segments(text: &str) -> Vec<(usize, usize, usize)> {
    if text.is_empty() {
        return vec![(0, 0, 0)];
    }

    let mut lines = Vec::new();
    let mut start = 0usize;

    for segment in text.split_inclusive('\n') {
        let content_len = segment.strip_suffix('\n').map_or(segment.len(), str::len);
        let end_content = start.saturating_add(content_len);
        let end_total = start.saturating_add(segment.len());
        lines.push((start, end_content, end_total));
        start = end_total;
    }

    if text.ends_with('\n') {
        lines.push((text.len(), text.len(), text.len()));
    }

    lines
}

/// Expand a byte position to the surrounding paragraph bounded by blank lines.
pub(crate) fn paragraph_bounds_at_byte(text: &str, pos: usize) -> (usize, usize) {
    let lines = line_segments(text);
    let pos = pos.min(text.len());
    let mut line_idx = lines.len().saturating_sub(1);

    for (idx, &(_, _, end_total)) in lines.iter().enumerate() {
        if pos < end_total || (pos == end_total && idx + 1 == lines.len()) {
            line_idx = idx;
            break;
        }
    }

    let (line_start, line_end, _) = lines[line_idx];
    let line_text = &text[line_start..line_end];
    if is_blank_line(line_text) {
        return (line_start, line_end);
    }

    let mut start_idx = line_idx;
    while start_idx > 0 {
        let prev_idx = start_idx - 1;
        let (prev_start, prev_end, _) = lines[prev_idx];
        if is_blank_line(&text[prev_start..prev_end]) {
            break;
        }
        start_idx = prev_idx;
    }

    let mut end_idx = line_idx;
    while end_idx + 1 < lines.len() {
        let next_idx = end_idx + 1;
        let (next_start, next_end, _) = lines[next_idx];
        if is_blank_line(&text[next_start..next_end]) {
            break;
        }
        end_idx = next_idx;
    }

    (lines[start_idx].0, lines[end_idx].1)
}

/// Expand a visual line index to the surrounding paragraph bounded by blank lines.
pub(crate) fn paragraph_line_range<T: AsRef<str>>(
    lines: &[T],
    line_idx: usize,
) -> Option<(usize, usize)> {
    let line = lines.get(line_idx)?.as_ref();
    if is_blank_line(line) {
        return Some((line_idx, line_idx));
    }

    let mut start_idx = line_idx;
    while start_idx > 0 {
        if is_blank_line(lines[start_idx - 1].as_ref()) {
            break;
        }
        start_idx -= 1;
    }

    let mut end_idx = line_idx;
    while end_idx + 1 < lines.len() {
        if is_blank_line(lines[end_idx + 1].as_ref()) {
            break;
        }
        end_idx += 1;
    }

    Some((start_idx, end_idx))
}

/// Screen coordinates for [`textarea_cursor_from_coords`].
pub(crate) struct TextAreaCursorCoords {
    pub x: u16,
    pub y: u16,
    pub inner: Rect,
    pub clamp_to_inner: bool,
}

/// Layout and scroll configuration for [`textarea_cursor_from_coords`].
pub(crate) struct TextAreaCursorLayout {
    pub line_numbers: bool,
    pub min_line_number_width: u8,
    pub wrap: bool,
    pub scroll_offset: usize,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_over_border: bool,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub h_scrollbar_over_border: bool,
    pub max_line_width: usize,
    pub h_scroll_offset: usize,
    pub tab_stop: usize,
    pub gutter_col_width: u16,
    pub gutter_gap: u16,
    pub logical_lines_count: usize,
}

/// Inputs for [`textarea_cursor_from_coords`].
pub(crate) struct TextAreaCursorParams<'a> {
    pub value: &'a str,
    pub current_cursor: usize,
    pub coords: TextAreaCursorCoords,
    pub layout: TextAreaCursorLayout,
    pub read_only: bool,
    pub sentinel: Option<SentinelInfo>,
    pub visual_lines: Option<&'a [TextAreaVisualLine]>,
    pub virtual_texts: &'a [TextAreaVirtualText],
}

/// Compute the cursor byte position from screen coordinates for a text area.
pub(crate) fn textarea_cursor_from_coords(params: TextAreaCursorParams<'_>) -> usize {
    let TextAreaCursorParams {
        value,
        current_cursor,
        coords,
        layout,
        read_only,
        sentinel,
        visual_lines,
        virtual_texts,
    } = params;
    let TextAreaCursorCoords {
        x,
        y,
        inner,
        clamp_to_inner,
    } = coords;
    let TextAreaCursorLayout {
        line_numbers,
        min_line_number_width,
        wrap,
        scroll_offset,
        scrollbar,
        scrollbar_variant: _scrollbar_variant,
        scrollbar_gap,
        scrollbar_over_border,
        h_scrollbar,
        h_scrollbar_variant: _h_scrollbar_variant,
        h_scrollbar_over_border,
        max_line_width,
        h_scroll_offset,
        tab_stop,
        gutter_col_width,
        gutter_gap,
        logical_lines_count,
    } = layout;
    if inner.w == 0 || inner.h == 0 {
        return value.len();
    }

    let mut inner = inner;
    let mut x = x;
    let mut y = y;

    let gutter_width = text_area_total_gutter_width(
        logical_lines_count,
        line_numbers,
        min_line_number_width,
        gutter_col_width,
        gutter_gap,
    ) as usize;

    // Mirror the renderer: reserve caret buffer + scrollbar column.
    let scrollbar_cols = if scrollbar && !scrollbar_over_border {
        1u16.saturating_add(scrollbar_gap)
    } else {
        0
    };

    let content_width = inner
        .w
        .saturating_sub(gutter_width as u16)
        .saturating_sub(scrollbar_cols)
        .saturating_sub(text_area_cursor_reserve(wrap, read_only)) as usize;

    if content_width == 0 {
        return value.len();
    }

    let h_scrollbar_visible = h_scrollbar && !wrap && max_line_width > content_width;
    if h_scrollbar_visible && !h_scrollbar_over_border {
        let scrollbar_y = inner.y.saturating_add(inner.h.saturating_sub(1) as i16);
        if (y as i16) == scrollbar_y {
            return crate::utils::text::clamp_cursor(value, current_cursor);
        }
        inner.h = inner.h.saturating_sub(1);
        if inner.h == 0 {
            return crate::utils::text::clamp_cursor(value, current_cursor);
        }
    }

    if clamp_to_inner {
        let max_x = inner.x.saturating_add(inner.w.saturating_sub(1) as i16);
        let max_y = inner.y.saturating_add(inner.h.saturating_sub(1) as i16);
        x = (x as i16).clamp(inner.x, max_x) as u16;
        y = (y as i16).clamp(inner.y, max_y) as u16;
    }

    // Avoid mapping into the standalone scrollbar column.
    if scrollbar && !scrollbar_over_border {
        let content_right = inner
            .x
            .saturating_add(gutter_width as i16)
            .saturating_add(content_width.saturating_sub(1) as i16);
        if (x as i16) > content_right {
            x = content_right.max(0) as u16;
        }
    }

    let owned_visual_lines;
    let visual_lines = if let Some(lines) = visual_lines {
        lines
    } else {
        let mut lines = Vec::new();
        let mut current_byte_offset: usize = 0;

        for (line_num, line) in value.split('\n').enumerate() {
            let line_len = line.len();
            let line_start_abs = current_byte_offset;
            let line_end_abs = current_byte_offset.saturating_add(line_len);
            let insertions = inline_virtual_insertions_for_line(
                value,
                virtual_texts,
                line_start_abs,
                line_end_abs,
            );
            if !insertions.is_empty() {
                layout_line_with_inline_virtual_text(
                    line,
                    VirtualTextLayoutCtx {
                        line_start_abs,
                        line_num: line_num + 1,
                        wrap,
                        content_width,
                        sentinel: sentinel.as_ref(),
                        tab_stop,
                        insertions: &insertions,
                    },
                    &mut lines,
                );
            } else if !wrap
                || str_visual_width_with_tabs(line, sentinel.as_ref(), 0, tab_stop) <= content_width
            {
                lines.push(TextAreaVisualLine {
                    line_num: line_num + 1,
                    continuation: false,
                    start: current_byte_offset,
                    end: current_byte_offset + line_len,
                    visual_start_col: 0,
                    visual_end_col: str_visual_width_with_tabs(
                        line,
                        sentinel.as_ref(),
                        0,
                        tab_stop,
                    ),
                    starts_with_virtual_text: false,
                    ends_with_virtual_text: false,
                });
            } else {
                let mut start_idx = 0;
                let mut current_width = 0;
                let mut absolute_width = 0;
                let mut last_break_idx = 0;
                let mut last_break_width = 0;
                let mut is_first = true;

                for (idx, ch) in line.char_indices() {
                    let char_width = if ch == '\t' && tab_stop > 0 {
                        tab_stop - (absolute_width % tab_stop)
                    } else {
                        char_visual_width(ch, sentinel.as_ref())
                    };
                    let next_absolute_width = absolute_width.saturating_add(char_width);
                    if ch.is_whitespace() {
                        last_break_idx = idx + ch.len_utf8();
                        last_break_width = current_width + char_width;
                    }

                    if current_width + char_width > content_width {
                        let (break_idx, break_width) =
                            if last_break_idx > start_idx && last_break_width <= content_width {
                                (last_break_idx, last_break_width)
                            } else {
                                (idx, current_width)
                            };

                        lines.push(TextAreaVisualLine {
                            line_num: line_num + 1,
                            continuation: !is_first,
                            start: current_byte_offset + start_idx,
                            end: current_byte_offset + break_idx,
                            visual_start_col: str_visual_width_with_tabs(
                                &line[..start_idx],
                                sentinel.as_ref(),
                                0,
                                tab_stop,
                            ),
                            visual_end_col: str_visual_width_with_tabs(
                                &line[..break_idx],
                                sentinel.as_ref(),
                                0,
                                tab_stop,
                            ),
                            starts_with_virtual_text: false,
                            ends_with_virtual_text: false,
                        });
                        start_idx = break_idx;
                        current_width = current_width + char_width - break_width;
                        last_break_idx = start_idx;
                        last_break_width = 0;
                        is_first = false;
                    } else {
                        current_width += char_width;
                    }
                    absolute_width = next_absolute_width;
                }

                lines.push(TextAreaVisualLine {
                    line_num: line_num + 1,
                    continuation: !is_first,
                    start: current_byte_offset + start_idx,
                    end: current_byte_offset + line_len,
                    visual_start_col: str_visual_width_with_tabs(
                        &line[..start_idx],
                        sentinel.as_ref(),
                        0,
                        tab_stop,
                    ),
                    visual_end_col: str_visual_width_with_tabs(
                        line,
                        sentinel.as_ref(),
                        0,
                        tab_stop,
                    ),
                    starts_with_virtual_text: false,
                    ends_with_virtual_text: false,
                });
            }

            if wrap
                && !read_only
                && content_width > 0
                && lines.last().is_some_and(|last| {
                    last.line_num == line_num + 1
                        && last.end == line_end_abs
                        && last.visual_end_col.saturating_sub(last.visual_start_col)
                            == content_width
                })
            {
                let visual_col = lines.last().map(|last| last.visual_end_col).unwrap_or(0);
                lines.push(TextAreaVisualLine {
                    line_num: line_num + 1,
                    continuation: true,
                    start: line_end_abs,
                    end: line_end_abs,
                    visual_start_col: visual_col,
                    visual_end_col: visual_col,
                    starts_with_virtual_text: false,
                    ends_with_virtual_text: false,
                });
            }

            current_byte_offset += line_len + 1;
        }
        owned_visual_lines = lines;
        owned_visual_lines.as_slice()
    };

    if visual_lines.is_empty() {
        return value.len();
    }

    let h_scroll_start = if wrap { 0 } else { h_scroll_offset };
    let y_rel = (y as i32 - inner.y as i32).max(0) as usize;
    if !clamp_to_inner && (y as i32) < (inner.y as i32) {
        return 0;
    }

    let visual_idx = { scroll_offset.saturating_add(y_rel) };
    let vline = match visual_lines.get(visual_idx) {
        Some(v) => v,
        None => return value.len(),
    };

    let content_x = inner.x.saturating_add(gutter_width as i16);
    let col = if !clamp_to_inner && (x as i32) < (content_x as i32) {
        0
    } else {
        (x as i32 - content_x as i32).max(0) as usize + h_scroll_start
    };

    let logical_line_start = value[..vline.start].rfind('\n').map_or(0, |i| i + 1);
    let logical_line_end = value[logical_line_start..]
        .find('\n')
        .map(|i| logical_line_start + i)
        .unwrap_or(value.len());
    let target_col = if wrap {
        vline
            .visual_start_col
            .saturating_add(col)
            .min(vline.visual_end_col)
    } else {
        col.min(vline.visual_end_col)
    };
    let insertions = inline_virtual_insertions_for_line(
        value,
        virtual_texts,
        logical_line_start,
        logical_line_end,
    );
    let byte_in_line = crate::utils::text::byte_at_col_sentinel_tabs_virtual(
        &value[logical_line_start..logical_line_end],
        target_col,
        sentinel.as_ref(),
        tab_stop,
        &insertions,
    );
    logical_line_start.saturating_add(byte_in_line)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        TextAreaCursorCoords, TextAreaCursorLayout, TextAreaCursorParams, paragraph_bounds_at_byte,
        paragraph_line_range, textarea_cursor_from_coords,
    };
    use crate::style::{Rect, ScrollbarVariant, Span};
    use crate::widgets::{TextAreaVirtualText, TextAreaVisualLine};

    #[test]
    fn textarea_dragging_above_selects_to_document_start() {
        let value = "zero\none\ntwo\nthree";
        let visual_lines = vec![
            TextAreaVisualLine {
                line_num: 1,
                continuation: false,
                start: 0,
                end: 4,
                visual_start_col: 0,
                visual_end_col: 4,
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            },
            TextAreaVisualLine {
                line_num: 2,
                continuation: false,
                start: 5,
                end: 8,
                visual_start_col: 0,
                visual_end_col: 3,
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            },
            TextAreaVisualLine {
                line_num: 3,
                continuation: false,
                start: 9,
                end: 12,
                visual_start_col: 0,
                visual_end_col: 3,
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            },
            TextAreaVisualLine {
                line_num: 4,
                continuation: false,
                start: 13,
                end: 18,
                visual_start_col: 0,
                visual_end_col: 5,
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            },
        ];

        let cursor = textarea_cursor_from_coords(TextAreaCursorParams {
            value,
            current_cursor: value.len(),
            coords: TextAreaCursorCoords {
                x: 3,
                y: 0,
                inner: Rect {
                    x: 0,
                    y: 2,
                    w: 10,
                    h: 2,
                },
                clamp_to_inner: false,
            },
            layout: TextAreaCursorLayout {
                line_numbers: false,
                min_line_number_width: 1,
                wrap: false,
                scroll_offset: 2,
                scrollbar: false,
                scrollbar_variant: ScrollbarVariant::Standalone,
                scrollbar_gap: 0,
                scrollbar_over_border: false,
                h_scrollbar: false,
                h_scrollbar_variant: ScrollbarVariant::Standalone,
                h_scrollbar_over_border: false,
                max_line_width: 5,
                h_scroll_offset: 0,
                tab_stop: 8,
                gutter_col_width: 0,
                gutter_gap: 0,
                logical_lines_count: 4,
            },
            read_only: false,
            sentinel: None,
            visual_lines: Some(&visual_lines),
            virtual_texts: &[],
        });

        assert_eq!(cursor, 0);
    }

    #[test]
    fn textarea_cursor_from_coords_clamps_click_inside_virtual_text_to_anchor() {
        let value = "ab";
        let visual_lines = vec![TextAreaVisualLine {
            line_num: 1,
            continuation: false,
            start: 0,
            end: 2,
            visual_start_col: 0,
            visual_end_col: 5,
            starts_with_virtual_text: false,
            ends_with_virtual_text: false,
        }];
        let virtual_texts = vec![TextAreaVirtualText::inline(1, vec![Span::new("xxx")])];

        let cursor = textarea_cursor_from_coords(TextAreaCursorParams {
            value,
            current_cursor: 0,
            coords: TextAreaCursorCoords {
                x: 2,
                y: 0,
                inner: Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 1,
                },
                clamp_to_inner: true,
            },
            layout: TextAreaCursorLayout {
                line_numbers: false,
                min_line_number_width: 0,
                wrap: true,
                scroll_offset: 0,
                scrollbar: false,
                scrollbar_variant: ScrollbarVariant::Standalone,
                scrollbar_gap: 0,
                scrollbar_over_border: false,
                h_scrollbar: false,
                h_scrollbar_variant: ScrollbarVariant::Standalone,
                h_scrollbar_over_border: false,
                max_line_width: 5,
                h_scroll_offset: 0,
                tab_stop: 8,
                gutter_col_width: 0,
                gutter_gap: 0,
                logical_lines_count: 1,
            },
            read_only: false,
            sentinel: None,
            visual_lines: Some(&visual_lines),
            virtual_texts: &virtual_texts,
        });

        assert_eq!(cursor, 1);
    }

    #[test]
    fn textarea_cursor_from_coords_fallback_uses_tab_stop() {
        let cursor = textarea_cursor_from_coords(TextAreaCursorParams {
            value: "a\tb",
            current_cursor: 0,
            coords: TextAreaCursorCoords {
                x: 2,
                y: 0,
                inner: Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 1,
                },
                clamp_to_inner: true,
            },
            layout: TextAreaCursorLayout {
                line_numbers: false,
                min_line_number_width: 0,
                wrap: false,
                scroll_offset: 0,
                scrollbar: false,
                scrollbar_variant: ScrollbarVariant::Standalone,
                scrollbar_gap: 0,
                scrollbar_over_border: false,
                h_scrollbar: false,
                h_scrollbar_variant: ScrollbarVariant::Standalone,
                h_scrollbar_over_border: false,
                max_line_width: 3,
                h_scroll_offset: 0,
                tab_stop: 2,
                gutter_col_width: 0,
                gutter_gap: 0,
                logical_lines_count: 1,
            },
            read_only: false,
            sentinel: None,
            visual_lines: None,
            virtual_texts: &[],
        });

        assert_eq!(cursor, 2);
    }

    #[test]
    fn textarea_cursor_from_coords_fallback_clamps_virtual_text_to_anchor() {
        let virtual_texts = vec![TextAreaVirtualText::inline(1, vec![Span::new("xxx")])];

        let cursor = textarea_cursor_from_coords(TextAreaCursorParams {
            value: "ab",
            current_cursor: 0,
            coords: TextAreaCursorCoords {
                x: 2,
                y: 0,
                inner: Rect {
                    x: 0,
                    y: 0,
                    w: 10,
                    h: 1,
                },
                clamp_to_inner: true,
            },
            layout: TextAreaCursorLayout {
                line_numbers: false,
                min_line_number_width: 0,
                wrap: true,
                scroll_offset: 0,
                scrollbar: false,
                scrollbar_variant: ScrollbarVariant::Standalone,
                scrollbar_gap: 0,
                scrollbar_over_border: false,
                h_scrollbar: false,
                h_scrollbar_variant: ScrollbarVariant::Standalone,
                h_scrollbar_over_border: false,
                max_line_width: 5,
                h_scroll_offset: 0,
                tab_stop: 8,
                gutter_col_width: 0,
                gutter_gap: 0,
                logical_lines_count: 1,
            },
            read_only: false,
            sentinel: None,
            visual_lines: None,
            virtual_texts: &virtual_texts,
        });

        assert_eq!(cursor, 1);
    }

    #[test]
    fn paragraph_bounds_expand_until_blank_lines() {
        let text = "alpha\nbeta\n\ngamma\ndelta\n\n";

        assert_eq!(paragraph_bounds_at_byte(text, 1), (0, 10));
        assert_eq!(paragraph_bounds_at_byte(text, 13), (12, 23));
    }

    #[test]
    fn paragraph_bounds_on_blank_line_stay_on_that_line() {
        let text = "alpha\n\n beta\n";

        assert_eq!(paragraph_bounds_at_byte(text, 6), (6, 6));
    }

    #[test]
    fn paragraph_line_range_groups_non_blank_visual_lines() {
        let lines = ["alpha", "beta", "", "gamma", "delta"];

        assert_eq!(paragraph_line_range(&lines, 0), Some((0, 1)));
        assert_eq!(paragraph_line_range(&lines, 2), Some((2, 2)));
        assert_eq!(paragraph_line_range(&lines, 4), Some((3, 4)));
    }
}

/// Find the word boundaries around a given byte position.
/// Returns (start, end) byte indices of the word.
///
/// When `sentinel` is provided, sentinel characters (inline image placeholders)
/// are treated as their own atomic unit - double-clicking a sentinel selects
/// exactly that one character.
pub(crate) fn word_at_byte(
    text: &str,
    pos: usize,
    sentinel: Option<SentinelInfo>,
) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }

    let pos = pos.min(text.len());

    // If the cursor is at or adjacent to a sentinel character, select it atomically.
    if let Some(ref si) = sentinel {
        let is_sentinel = |c: char| si.is_sentinel(c);

        // Check the character at pos (right side of cursor).
        if pos < text.len() {
            let c = text[pos..].chars().next().unwrap();
            if is_sentinel(c) {
                return (pos, pos + c.len_utf8());
            }
        }
        // Check the character just before pos (left side of cursor).
        if pos > 0
            && let Some(c) = text[..pos].chars().last()
            && is_sentinel(c)
        {
            let start = pos - c.len_utf8();
            return (start, pos);
        }
    }

    #[derive(PartialEq, Copy, Clone)]
    enum Category {
        Word,
        Whitespace,
        Other,
    }

    fn get_category(c: char) -> Category {
        if crate::utils::text::is_word_char(c) {
            Category::Word
        } else if c.is_whitespace() {
            Category::Whitespace
        } else {
            Category::Other
        }
    }

    let left_char = if pos > 0 {
        text[..pos].chars().last()
    } else {
        None
    };
    let right_char = if pos < text.len() {
        text[pos..].chars().next()
    } else {
        None
    };

    let target_cat = match (left_char.map(get_category), right_char.map(get_category)) {
        (Some(Category::Word), _) => Category::Word,
        (_, Some(Category::Word)) => Category::Word,
        (Some(c), _) => c,
        (None, Some(c)) => c,
        (None, None) => return (0, 0),
    };

    let mut start = pos;
    for (i, c) in text[..pos].char_indices().rev() {
        if get_category(c) != target_cat {
            start = i + c.len_utf8();
            break;
        }
        start = i;
    }

    let mut end = pos;
    for (i, c) in text[pos..].char_indices() {
        if get_category(c) != target_cat {
            end = pos + i;
            break;
        }
        end = pos + i + c.len_utf8();
    }

    (start, end)
}

/// Compute the cursor byte position from screen coordinates for an input field.
///
/// This is a simpler version of `textarea_cursor_from_coords` for single-line inputs.
pub(crate) fn input_cursor_from_coords(
    value: &str,
    prefix: Option<&str>,
    x: u16,
    current_cursor: usize,
    inner: Rect,
) -> usize {
    if inner.w == 0 {
        return value.len();
    }

    let line = value.lines().next().unwrap_or("");
    let prefix_w = prefix.map(UnicodeWidthStr::width).unwrap_or(0);

    let cursor = crate::utils::text::clamp_cursor(line, current_cursor.min(line.len()));
    let start = crate::utils::text::input_viewport_start(
        line,
        cursor,
        inner.w.saturating_sub(prefix_w as u16),
    );
    let col = (x as i32)
        .saturating_sub(inner.x as i32)
        .saturating_sub(prefix_w as i32) as usize;
    let new_cursor = start + crate::utils::text::byte_at_col(&line[start..], col);
    crate::utils::text::clamp_cursor(line, new_cursor)
}
