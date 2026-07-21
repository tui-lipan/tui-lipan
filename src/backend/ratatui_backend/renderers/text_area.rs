use std::cell::RefCell;

use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::backend::ratatui_backend::common::{
    DEFAULT_SCROLLBAR_THUMB, IntegratedScrollbarAppearance, ScrollbarAppearance,
    ScrollbarScrollState, calculate_visible_borders, clear_fg_preserve_bg_clipped, finalize_style,
    integrated_hscrollbar_track_char, integrated_vscrollbar_track_char, remember_cursor_position,
    render_hscrollbar, render_integrated_hscrollbar, render_integrated_scrollbar,
    render_vscrollbar, resolve_interactive_style_raw, resolve_scrollbar_thumb_style,
    style_backdrop, style_paints_bg, style_uses_backdrop_bg, to_ratatui_border_set,
    to_ratatui_border_type, to_ratatui_rect, to_ratatui_style,
};
use crate::backend::ratatui_backend::render::{
    FrameIntegratedHTrack, FrameIntegratedVTrack, RenderState, ancestor_frame_integrated_tracks,
    apply_copy_feedback_to_selection_style,
};
use crate::style::resolve::{
    resolve_base_style, resolve_focus_style_defaults, resolve_muted_style, resolve_scrollbar_theme,
};
use crate::style::{BorderStyle, Padding, Rect, ScrollbarVariant, Style, ThemeRole, resolve_slot};
use crate::utils::scrollbar::ScrollbarMetricsCache;
use crate::utils::text::{
    self as util, SentinelInfo, char_visual_width, expand_tabs, str_visual_width,
    str_visual_width_with_tabs, visual_col_with_virtual,
};
use crate::utils::text::{input_viewport_start, viewport};
use crate::widgets::{
    IMAGE_SENTINEL_BASE, TextAreaImageMode, TextAreaLineNumberMode, TextAreaVimConfig,
    TextAreaVimCurrentLineHighlight, TextAreaVimMode, sentinel_info_for,
};
use crate::widgets::{
    TEXT_AREA_LAYER_PRIORITY_CURRENT_SEARCH, TEXT_AREA_LAYER_PRIORITY_SEARCH,
    TEXT_AREA_LAYER_PRIORITY_SELECTION, TextAreaColorCache, TextAreaLayerKind, TextAreaRangeLayer,
    TextAreaStyledSegment, TextAreaVirtualText, TextAreaVisualCache, TextAreaVisualKeyArgs,
    TextAreaVisualLine, eol_virtual_texts_for_visual_line, hash_peer_source_lines,
    inline_virtual_insertions_for_line, inline_virtual_texts_for_visual_line,
    make_text_area_visual_key, public_decoration_layers_for_visible_range, resolve_text_area_spans,
    segments_from_plain, segments_from_spans, text_area_virtual_text_hash,
};

/// Replace `\t` in each span's text with spaces, aligning to `tab_stop`
/// columns measured from `start_col`. Other characters pass through unchanged.
fn expand_tabs_in_spans(
    spans: Vec<Span<'static>>,
    start_col: usize,
    tab_stop: usize,
) -> Vec<Span<'static>> {
    if tab_stop == 0 || !spans.iter().any(|s| s.content.contains('\t')) {
        return spans;
    }
    let mut col = start_col;
    spans
        .into_iter()
        .map(|span| {
            let new_content = expand_tabs(span.content.as_ref(), col, tab_stop).into_owned();
            for ch in span.content.chars() {
                let w = if ch == '\t' {
                    tab_stop - (col % tab_stop)
                } else {
                    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
                };
                col += w;
            }
            Span::styled(new_content, span.style)
        })
        .collect()
}

struct VisualLine<'a> {
    text: &'a str,
    line_num: usize,
    continuation: bool,
    start: usize, // byte offset in original value
    end: usize,   // byte offset end (exclusive) in original value
    visual_start_col: usize,
    visual_end_col: usize,
    starts_with_virtual_text: bool,
    ends_with_virtual_text: bool,
}

fn visual_line_contains_cursor(lines: &[VisualLine<'_>], idx: usize, cursor: usize) -> bool {
    let Some(line) = lines.get(idx) else {
        return false;
    };
    if cursor == line.start && line.continuation {
        return true;
    }
    if cursor >= line.start && cursor < line.end {
        return true;
    }
    let next_starts_at_boundary = lines.get(idx + 1).is_some_and(|next| {
        next.line_num == line.line_num && next.continuation && next.start == cursor
    });
    cursor == line.end && !next_starts_at_boundary
}

fn full_row_bg_from_spans(spans: &[crate::style::Span]) -> Option<Style> {
    let bg = spans.iter().find_map(|span| {
        (span.content.is_empty() && style_paints_bg(span.style))
            .then_some(span.style.bg)
            .flatten()
    })?;
    Some(Style {
        bg: Some(bg),
        ..Style::default()
    })
}

fn split_wrap_padding_row_bg_style(style: Style) -> Option<Style> {
    style_paints_bg(style).then_some(Style {
        bg: style.bg,
        ..Style::default()
    })
}

#[cfg(feature = "diff-view")]
fn diff_context_separator_hover_style(
    config: Option<&crate::widgets::DiffContextSeparatorClickConfig>,
    source_line: usize,
) -> Option<Style> {
    let config = config?;
    config.events_by_source_line.get(source_line)?.as_ref()?;
    let style = config.hover_style?;
    (!style.is_empty()).then_some(style)
}

#[cfg(test)]
fn gutter_spans_with_inset(spans: &[crate::style::Span], inset: u16) -> Vec<Span<'static>> {
    gutter_spans_with_inset_and_style(spans, inset, None)
}

fn gutter_spans_with_inset_and_style(
    spans: &[crate::style::Span],
    inset: u16,
    style_overlay: Option<Style>,
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let patch = |style: Style| {
        style_overlay
            .map(|overlay| style.patch(overlay))
            .unwrap_or(style)
    };
    if inset > 0 {
        let inset_style = spans
            .iter()
            .find(|span| !span.content.is_empty())
            .or_else(|| spans.first())
            .map(|span| span.style)
            .unwrap_or_default();
        out.push(Span::styled(
            " ".repeat(inset as usize),
            to_ratatui_style(patch(inset_style)),
        ));
    }
    out.extend(spans.iter().map(|s| {
        Span::styled(
            s.content.as_ref().to_owned(),
            to_ratatui_style(patch(s.style)),
        )
    }));
    out
}

fn blank_gutter_spans_with_inset(
    spans: &[crate::style::Span],
    gutter_width: u16,
    inset: u16,
    override_style: Option<Style>,
) -> Vec<Span<'static>> {
    use unicode_width::UnicodeWidthStr;

    let mut out = Vec::new();
    let mut painted_width = 0usize;
    let fill_style = override_style.unwrap_or_else(|| {
        spans
            .iter()
            .rev()
            .find(|span| !span.content.is_empty())
            .or_else(|| spans.first())
            .map(|span| span.style)
            .unwrap_or_default()
    });

    if inset > 0 {
        let inset_style = override_style.unwrap_or_else(|| {
            spans
                .iter()
                .find(|span| !span.content.is_empty())
                .or_else(|| spans.first())
                .map(|span| span.style)
                .unwrap_or(fill_style)
        });
        out.push(Span::styled(
            " ".repeat(inset as usize),
            to_ratatui_style(inset_style),
        ));
        painted_width = painted_width.saturating_add(inset as usize);
    }

    for span in spans {
        let span_width = UnicodeWidthStr::width(span.content.as_ref());
        if span_width == 0 {
            continue;
        }
        let span_style = override_style.unwrap_or(span.style);
        out.push(Span::styled(
            " ".repeat(span_width),
            to_ratatui_style(span_style),
        ));
        painted_width = painted_width.saturating_add(span_width);
    }

    let remaining = (gutter_width as usize).saturating_sub(painted_width);
    if remaining > 0 {
        out.push(Span::styled(
            " ".repeat(remaining),
            to_ratatui_style(fill_style),
        ));
    }

    out
}

fn resolved_segments_to_spans<'a>(segments: Vec<TextAreaStyledSegment<'a>>) -> Vec<Span<'a>> {
    segments
        .into_iter()
        .map(|segment| Span::styled(segment.text, to_ratatui_style(segment.style)))
        .collect()
}

fn finalize_text_area_selection_style(
    content_base: Style,
    selection_style: Style,
    contrast_policy: crate::app::ContrastPolicy,
) -> Style {
    finalize_style(
        content_base.patch(selection_style),
        style_backdrop(content_base),
        contrast_policy,
    )
}

fn cache_shape_for_visual_line(line: &VisualLine<'_>) -> TextAreaVisualLine {
    TextAreaVisualLine {
        line_num: line.line_num,
        continuation: line.continuation,
        start: line.start,
        end: line.end,
        visual_start_col: line.visual_start_col,
        visual_end_col: line.visual_end_col,
        starts_with_virtual_text: line.starts_with_virtual_text,
        ends_with_virtual_text: line.ends_with_virtual_text,
    }
}

fn virtual_text_spans_to_ratatui(vt: &TextAreaVirtualText) -> Vec<Span<'static>> {
    vt.content
        .iter()
        .filter_map(|span| {
            let content = if span.content.contains('\n') || span.content.contains('\r') {
                span.content
                    .chars()
                    .filter(|ch| *ch != '\n' && *ch != '\r')
                    .collect::<String>()
            } else {
                span.content.as_ref().to_owned()
            };
            (!content.is_empty()).then(|| Span::styled(content, to_ratatui_style(span.style)))
        })
        .collect()
}

fn search_match_ranges(value: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() || value.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut local_start = 0usize;
    while local_start < value.len() {
        let Some(offset) = value[local_start..].find(query) else {
            break;
        };
        let match_start = local_start + offset;
        let match_end = match_start + query.len();
        ranges.push((match_start, match_end));
        local_start = match_end;
    }
    ranges
}

fn vim_search_bar_prefix(forward: bool) -> &'static str {
    if forward { "  " } else { "  " }
}

fn take_prefix_width(text: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used.saturating_add(width) > max_width {
            break;
        }
        out.push(ch);
        used = used.saturating_add(width);
    }
    out
}

fn take_suffix_width(text: &str, max_width: usize) -> String {
    let mut out = Vec::new();
    let mut used = 0usize;
    for ch in text.chars().rev() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used.saturating_add(width) > max_width {
            break;
        }
        out.push(ch);
        used = used.saturating_add(width);
    }
    out.into_iter().rev().collect()
}

fn search_bar_line(
    feedback: &crate::widgets::TextAreaVimSearchFeedback,
    width: usize,
    bar_style: Style,
    prefix_style: Style,
    count_style: Style,
) -> Line<'static> {
    if width == 0 {
        return Line::from("");
    }

    let suffix = search_bar_count_suffix_for_width(feedback, width);
    let suffix_width = suffix
        .as_deref()
        .map(unicode_width::UnicodeWidthStr::width)
        .unwrap_or(0);
    let query_width = width.saturating_sub(if suffix_width > 0 {
        suffix_width.saturating_add(1)
    } else {
        0
    });
    let (prefix, query) = search_bar_query_parts(feedback, query_width.max(1));
    let prompt_width = unicode_width::UnicodeWidthStr::width(prefix.as_str())
        .saturating_add(unicode_width::UnicodeWidthStr::width(query.as_str()));

    let mut spans = Vec::new();
    if !prefix.is_empty() {
        spans.push(Span::styled(prefix, to_ratatui_style(prefix_style)));
    }
    if !query.is_empty() {
        spans.push(Span::styled(query, to_ratatui_style(bar_style)));
    }
    if let Some(suffix) = suffix
        && prompt_width.saturating_add(suffix_width) < width
    {
        spans.push(Span::styled(
            " ".repeat(width - prompt_width - suffix_width),
            to_ratatui_style(bar_style),
        ));
        spans.push(Span::styled(suffix, to_ratatui_style(count_style)));
    }
    Line::from(spans)
}

fn search_bar_query_parts(
    feedback: &crate::widgets::TextAreaVimSearchFeedback,
    width: usize,
) -> (String, String) {
    if width == 0 {
        return (String::new(), String::new());
    }

    let prefix = vim_search_bar_prefix(feedback.forward);
    let prefix_width = unicode_width::UnicodeWidthStr::width(prefix);
    if prefix_width >= width {
        return (take_prefix_width(prefix, width), String::new());
    }

    let query = feedback.query.as_ref();
    let query_width = unicode_width::UnicodeWidthStr::width(query);
    let available = width.saturating_sub(prefix_width);
    if query_width <= available {
        return (prefix.to_string(), query.to_string());
    }

    let keep = available.saturating_sub(1);
    let tail = take_suffix_width(query, keep);
    (prefix.to_string(), format!("…{tail}"))
}

fn search_bar_count_suffix(feedback: &crate::widgets::TextAreaVimSearchFeedback) -> Option<String> {
    if feedback.query.is_empty() {
        return None;
    }
    Some(format!(
        "[{}/{}]",
        feedback.current_match_index.unwrap_or(0),
        feedback.match_count
    ))
}

fn search_bar_count_suffix_for_width(
    feedback: &crate::widgets::TextAreaVimSearchFeedback,
    width: usize,
) -> Option<String> {
    let suffix = search_bar_count_suffix(feedback)?;
    let prefix_width =
        unicode_width::UnicodeWidthStr::width(vim_search_bar_prefix(feedback.forward));
    let suffix_width = unicode_width::UnicodeWidthStr::width(suffix.as_str());
    let min_query_width = usize::from(!feedback.query.is_empty());
    (width >= prefix_width + min_query_width + 1 + suffix_width).then_some(suffix)
}

fn search_bar_cursor_x(feedback: &crate::widgets::TextAreaVimSearchFeedback, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    let suffix_width = search_bar_count_suffix_for_width(feedback, width as usize)
        .as_deref()
        .map(unicode_width::UnicodeWidthStr::width)
        .unwrap_or(0);
    let query_width = (width as usize).saturating_sub(if suffix_width > 0 {
        suffix_width.saturating_add(1)
    } else {
        0
    });
    let (prefix, query) = search_bar_query_parts(feedback, query_width.max(1));
    let cursor = crate::utils::text::clamp_cursor(feedback.query.as_ref(), feedback.cursor);
    let hidden_query_prefix_width = unicode_width::UnicodeWidthStr::width(feedback.query.as_ref())
        .saturating_sub(unicode_width::UnicodeWidthStr::width(query.as_str()));
    let visible_cursor = cursor.saturating_sub(hidden_query_prefix_width);
    let visible_cursor = crate::utils::text::clamp_cursor(query.as_str(), visible_cursor);
    let text_width = unicode_width::UnicodeWidthStr::width(prefix.as_str()).saturating_add(
        unicode_width::UnicodeWidthStr::width(&query[..visible_cursor]),
    ) as u16;
    text_width.min(width.saturating_sub(1))
}

fn logical_line_num_at_cursor(value: &str, cursor: usize) -> usize {
    let cursor = util::clamp_cursor(value, cursor.min(value.len()));
    value[..cursor]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        .saturating_add(1)
}

fn display_line_number(
    mode: TextAreaLineNumberMode,
    line_num: usize,
    cursor_line_num: usize,
) -> usize {
    match mode {
        TextAreaLineNumberMode::Absolute => line_num,
        TextAreaLineNumberMode::Relative if line_num == cursor_line_num => line_num,
        TextAreaLineNumberMode::Relative => line_num.abs_diff(cursor_line_num),
    }
}

fn resolve_text_area_selection_slot(
    theme: &crate::style::Theme,
    selection_slot: &crate::style::StyleSlot,
    unfocused_selection_slot: &crate::style::StyleSlot,
    is_focused: bool,
) -> Style {
    if is_focused
        || (matches!(unfocused_selection_slot, crate::style::StyleSlot::Inherit)
            && !matches!(selection_slot, crate::style::StyleSlot::Inherit))
    {
        resolve_slot(theme, ThemeRole::TextSelection, selection_slot)
    } else {
        resolve_slot(theme, ThemeRole::TextSelection, unfocused_selection_slot)
    }
}

fn resolve_chrome_relative_slot(base: Style, slot: &crate::style::StyleSlot) -> Style {
    match *slot {
        crate::style::StyleSlot::Inherit => base,
        crate::style::StyleSlot::Extend(style) => base.patch(style),
        crate::style::StyleSlot::Replace(style) => style,
    }
}

fn resolve_search_bar_part_slot(base: Style, slot: &crate::style::StyleSlot) -> Style {
    match *slot {
        crate::style::StyleSlot::Inherit => base,
        crate::style::StyleSlot::Extend(style) | crate::style::StyleSlot::Replace(style) => {
            base.patch(style)
        }
    }
}

fn resolve_current_line_number_slot(base: Style, slot: &crate::style::StyleSlot) -> Style {
    match *slot {
        crate::style::StyleSlot::Inherit => base,
        crate::style::StyleSlot::Extend(style) | crate::style::StyleSlot::Replace(style) => {
            base.patch(style)
        }
    }
}

/// Byte offset, selection overlay, and custom sentinel table for [`substitute_sentinels`].
struct SubstituteSentinelsCtx<'a> {
    byte_start: usize,
    hovered_byte: Option<usize>,
    selection_range: Option<(usize, usize)>,
    selection_style: ratatui::style::Style,
    custom_sentinels: &'a [crate::widgets::TextAreaSentinel],
    inline_virtuals: Vec<(usize, &'a TextAreaVirtualText)>,
    is_focused: bool,
}

/// Replace inline sentinel characters in a list of ratatui spans with styled placeholder spans.
/// Returns an owned Vec with all content heap-allocated.
///
/// `ctx.byte_start` is the byte offset of the first character in `spans` relative to the full
/// value string. Together with `ctx.selection_range` and `ctx.selection_style` it allows the
/// placeholder span to receive the same selection overlay as regular text.
fn substitute_sentinels(
    spans: Vec<Span<'_>>,
    num_images: usize,
    placeholder: &str,
    placeholder_style: Style,
    placeholder_hover_style: Style,
    ctx: SubstituteSentinelsCtx<'_>,
) -> Vec<Span<'static>> {
    let SubstituteSentinelsCtx {
        byte_start,
        hovered_byte,
        selection_range,
        selection_style,
        custom_sentinels,
        inline_virtuals,
        is_focused,
    } = ctx;

    let image_sentinel_base = IMAGE_SENTINEL_BASE as u32;
    let image_sentinel_end = image_sentinel_base + num_images as u32;
    let custom_sentinel_base = crate::widgets::SENTINEL_BASE as u32;
    let custom_sentinel_end = custom_sentinel_base + custom_sentinels.len() as u32;

    let has_any = num_images > 0 || !custom_sentinels.is_empty();
    let has_virtuals = !inline_virtuals.is_empty();
    if !has_any && !has_virtuals {
        return spans
            .into_iter()
            .map(|s| Span::styled(s.content.into_owned(), s.style))
            .collect();
    }
    let placeholder_owned = placeholder.to_owned();
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut byte_pos = byte_start;
    let mut virtual_idx = 0usize;
    let push_virtuals_at =
        |anchor: usize, virtual_idx: &mut usize, out: &mut Vec<Span<'static>>| {
            while let Some((virtual_anchor, vt)) = inline_virtuals.get(*virtual_idx) {
                if *virtual_anchor != anchor {
                    break;
                }
                out.extend(virtual_text_spans_to_ratatui(vt));
                *virtual_idx = (*virtual_idx).saturating_add(1);
            }
        };
    push_virtuals_at(byte_pos, &mut virtual_idx, &mut out);
    for span in spans {
        let text = span.content.as_ref();
        let has_sentinel = text.chars().any(|c| {
            let cp = c as u32;
            (cp >= image_sentinel_base && cp < image_sentinel_end)
                || (cp >= custom_sentinel_base && cp < custom_sentinel_end)
        });
        if !has_sentinel && !has_virtuals {
            byte_pos += text.len();
            out.push(Span::styled(text.to_owned(), span.style));
            continue;
        }
        let mut buf = String::new();
        for ch in text.chars() {
            if inline_virtuals
                .get(virtual_idx)
                .is_some_and(|(anchor, _)| *anchor == byte_pos)
            {
                if !buf.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                push_virtuals_at(byte_pos, &mut virtual_idx, &mut out);
            }
            let cp = ch as u32;
            if cp >= image_sentinel_base && cp < image_sentinel_end {
                if !buf.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                let index = (cp - image_sentinel_base) as usize + 1;
                let label = if placeholder.contains('X') {
                    placeholder.replace('X', index.to_string().as_str())
                } else {
                    placeholder_owned.clone()
                };
                let in_selection =
                    selection_range.is_some_and(|(s, e)| byte_pos >= s && byte_pos < e);
                let mut style = placeholder_style;
                if hovered_byte == Some(byte_pos) {
                    style = style.patch(placeholder_hover_style);
                }
                let base_rat = to_ratatui_style(style);
                let effective = if in_selection {
                    base_rat.patch(selection_style)
                } else {
                    base_rat
                };
                out.push(Span::styled(label, effective));
            } else if cp >= custom_sentinel_base && cp < custom_sentinel_end {
                if !buf.is_empty() {
                    out.push(Span::styled(std::mem::take(&mut buf), span.style));
                }
                let idx = (cp - custom_sentinel_base) as usize;
                let entry = &custom_sentinels[idx];
                let base_style = if is_focused {
                    entry.focus_style.unwrap_or(entry.style)
                } else {
                    entry.style
                };
                let base_style = if hovered_byte == Some(byte_pos) {
                    if let Some(hover_style) = entry.hover_style {
                        base_style.patch(hover_style)
                    } else {
                        base_style
                    }
                } else {
                    base_style
                };
                let base_rat = to_ratatui_style(base_style);
                let in_selection =
                    selection_range.is_some_and(|(s, e)| byte_pos >= s && byte_pos < e);
                let effective = if in_selection {
                    base_rat.patch(selection_style)
                } else {
                    base_rat
                };
                out.push(Span::styled(entry.label.as_ref().to_owned(), effective));
            } else {
                buf.push(ch);
            }
            byte_pos += ch.len_utf8();
        }
        if !buf.is_empty() {
            out.push(Span::styled(buf, span.style));
        }
    }
    push_virtuals_at(byte_pos, &mut virtual_idx, &mut out);
    out
}

fn calculate_visual_lines<'a>(
    value: &'a str,
    content_width: usize,
    wrap: bool,
    caret: Option<usize>,
    sentinel: Option<&SentinelInfo>,
) -> Vec<VisualLine<'a>> {
    let mut visual_lines = Vec::new();
    let mut current_byte_offset = 0;

    for (i, line) in value.split('\n').enumerate() {
        let line_num = i + 1;
        let line_len = line.len();

        if !wrap || str_visual_width(line, sentinel) <= content_width {
            visual_lines.push(VisualLine {
                text: line,
                line_num,
                continuation: false,
                start: current_byte_offset,
                end: current_byte_offset + line_len,
                visual_start_col: 0,
                visual_end_col: str_visual_width(line, sentinel),
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            });
        } else {
            // Word-aware wrapping: try to break at word boundaries
            let mut start_idx = 0;
            let mut current_width = 0;
            let mut is_first = true;
            let mut last_break_idx = 0; // byte index of last breakable position
            let mut last_break_width = 0; // width at that position

            for (idx, ch) in line.char_indices() {
                let char_width = char_visual_width(ch, sentinel);

                if current_width + char_width > content_width {
                    // Need to break - prefer word boundary if available AND it fits
                    let (break_idx, break_width) =
                        if last_break_idx > start_idx && last_break_width <= content_width {
                            // We have a word boundary that fits within content_width
                            (last_break_idx, last_break_width)
                        } else {
                            // No valid word boundary or it doesn't fit, force break at current position
                            (idx, current_width)
                        };

                    visual_lines.push(VisualLine {
                        text: &line[start_idx..break_idx],
                        line_num,
                        continuation: !is_first,
                        start: current_byte_offset + start_idx,
                        end: current_byte_offset + break_idx,
                        visual_start_col: str_visual_width(&line[..start_idx], sentinel),
                        visual_end_col: str_visual_width(&line[..break_idx], sentinel),
                        starts_with_virtual_text: false,
                        ends_with_virtual_text: false,
                    });
                    start_idx = break_idx;
                    current_width = current_width + char_width - break_width;
                    is_first = false;
                    last_break_idx = start_idx;
                    last_break_width = 0;
                } else {
                    current_width += char_width;
                }

                if ch.is_whitespace() {
                    last_break_idx = idx + ch.len_utf8();
                    last_break_width = current_width;
                }
            }
            // Remainder
            visual_lines.push(VisualLine {
                text: &line[start_idx..],
                line_num,
                continuation: !is_first,
                start: current_byte_offset + start_idx,
                end: current_byte_offset + line_len,
                visual_start_col: str_visual_width(&line[..start_idx], sentinel),
                visual_end_col: str_visual_width(line, sentinel),
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            });
        }

        if caret == Some(current_byte_offset + line_len)
            && wrap
            && content_width > 0
            && visual_lines.last().is_some_and(|last| {
                last.line_num == line_num
                    && last.end == current_byte_offset + line_len
                    && last.visual_end_col.saturating_sub(last.visual_start_col) == content_width
            })
        {
            let last = visual_lines.pop().expect("checked last visual line");
            if content_width == 1 || last.text.ends_with(char::is_whitespace) {
                let boundary = last.end;
                let visual_col = last.visual_end_col;
                visual_lines.push(last);
                visual_lines.push(VisualLine {
                    text: "",
                    line_num,
                    continuation: true,
                    start: boundary,
                    end: boundary,
                    visual_start_col: visual_col,
                    visual_end_col: visual_col,
                    starts_with_virtual_text: false,
                    ends_with_virtual_text: false,
                });
            } else {
                let start = last.start;
                let start_col = last.visual_start_col;
                let continuation = last.continuation;
                for (idx, mut row) in
                    calculate_visual_lines(last.text, content_width - 1, true, None, sentinel)
                        .into_iter()
                        .enumerate()
                {
                    row.line_num = line_num;
                    row.continuation = continuation || idx > 0;
                    row.start = row.start.saturating_add(start);
                    row.end = row.end.saturating_add(start);
                    row.visual_start_col = row.visual_start_col.saturating_add(start_col);
                    row.visual_end_col = row.visual_end_col.saturating_add(start_col);
                    visual_lines.push(row);
                }
            }
        }

        current_byte_offset += line_len + 1; // +1 includes the newline char
    }
    visual_lines
}

fn build_visual_lines<'a>(
    value: &'a str,
    content_width: usize,
    wrap: bool,
    caret: Option<usize>,
    cached_lines: Option<&[TextAreaVisualLine]>,
    sentinel: Option<&SentinelInfo>,
) -> Vec<VisualLine<'a>> {
    if let Some(cached) = cached_lines {
        return cached
            .iter()
            .map(|line| VisualLine {
                text: &value[line.start..line.end],
                line_num: line.line_num,
                continuation: line.continuation,
                start: line.start,
                end: line.end,
                visual_start_col: line.visual_start_col,
                visual_end_col: line.visual_end_col,
                starts_with_virtual_text: line.starts_with_virtual_text,
                ends_with_virtual_text: line.ends_with_virtual_text,
            })
            .collect();
    }

    calculate_visual_lines(value, content_width, wrap, caret, sentinel)
}

pub(crate) struct TextAreaVimRenderCtx<'a> {
    pub search_feedback: Option<&'a crate::widgets::TextAreaVimSearchFeedback>,
    pub config: &'a TextAreaVimConfig,
    pub mode: TextAreaVimMode,
    pub visual_line_caret: Option<usize>,
    pub yank_feedback_range: Option<(usize, usize)>,
    pub search_bar_style: Style,
    pub search_bar_prefix_style: Style,
    pub search_bar_count_style: Style,
    pub search_match_style: Style,
    pub current_search_match_style: Style,
    pub current_line_style: Style,
    pub current_line_number_style: Style,
}

pub(crate) struct TextAreaChromeRenderCtx {
    pub chrome_style: Style,
    pub content_style: Style,
    pub hover_border_style: Option<BorderStyle>,
    pub selection_style: Style,
    pub placeholder_style: Style,
    pub line_numbers: bool,
    pub line_number_mode: TextAreaLineNumberMode,
    pub line_number_style: Style,
    pub min_line_number_width: u8,
    pub wrap: bool,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
}

pub(crate) struct TextAreaScrollRenderCtx<'a> {
    pub scroll_offset: usize,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub h_scrollbar_thumb: Option<char>,
    pub h_scroll_offset: usize,
    pub value_hash: u64,
    pub peer_hash: u64,
    pub visual_cache: &'a TextAreaVisualCache,
    pub color_cache: &'a TextAreaColorCache,
    pub metrics_cache: Option<&'a RefCell<ScrollbarMetricsCache>>,
    pub parent_integrated_v: Option<FrameIntegratedVTrack>,
    pub parent_integrated_h: Option<FrameIntegratedHTrack>,
}

pub(crate) struct TextAreaInteractionRenderCtx<'a> {
    pub is_focused: bool,
    pub show_selection_when_unfocused: bool,
    pub is_hovered: bool,
    pub blink_visible: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub cursor_sink: Option<&'a std::cell::Cell<Option<ratatui::layout::Position>>>,
}

pub(crate) struct TextAreaLayoutRenderCtx {
    pub rect: Rect,
    pub rrect: ratatui::layout::Rect,
    pub clip_rect: Option<Rect>,
}

pub(crate) struct TextAreaExtrasRenderCtx<'a> {
    pub images_count: usize,
    pub image_mode: TextAreaImageMode,
    pub image_placeholder: &'a str,
    pub image_placeholder_style: Style,
    pub image_placeholder_focus_style: Style,
    pub image_placeholder_hover_style: Style,
    pub sentinels: &'a [crate::widgets::TextAreaSentinel],
    pub gutter_lines: Option<&'a [Vec<crate::style::Span>]>,
    pub gutter_col_width: u16,
    pub gutter_gap: u16,
    pub split_wrap_padding_gutter_style: Option<Style>,
    pub split_wrap_padding_style: Option<Style>,
    pub selection_excluded_lines: Option<&'a [usize]>,
    pub decorations: &'a [crate::widgets::TextAreaDecoration],
    pub virtual_texts: &'a [TextAreaVirtualText],
    pub geometry: &'a crate::widgets::TextAreaGeometry,
    #[cfg(feature = "diff-view")]
    pub diff_context_separator_click: Option<&'a crate::widgets::DiffContextSeparatorClickConfig>,
    pub hover_mouse_pos: Option<(u16, u16)>,
    pub tab_stop: usize,
}

pub(crate) struct TextAreaContentRenderCtx<'a> {
    pub value: &'a str,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub show_cursor_with_selection: bool,
    pub placeholder: Option<&'a str>,
}

pub(crate) struct TextAreaRenderParts<'a> {
    pub vim: TextAreaVimRenderCtx<'a>,
    pub chrome: TextAreaChromeRenderCtx,
    pub scroll: TextAreaScrollRenderCtx<'a>,
    pub interaction: TextAreaInteractionRenderCtx<'a>,
    pub layout: TextAreaLayoutRenderCtx,
    pub extras: TextAreaExtrasRenderCtx<'a>,
}

pub(crate) fn render_text_area(
    f: &mut ratatui::Frame<'_>,
    content: TextAreaContentRenderCtx<'_>,
    parts: TextAreaRenderParts<'_>,
) {
    let TextAreaContentRenderCtx {
        value,
        cursor,
        anchor,
        show_cursor_with_selection,
        placeholder,
    } = content;
    let TextAreaRenderParts {
        vim,
        chrome,
        scroll,
        interaction,
        layout,
        extras,
    } = parts;
    let TextAreaVimRenderCtx {
        search_feedback: vim_search_feedback,
        config: vim_config,
        mode: vim_mode,
        visual_line_caret: vim_visual_line_caret,
        yank_feedback_range: vim_yank_feedback_range,
        search_bar_style: vim_search_bar_style,
        search_bar_prefix_style: vim_search_bar_prefix_style,
        search_bar_count_style: vim_search_bar_count_style,
        search_match_style: vim_search_match_style,
        current_search_match_style: vim_current_search_match_style,
        current_line_style: vim_current_line_style,
        current_line_number_style: vim_current_line_number_style,
    } = vim;
    let TextAreaChromeRenderCtx {
        chrome_style,
        content_style,
        hover_border_style,
        selection_style,
        placeholder_style,
        line_numbers,
        line_number_mode,
        line_number_style,
        min_line_number_width,
        wrap,
        border,
        border_style,
        padding,
    } = chrome;
    let TextAreaScrollRenderCtx {
        scroll_offset,
        scrollbar,
        scrollbar_variant,
        scrollbar_gap,
        scrollbar_thumb,
        scrollbar_thumb_style,
        scrollbar_thumb_focus_style,
        scrollbar_track_style,
        h_scrollbar_variant,
        h_scrollbar_thumb,
        h_scroll_offset,
        value_hash,
        peer_hash,
        visual_cache,
        color_cache,
        metrics_cache,
        parent_integrated_v,
        parent_integrated_h,
    } = scroll;
    let TextAreaInteractionRenderCtx {
        is_focused,
        show_selection_when_unfocused,
        is_hovered,
        blink_visible,
        disabled,
        read_only,
        cursor_sink,
    } = interaction;
    let TextAreaLayoutRenderCtx {
        rect,
        rrect,
        clip_rect,
    } = layout;
    let TextAreaExtrasRenderCtx {
        images_count,
        image_mode,
        image_placeholder,
        image_placeholder_style,
        image_placeholder_focus_style,
        image_placeholder_hover_style,
        sentinels,
        gutter_lines,
        gutter_col_width,
        gutter_gap,
        split_wrap_padding_gutter_style,
        split_wrap_padding_style,
        selection_excluded_lines,
        decorations,
        virtual_texts,
        geometry,
        #[cfg(feature = "diff-view")]
        diff_context_separator_click,
        hover_mouse_pos,
        tab_stop,
    } = extras;
    let ph_style = placeholder_style;
    let clip_rrect = clip_rect.map(to_ratatui_rect);

    // 1. Draw Background & Border
    if style_uses_backdrop_bg(chrome_style) {
        clear_fg_preserve_bg_clipped(f, rect, clip_rect);
    } else if style_paints_bg(chrome_style) {
        f.render_widget(Clear, rrect);
    }

    let mut inner = rect;
    if border {
        let mut border_type = border_style;
        if !disabled
            && is_hovered
            && let Some(bt) = hover_border_style
        {
            border_type = bt;
        }
        let borders = calculate_visible_borders(rect, clip_rect);
        let mut block = Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(border_type))
            .style(to_ratatui_style(chrome_style));

        if let Some(set) = to_ratatui_border_set(border_type) {
            block = block.border_set(set);
        }

        f.render_widget(block, rrect);

        // Always reserve space for borders if enabled, even if clipped
        inner.x = inner.x.saturating_add(1);
        inner.w = inner.w.saturating_sub(2);
        inner.y = inner.y.saturating_add(1);
        inner.h = inner.h.saturating_sub(2);
    } else if style_uses_backdrop_bg(chrome_style) {
        clear_fg_preserve_bg_clipped(f, rect, clip_rect);
    } else if style_paints_bg(chrome_style) {
        let bg = Block::default().style(to_ratatui_style(chrome_style));
        f.render_widget(bg, rrect);
    }

    inner = inner.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    // 2. Prepare Layout (Wrapping)
    let placeholder_active = value.is_empty() && placeholder.is_some();
    let render_value = if placeholder_active {
        placeholder.unwrap_or("")
    } else {
        value
    };

    let gutter_base_width = if geometry.gutter_width > 0 {
        geometry.gutter_width.saturating_sub(gutter_gap as usize)
    } else {
        0
    };
    let gutter_width = geometry.gutter_width;
    let content_width = geometry.content_width;

    let h_scrollbar_over_border = geometry.h_scrollbar_visible
        && matches!(h_scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_integrated_h.is_some());
    let search_status_feedback = vim_search_feedback.filter(|_| is_focused && inner.h > 0);
    let pending_search_feedback = search_status_feedback.filter(|feedback| feedback.pending);
    let search_bar_active = pending_search_feedback.is_some();

    let mut content_inner = inner;
    content_inner.h = if search_bar_active && !h_scrollbar_over_border {
        inner.h
    } else {
        geometry.content_viewport_h(h_scrollbar_over_border)
    };
    let editor_viewport_h = if search_bar_active {
        content_inner.h.saturating_sub(1)
    } else {
        content_inner.h
    };
    let search_bar_y =
        search_bar_active.then(|| content_inner.y.saturating_add(editor_viewport_h as i16));

    if content_width == 0 {
        if let (Some(feedback), Some(y)) = (pending_search_feedback, search_bar_y) {
            let search_rect = ratatui::layout::Rect {
                x: inner.x.max(0) as u16,
                y: y.max(0) as u16,
                width: inner.w,
                height: 1,
            }
            .intersection(rrect);
            if search_rect.width > 0 {
                let prompt = search_bar_line(
                    feedback,
                    search_rect.width as usize,
                    vim_search_bar_style,
                    vim_search_bar_prefix_style,
                    vim_search_bar_count_style,
                );
                f.render_widget(
                    Block::default().style(to_ratatui_style(vim_search_bar_style)),
                    search_rect,
                );
                f.render_widget(
                    Paragraph::new(prompt).style(to_ratatui_style(vim_search_bar_style)),
                    search_rect,
                );
            }

            if feedback.pending && !disabled && !read_only && inner.w > 0 {
                let cx = inner
                    .x
                    .saturating_add(search_bar_cursor_x(feedback, inner.w) as i16);
                let cy = y;
                if cx >= rrect.x as i16
                    && cx < (rrect.x as i32 + rrect.width as i32) as i16
                    && cy >= rrect.y as i16
                    && cy < (rrect.y as i32 + rrect.height as i32) as i16
                {
                    let position = ratatui::layout::Position::new(cx as u16, cy as u16);
                    f.set_cursor_position(position);
                    remember_cursor_position(cursor_sink, position);
                }
            }
        }
        return;
    }

    let scrollbar_over_border = scrollbar
        && matches!(scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_integrated_v.is_some());
    let sentinel = sentinel_info_for(image_mode, images_count, image_placeholder, sentinels);
    let (sentinel_ph_width, sentinel_count) = sentinel
        .as_ref()
        .and_then(|si| si.image.map(|(_, _, pw)| (pw, images_count)))
        .unwrap_or((0, 0));
    let custom_sentinel_hash: u64 = {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        if let Some(si) = sentinel.as_ref()
            && let Some((_, _, ref widths, _)) = si.custom
        {
            widths.hash(&mut h);
        }
        h.finish()
    };
    let cache_key = make_text_area_visual_key(
        value_hash,
        peer_hash,
        TextAreaVisualKeyArgs {
            inner_w: geometry.inner_w,
            wrap,
            line_numbers,
            min_line_number_width,
            scrollbar,
            scrollbar_over_border,
            scrollbar_gap,
            read_only,
            cursor,
            tab_stop: tab_stop as u8,
            sentinel_ph_width,
            sentinel_count,
            custom_sentinel_hash,
            virtual_text_hash: text_area_virtual_text_hash(virtual_texts),
            gutter_col_width,
            gutter_gap,
            #[cfg(feature = "diff-view")]
            split_wrap_pane_widths: None,
            #[cfg(feature = "diff-view")]
            split_wrap_scrollbar_cols: None,
            #[cfg(feature = "diff-view")]
            split_wrap_layout_pass: 0,
        },
    );
    let cached_lines = if placeholder_active {
        None
    } else {
        visual_cache.get_lines(&cache_key)
    };
    let visual_lines = build_visual_lines(
        render_value,
        content_width,
        wrap,
        (!read_only && !placeholder_active).then_some(cursor),
        cached_lines,
        sentinel.as_ref(),
    );

    let h_scroll_start = if wrap { 0 } else { h_scroll_offset };

    // 3. Render Lines
    let selection_range = if !placeholder_active && (is_focused || show_selection_when_unfocused) {
        anchor.map(|a| {
            let s = cursor.min(value.len()).min(a.min(value.len()));
            let e = cursor.min(value.len()).max(a.min(value.len()));
            (s, e)
        })
    } else {
        None
    }
    .or_else(|| {
        if placeholder_active {
            return None;
        }
        vim_yank_feedback_range.map(|(start, end)| {
            let s = start.min(value.len());
            let e = end.min(value.len());
            (s.min(e), s.max(e))
        })
    });
    let active_search_feedback = vim_search_feedback.filter(|feedback| !feedback.query.is_empty());
    let search_query = active_search_feedback.map(|feedback| feedback.query.as_ref());
    let all_search_ranges = search_query
        .map(|query| search_match_ranges(render_value, query))
        .unwrap_or_default();
    let current_search_range = active_search_feedback.and_then(|feedback| feedback.target_range);
    let current_search_count_suffix = active_search_feedback
        .filter(|feedback| !feedback.pending)
        .and_then(search_bar_count_suffix);

    let content_style = if placeholder_active {
        ph_style
    } else {
        content_style
    };
    let current_line_num = (is_focused
        && show_cursor_with_selection
        && !placeholder_active
        && !matches!(
            vim_mode,
            TextAreaVimMode::Visual | TextAreaVimMode::VisualLine
        )
        && vim_config.current_line_highlight != TextAreaVimCurrentLineHighlight::Off)
        .then(|| logical_line_num_at_cursor(value, cursor));
    let line_number_cursor = vim_visual_line_caret.unwrap_or(cursor);
    let line_number_cursor_num = (line_numbers
        && matches!(line_number_mode, TextAreaLineNumberMode::Relative))
    .then(|| logical_line_num_at_cursor(value, line_number_cursor));

    let color_lines = if placeholder_active || color_cache.lines.is_empty() {
        None
    } else {
        Some(color_cache.lines.as_slice())
    };
    let line_starts = color_lines.map(|_| color_cache.line_starts.as_slice());
    let line_lengths = color_lines.map(|_| color_cache.line_lengths.as_slice());

    let content_x = inner.x.saturating_add(gutter_width as i16);
    let content_bg_x = content_x;
    // Use content_width for rendering to match scroll calculations
    let content_rect_width = content_width as u16;
    let content_bg_width = content_rect_width;

    // Apply scroll offset: skip the first `scroll_offset` visual lines
    let inline_mode = image_mode == TextAreaImageMode::Inline;

    for (y, (visual_idx, vline)) in visual_lines
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(editor_viewport_h as usize)
        .enumerate()
    {
        let cache_line = cache_shape_for_visual_line(vline);
        let y_pos = content_inner.y.saturating_add(y as i16);
        let is_synthetic_padding_line = wrap
            && vline.continuation
            && vline.start == vline.end
            && vline.text.is_empty()
            && inline_virtual_texts_for_visual_line(value, virtual_texts, &cache_line).is_empty();
        let is_current_line = current_line_num == Some(vline.line_num);
        let row_content_style = if is_current_line {
            content_style.patch(vim_current_line_style)
        } else {
            content_style
        };
        let row_line_number_style = if is_current_line {
            line_number_style.patch(vim_current_line_number_style)
        } else {
            line_number_style
        };

        if is_current_line {
            let (highlight_x, highlight_width) = match vim_config.current_line_highlight {
                TextAreaVimCurrentLineHighlight::Full => (inner.x, inner.w),
                TextAreaVimCurrentLineHighlight::Content => (content_x, content_rect_width),
                TextAreaVimCurrentLineHighlight::Off => (content_x, 0),
            };
            if highlight_width > 0 {
                let highlight_rect = ratatui::layout::Rect {
                    x: highlight_x.max(0) as u16,
                    y: y_pos.max(0) as u16,
                    width: highlight_width,
                    height: 1,
                }
                .intersection(rrect);
                if highlight_rect.width > 0 {
                    f.render_widget(
                        Block::default().style(to_ratatui_style(vim_current_line_style)),
                        highlight_rect,
                    );
                }
            }
        }

        // Render Gutter
        if let Some(custom_gutter) = gutter_lines {
            // Custom gutter: render per-logical-line spans (continuation = empty)
            let gutter_rect = ratatui::layout::Rect {
                x: inner.x.max(0) as u16,
                y: y_pos.max(0) as u16,
                width: gutter_width as u16,
                height: 1,
            };
            let effective_gutter = gutter_rect.intersection(rrect);
            if effective_gutter.width > 0 {
                let spans: Vec<Span<'static>> = if !vline.continuation {
                    let idx = vline.line_num.saturating_sub(1);
                    custom_gutter
                        .get(idx)
                        .map(|line_spans| {
                            gutter_spans_with_inset_and_style(
                                line_spans,
                                gutter_gap,
                                is_current_line.then_some(vim_current_line_number_style),
                            )
                        })
                        .unwrap_or_default()
                } else {
                    let idx = vline.line_num.saturating_sub(1);
                    custom_gutter
                        .get(idx)
                        .map(|line_spans| {
                            let gutter_style = if is_synthetic_padding_line {
                                split_wrap_padding_gutter_style
                            } else if is_current_line {
                                Some(vim_current_line_number_style)
                            } else {
                                None
                            };
                            blank_gutter_spans_with_inset(
                                line_spans,
                                gutter_width as u16,
                                gutter_gap,
                                gutter_style,
                            )
                        })
                        .unwrap_or_default()
                };
                f.render_widget(Paragraph::new(Line::from(spans)), effective_gutter);
            }
        } else if line_numbers {
            let gutter_rect = ratatui::layout::Rect {
                x: inner.x.max(0) as u16,
                y: y_pos.max(0) as u16,
                width: gutter_width as u16,
                height: 1,
            };

            let gutter_text = if !vline.continuation {
                let line_num = line_number_cursor_num.map_or(vline.line_num, |cursor_line_num| {
                    display_line_number(line_number_mode, vline.line_num, cursor_line_num)
                });
                format!(
                    "{}{:>width$} │",
                    " ".repeat(gutter_gap as usize),
                    line_num,
                    width = gutter_base_width.saturating_sub(2)
                )
            } else {
                format!(
                    "{}{:>width$} │",
                    " ".repeat(gutter_gap as usize),
                    " ",
                    width = gutter_base_width.saturating_sub(2)
                )
            };

            let effective_gutter = gutter_rect.intersection(rrect);
            let gdx = (effective_gutter.x as i32)
                .saturating_sub(gutter_rect.x as i32)
                .max(0) as u16;
            let gdy = (effective_gutter.y as i32)
                .saturating_sub(gutter_rect.y as i32)
                .max(0) as u16;

            f.render_widget(
                Paragraph::new(gutter_text)
                    .style(to_ratatui_style(row_line_number_style))
                    .scroll((gdy, gdx)),
                effective_gutter,
            );
        }

        // Render Content
        let content_rect = ratatui::layout::Rect {
            x: content_x.max(0) as u16,
            y: y_pos.max(0) as u16,
            width: content_rect_width,
            height: 1,
        };
        let content_bg_rect = ratatui::layout::Rect {
            x: content_bg_x.max(0) as u16,
            y: y_pos.max(0) as u16,
            width: content_bg_width,
            height: 1,
        };

        let clipped_content = content_rect.intersection(rrect);
        let clipped_content_bg = content_bg_rect.intersection(rrect);
        if clipped_content.width == 0 || clipped_content.height == 0 {
            continue;
        }
        #[cfg(feature = "diff-view")]
        let context_separator_hover_style = hover_mouse_pos.and_then(|(mx, my)| {
            let mx = mx as i16;
            let my = my as i16;
            let right = content_bg_x.saturating_add(content_bg_width as i16);
            if my == y_pos && mx >= content_bg_x && mx < right {
                diff_context_separator_hover_style(
                    diff_context_separator_click,
                    vline.line_num.saturating_sub(1),
                )
            } else {
                None
            }
        });
        #[cfg(not(feature = "diff-view"))]
        let context_separator_hover_style: Option<Style> = None;

        // Apply horizontal scroll when wrap is disabled
        // Compute the visible portion of this line
        let has_virtual_text = !virtual_texts.is_empty();
        let (visible_text, visible_byte_start, visible_byte_end, visible_start_col) =
            if !wrap && h_scroll_start > 0 && !has_virtual_text {
                // Find byte offset corresponding to h_scroll_start columns
                let line_text = vline.text;
                let start_byte = util::byte_at_col_sentinel_tabs(
                    line_text,
                    h_scroll_start,
                    sentinel.as_ref(),
                    tab_stop,
                );
                let end_byte = util::end_at_width_sentinel_tabs(
                    line_text,
                    start_byte,
                    content_width,
                    sentinel.as_ref(),
                    h_scroll_start,
                    tab_stop,
                );
                (
                    &line_text[start_byte..end_byte],
                    vline.start + start_byte,
                    vline.start + end_byte,
                    h_scroll_start,
                )
            } else if !wrap && !has_virtual_text {
                let line_text = vline.text;
                let end_byte = util::end_at_width_sentinel_tabs(
                    line_text,
                    0,
                    content_width,
                    sentinel.as_ref(),
                    0,
                    tab_stop,
                );
                (
                    &line_text[..end_byte],
                    vline.start,
                    vline.start + end_byte,
                    0,
                )
            } else if !wrap {
                (vline.text, vline.start, vline.end, 0)
            } else {
                // Wrapped: compute the visual column at which this visual line begins
                // within its logical line, so tab expansion aligns correctly.
                let logical_line_start = value[..vline.start].rfind('\n').map_or(0, |i| i + 1);
                let start_col = if vline.continuation {
                    str_visual_width_with_tabs(
                        &value[logical_line_start..vline.start],
                        sentinel.as_ref(),
                        0,
                        tab_stop,
                    )
                } else {
                    0
                };
                (vline.text, vline.start, vline.end, start_col)
            };

        let mut segments = None;
        let mut full_row_bg = None;
        if is_synthetic_padding_line {
            full_row_bg = split_wrap_padding_style.and_then(split_wrap_padding_row_bg_style);
        } else if let (Some(lines), Some(line_starts), Some(line_lengths)) =
            (color_lines, line_starts.as_ref(), line_lengths.as_ref())
        {
            let line_idx = vline.line_num.saturating_sub(1);
            if let (Some(line_spans), Some(line_start), Some(line_len)) = (
                lines.get(line_idx),
                line_starts.get(line_idx),
                line_lengths.get(line_idx),
            ) {
                let span_len = line_spans
                    .iter()
                    .fold(0usize, |acc, span| acc.saturating_add(span.content.len()));
                let span_range_end = if span_len > *line_len {
                    line_start.saturating_add(span_len)
                } else {
                    visible_byte_end
                };
                full_row_bg = full_row_bg_from_spans(line_spans);
                segments = segments_from_spans(
                    line_spans,
                    *line_start,
                    *line_len,
                    visible_byte_start,
                    span_range_end,
                    row_content_style,
                );
            }
        }

        let segments = segments.unwrap_or_else(|| {
            segments_from_plain(
                visible_text,
                row_content_style,
                visible_byte_start,
                visible_byte_end,
            )
        });

        let search_ranges = all_search_ranges
            .iter()
            .copied()
            .filter(|(start, end)| *end > visible_byte_start && *start < visible_byte_end)
            .collect::<Vec<_>>();
        let mut layers = Vec::new();
        if !search_ranges.is_empty() {
            layers.push(TextAreaRangeLayer::new(
                search_ranges,
                vim_search_match_style,
                TEXT_AREA_LAYER_PRIORITY_SEARCH,
                TextAreaLayerKind::Search,
            ));
        }
        let current_search_ranges = current_search_range
            .filter(|(start, end)| *end > visible_byte_start && *start < visible_byte_end)
            .map(|range| vec![range])
            .unwrap_or_default();
        if !current_search_ranges.is_empty() {
            layers.push(TextAreaRangeLayer::new(
                current_search_ranges,
                vim_current_search_match_style,
                TEXT_AREA_LAYER_PRIORITY_CURRENT_SEARCH,
                TextAreaLayerKind::CurrentSearch,
            ));
        }
        layers.extend(public_decoration_layers_for_visible_range(
            decorations,
            visible_byte_start,
            visible_byte_end,
            vline.start,
            vline.end,
        ));
        if let Some((sel_start, sel_end)) = selection_range
            && sel_start != sel_end
        {
            layers.push(TextAreaRangeLayer::single(
                (sel_start, sel_end),
                selection_style,
                TEXT_AREA_LAYER_PRIORITY_SELECTION,
                TextAreaLayerKind::Selection,
            ));
        }
        if let Some(hover_style) = context_separator_hover_style {
            layers.push(TextAreaRangeLayer::single(
                (visible_byte_start, visible_byte_end),
                hover_style,
                TEXT_AREA_LAYER_PRIORITY_SELECTION.saturating_sub(1),
                TextAreaLayerKind::PublicDecoration,
            ));
            if hover_style.bg.is_some() {
                full_row_bg = Some(Style {
                    bg: hover_style.bg,
                    ..Style::default()
                });
            }
        }
        let raw_spans = resolved_segments_to_spans(resolve_text_area_spans(segments, &layers));
        let row_inline_virtuals = if placeholder_active || virtual_texts.is_empty() {
            Vec::new()
        } else {
            inline_virtual_texts_for_visual_line(value, virtual_texts, &cache_line)
        };

        // Substitute inline image sentinel characters with placeholder label spans.
        let effective_ph_style = if is_focused {
            image_placeholder_focus_style
        } else {
            image_placeholder_style
        };
        let hovered_byte = hover_mouse_pos.and_then(|(mx, my)| {
            let mx = mx as i16;
            let my = my as i16;
            let right = content_x.saturating_add(content_rect_width as i16);
            if my != y_pos || mx < content_x || mx >= right {
                return None;
            }
            let col = mx.saturating_sub(content_x) as usize;
            if has_virtual_text {
                let logical_line_start = value[..vline.start].rfind('\n').map_or(0, |i| i + 1);
                let logical_line_end = value[logical_line_start..]
                    .find('\n')
                    .map(|i| logical_line_start + i)
                    .unwrap_or(value.len());
                let insertions = inline_virtual_insertions_for_line(
                    value,
                    virtual_texts,
                    logical_line_start,
                    logical_line_end,
                );
                let line_col = if wrap {
                    cache_line.visual_start_col.saturating_add(col)
                } else {
                    col.saturating_add(h_scroll_start)
                };
                let line_col = line_col.min(cache_line.visual_end_col);
                let byte_in_line = util::byte_at_col_sentinel_tabs_virtual(
                    &value[logical_line_start..logical_line_end],
                    line_col,
                    sentinel.as_ref(),
                    tab_stop,
                    &insertions,
                );
                Some(logical_line_start.saturating_add(byte_in_line))
            } else {
                let byte_in_visible =
                    util::byte_at_col_sentinel(visible_text, col, sentinel.as_ref());
                visible_byte_start.checked_add(byte_in_visible)
            }
        });
        let mut spans: Vec<Span<'static>> = substitute_sentinels(
            raw_spans,
            if inline_mode { images_count } else { 0 },
            image_placeholder,
            effective_ph_style,
            image_placeholder_hover_style,
            SubstituteSentinelsCtx {
                byte_start: visible_byte_start,
                hovered_byte,
                selection_range,
                selection_style: to_ratatui_style(selection_style),
                custom_sentinels: sentinels,
                inline_virtuals: row_inline_virtuals,
                is_focused,
            },
        );

        if let Some((sel_start, sel_end)) = selection_range
            && sel_start <= vline.end
            && sel_end > vline.end
            && vline.end < value.len()
            && value.as_bytes()[vline.end] == b'\n'
            && vline.text.is_empty()
            && !selection_excluded_lines
                .is_some_and(|excl| excl.contains(&vline.line_num.saturating_sub(1)))
        {
            spans.push(Span::styled(" ", to_ratatui_style(selection_style)));
        }

        if !placeholder_active {
            let is_last_visual_for_logical_line = visual_idx + 1 == visual_lines.len()
                || visual_lines[visual_idx + 1].line_num != vline.line_num;
            for vt in eol_virtual_texts_for_visual_line(
                value,
                virtual_texts,
                &cache_line,
                is_last_visual_for_logical_line,
            ) {
                spans.extend(virtual_text_spans_to_ratatui(vt));
            }
        }

        let cdx = (clipped_content.x as i32)
            .saturating_sub(content_rect.x as i32)
            .max(0) as u16;
        let cdy = (clipped_content.y as i32)
            .saturating_sub(content_rect.y as i32)
            .max(0) as u16;
        if let Some(bg_style) = full_row_bg {
            f.render_widget(
                Block::default().style(to_ratatui_style(bg_style)),
                clipped_content_bg,
            );
        }
        let spans = expand_tabs_in_spans(spans, visible_start_col, tab_stop);
        let paragraph_scroll_x = if !wrap && has_virtual_text {
            cdx.saturating_add(h_scroll_start.min(u16::MAX as usize) as u16)
        } else {
            cdx
        };
        f.render_widget(
            Paragraph::new(Line::from(spans)).scroll((cdy, paragraph_scroll_x)),
            clipped_content,
        );

        if let (Some((target_start, _)), Some(count_suffix)) =
            (current_search_range, current_search_count_suffix.as_deref())
            && target_start >= visible_byte_start
            && target_start < visible_byte_end
        {
            let label = count_suffix.to_owned();
            let label_width = unicode_width::UnicodeWidthStr::width(label.as_str());
            let visible_text_width = str_visual_width_with_tabs(
                visible_text,
                sentinel.as_ref(),
                visible_start_col,
                tab_stop,
            );
            if label_width > 0
                && visible_text_width
                    .saturating_add(1)
                    .saturating_add(label_width)
                    <= usize::from(content_rect_width)
            {
                let suffix_rect = ratatui::layout::Rect {
                    x: content_rect
                        .x
                        .saturating_add(visible_text_width.saturating_add(1) as u16),
                    y: content_rect.y,
                    width: label_width as u16,
                    height: 1,
                };
                let clipped_suffix = suffix_rect.intersection(rrect);
                if clipped_suffix.width > 0 {
                    let sdx = clipped_suffix.x.saturating_sub(suffix_rect.x);
                    f.render_widget(
                        Paragraph::new(label)
                            .style(to_ratatui_style(vim_search_bar_count_style))
                            .scroll((0, sdx)),
                        clipped_suffix,
                    );
                }
            }
        }
    }

    if let (Some(feedback), Some(y)) = (pending_search_feedback, search_bar_y) {
        let search_rect = ratatui::layout::Rect {
            x: inner.x.max(0) as u16,
            y: y.max(0) as u16,
            width: inner.w,
            height: 1,
        }
        .intersection(rrect);
        if search_rect.width > 0 {
            let prompt = search_bar_line(
                feedback,
                search_rect.width as usize,
                vim_search_bar_style,
                vim_search_bar_prefix_style,
                vim_search_bar_count_style,
            );
            f.render_widget(
                Block::default().style(to_ratatui_style(vim_search_bar_style)),
                search_rect,
            );
            f.render_widget(
                Paragraph::new(prompt).style(to_ratatui_style(vim_search_bar_style)),
                search_rect,
            );
        }
    }

    // 4. Render Cursor
    if is_focused && !disabled && !read_only {
        if let (Some(feedback), Some(y)) = (pending_search_feedback, search_bar_y) {
            let prompt_width = inner.w;
            if prompt_width > 0 {
                let cx = inner
                    .x
                    .saturating_add(search_bar_cursor_x(feedback, prompt_width) as i16);
                let cy = y;
                if cx >= rrect.x as i16
                    && cx < (rrect.x as i32 + rrect.width as i32) as i16
                    && cy >= rrect.y as i16
                    && cy < (rrect.y as i32 + rrect.height as i32) as i16
                {
                    let position = ratatui::layout::Position::new(cx as u16, cy as u16);
                    f.set_cursor_position(position);
                    remember_cursor_position(cursor_sink, position);
                }
            }
        } else {
            let render_cursor = vim_visual_line_caret.unwrap_or(cursor).min(value.len());
            let mut cursor_x = 0;
            let mut cursor_visual_line_idx = 0;
            let mut found = false;

            // Find which visual line contains the cursor
            for (idx, vline) in visual_lines.iter().enumerate() {
                if visual_line_contains_cursor(&visual_lines, idx, render_cursor) {
                    // Cursor X relative to the visual line's left edge. For
                    // wrapped continuation lines, tab stops align to the logical
                    // line start, so we pass the visual column of the visual line
                    // start as the measurement origin.
                    let logical_line_start = value[..vline.start].rfind('\n').map_or(0, |i| i + 1);
                    let logical_line_end = value[logical_line_start..]
                        .find('\n')
                        .map(|i| logical_line_start + i)
                        .unwrap_or(value.len());
                    let insertions = inline_virtual_insertions_for_line(
                        value,
                        virtual_texts,
                        logical_line_start,
                        logical_line_end,
                    );
                    let logical_cursor_col = visual_col_with_virtual(
                        &value[logical_line_start..render_cursor],
                        0,
                        tab_stop,
                        sentinel.as_ref(),
                        &insertions,
                    );
                    let full_cursor_x = if wrap {
                        logical_cursor_col.saturating_sub(vline.visual_start_col)
                    } else {
                        logical_cursor_col
                    }
                    .min(u16::MAX as usize) as u16;
                    // Adjust for horizontal scroll when wrap is disabled
                    // If cursor is out of view (left or right), we shouldn't render it
                    let is_visible_horizontally = if !wrap {
                        full_cursor_x >= h_scroll_start as u16
                            && (full_cursor_x as usize)
                                <= h_scroll_start + content_rect_width as usize
                    } else {
                        true
                    };

                    if is_visible_horizontally {
                        cursor_x = if !wrap {
                            full_cursor_x.saturating_sub(h_scroll_start as u16)
                        } else {
                            full_cursor_x
                        };
                        cursor_visual_line_idx = idx;
                        found = true;
                    }
                    break;
                }
            }

            if found
                && cursor_visual_line_idx >= scroll_offset
                && cursor_visual_line_idx < scroll_offset + editor_viewport_h as usize
            {
                let cursor_y = (cursor_visual_line_idx - scroll_offset) as i16;
                let cx = content_x.saturating_add(cursor_x as i16);
                let cy = content_inner.y.saturating_add(cursor_y);

                // Only show cursor if there's no selection, unless the caller opts into
                // Vim-style modal selections where the caret remains visible.
                let has_selection = selection_range.map(|(s, e)| s != e).unwrap_or(false);

                if (show_cursor_with_selection || blink_visible && !has_selection)
                    && cx >= rrect.x as i16
                    && cx < (rrect.x as i32 + rrect.width as i32) as i16
                    && cy >= rrect.y as i16
                    && cy < (rrect.y as i32 + rrect.height as i32) as i16
                {
                    let position = ratatui::layout::Position::new(cx as u16, cy as u16);
                    f.set_cursor_position(position);
                    remember_cursor_position(cursor_sink, position);
                }
            }
        }
    }

    // 5. Render Scrollbar (if enabled and needed)
    if scrollbar
        && editor_viewport_h > 0
        && geometry.total_visual_lines > editor_viewport_h as usize
    {
        let total_lines = geometry.total_visual_lines;
        let visible_count = editor_viewport_h as usize;
        // Check if we should use integrated style (in border) or standalone
        let use_integrated = matches!(scrollbar_variant, ScrollbarVariant::Integrated);

        if use_integrated && (border || parent_integrated_v.is_some()) {
            // Integrated scrollbar (overwrites right border or parent frame edge / decoration)
            let sb_x = parent_integrated_v
                .map(|p| p.track_x)
                .unwrap_or_else(|| rect.x.saturating_add(rect.w.saturating_sub(1) as i16));
            let sb_rect = Rect {
                x: sb_x,
                y: content_inner.y,
                w: 1,
                h: editor_viewport_h,
            };

            let thumb = scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
            let thumb_style = resolve_scrollbar_thumb_style(
                is_focused,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
            );

            let b_style = parent_integrated_v
                .map(|p| p.border_style_fallback)
                .unwrap_or(border_style);
            let track_glyph = parent_integrated_v.and_then(|p| p.track_glyph);
            let mut v_scratch = [0u8; 4];
            let border_char =
                integrated_vscrollbar_track_char(track_glyph, b_style, &mut v_scratch);
            let integrated_base_style = parent_integrated_v
                .map(|p| p.track_style)
                .unwrap_or(chrome_style);

            render_integrated_scrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset: scroll_offset,
                    visible: visible_count,
                    total: total_lines,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: thumb,
                    border_char,
                    base_style: integrated_base_style,
                    thumb_style,
                    track_style: scrollbar_track_style,
                    clip_rect: None,
                    metrics_cache,
                },
            );
        } else {
            // Standalone scrollbar (takes space in content area)
            let sb_rect = Rect {
                x: inner
                    .x
                    .saturating_add(inner.w.saturating_sub(1) as i16)
                    .max(0),
                y: content_inner.y,
                w: 1,
                h: editor_viewport_h,
            };

            let thumb = scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
            let thumb_style = resolve_scrollbar_thumb_style(
                is_focused,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
            );

            render_vscrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset: scroll_offset,
                    visible: visible_count,
                    total: total_lines,
                },
                ScrollbarAppearance {
                    thumb_char: thumb,
                    thumb_style,
                    track_style: scrollbar_track_style,
                    clip_rect: clip_rrect,
                    metrics_cache,
                },
            );
        }
    }

    // 6. Render Horizontal Scrollbar (if enabled and needed)
    if !search_bar_active && geometry.h_scrollbar_visible && geometry.content_width > 0 {
        let total_cols = geometry.max_line_width;
        let visible_cols = geometry.content_width;
        if total_cols > visible_cols {
            let offset = h_scroll_start.min(total_cols.saturating_sub(visible_cols));
            let use_integrated = matches!(h_scrollbar_variant, ScrollbarVariant::Integrated);
            let thumb = h_scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
            let thumb_style = resolve_scrollbar_thumb_style(
                is_focused,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
            );

            if use_integrated && (border || parent_integrated_h.is_some()) {
                let sb_y = parent_integrated_h
                    .map(|p| p.track_y)
                    .unwrap_or_else(|| rect.y.saturating_add(rect.h.saturating_sub(1) as i16));
                let sb_rect = Rect {
                    x: content_x,
                    y: sb_y,
                    w: content_width as u16,
                    h: 1,
                };

                let b_style = parent_integrated_h
                    .map(|p| p.border_style_fallback)
                    .unwrap_or(border_style);
                let track_glyph = parent_integrated_h.and_then(|p| p.track_glyph);
                let mut h_scratch = [0u8; 4];
                let border_char =
                    integrated_hscrollbar_track_char(track_glyph, b_style, &mut h_scratch);
                let integrated_base_style = parent_integrated_h
                    .map(|p| p.track_style)
                    .unwrap_or(chrome_style);

                render_integrated_hscrollbar(
                    f,
                    sb_rect,
                    ScrollbarScrollState {
                        offset,
                        visible: visible_cols,
                        total: total_cols,
                    },
                    IntegratedScrollbarAppearance {
                        thumb_char: thumb,
                        border_char,
                        base_style: integrated_base_style,
                        thumb_style,
                        track_style: scrollbar_track_style,
                        clip_rect: None,
                        metrics_cache,
                    },
                );
            } else {
                let sb_rect = Rect {
                    x: content_x,
                    y: content_inner.y.saturating_add(content_inner.h as i16),
                    w: content_width as u16,
                    h: 1,
                };

                render_hscrollbar(
                    f,
                    sb_rect,
                    ScrollbarScrollState {
                        offset,
                        visible: visible_cols,
                        total: total_cols,
                    },
                    ScrollbarAppearance {
                        thumb_char: thumb,
                        thumb_style,
                        track_style: scrollbar_track_style,
                        clip_rect: clip_rrect,
                        metrics_cache,
                    },
                );
            }
        }
    }
}

pub(crate) fn render_text_area_node(
    state: &mut RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    ta: &crate::widgets::internal::TextAreaNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node.id) == state.ctx.focused && !ta.disabled;
    let is_hovered = Some(node.id) == state.ctx.hovered && !ta.disabled;
    let contrast_policy = state.ctx.contrast_policy;
    let theme = node.active_theme();
    let style = resolve_base_style(theme, ta.style);
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &ta.hover_style);
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &ta.focus_style);
    let focus_content_style =
        resolve_focus_style_defaults(theme, ta.focus_content_style, theme.text_area.focus);
    let disabled_style = resolve_muted_style(theme, ta.disabled_style);
    let placeholder_style = resolve_muted_style(theme, ta.placeholder_style);
    let focus_placeholder_style = resolve_muted_style(theme, ta.focus_placeholder_style);
    let line_number_style = resolve_muted_style(theme, ta.line_number_style);
    let image_placeholder_style = resolve_muted_style(theme, ta.image_placeholder_style);
    let image_placeholder_focus_style =
        resolve_muted_style(theme, ta.image_placeholder_focus_style);
    let image_placeholder_hover_style = ta.image_placeholder_hover_style;
    let copy_feedback_active = state
        .ctx
        .copy_feedback
        .is_some_and(|feedback| feedback.is_active(node.id));
    let selection_style = apply_copy_feedback_to_selection_style(
        state.ctx,
        node.id,
        resolve_text_area_selection_slot(
            theme,
            &ta.selection_style,
            &ta.unfocused_selection_style,
            is_focused,
        ),
    );
    let (scrollbar_thumb_style, scrollbar_thumb_focus_style, scrollbar_track_style) =
        resolve_scrollbar_theme(
            theme,
            ta.scrollbar_thumb_style,
            ta.scrollbar_thumb_focus_style,
            ta.scrollbar_track_style,
        );
    let chrome_base = finalize_style(
        resolve_interactive_style_raw(
            style,
            focus_style,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            ta.disabled,
        ),
        None,
        contrast_policy,
    );
    let content_focus = if is_focused {
        focus_content_style
    } else {
        Style::default()
    };
    let content_base = finalize_style(
        resolve_interactive_style_raw(
            style,
            content_focus,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            ta.disabled,
        ),
        None,
        contrast_policy,
    );
    let ph_focus = if is_focused {
        focus_placeholder_style
    } else {
        Style::default()
    };
    let placeholder_base = finalize_style(
        resolve_interactive_style_raw(
            placeholder_style,
            ph_focus,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            ta.disabled,
        ),
        None,
        contrast_policy,
    );
    let vim_search_bar_style = finalize_style(
        resolve_chrome_relative_slot(chrome_base, &ta.vim_config.search_bar_style),
        style_backdrop(chrome_base),
        contrast_policy,
    );
    let vim_search_bar_prefix_style = finalize_style(
        resolve_search_bar_part_slot(vim_search_bar_style, &ta.vim_config.search_bar_prefix_style),
        style_backdrop(vim_search_bar_style),
        contrast_policy,
    );
    let vim_search_bar_count_style = finalize_style(
        resolve_search_bar_part_slot(vim_search_bar_style, &ta.vim_config.search_bar_count_style),
        style_backdrop(vim_search_bar_style),
        contrast_policy,
    );
    let vim_search_match_style = finalize_style(
        resolve_slot(
            theme,
            ThemeRole::TextSelection,
            &ta.vim_config.search_match_style,
        ),
        style_backdrop(content_base),
        contrast_policy,
    );
    let vim_current_search_match_style = finalize_style(
        resolve_slot(
            theme,
            ThemeRole::TextSelection,
            &ta.vim_config.current_search_match_style,
        ),
        style_backdrop(content_base),
        contrast_policy,
    );
    let vim_current_line_style = finalize_style(
        resolve_slot(
            theme,
            ThemeRole::ItemHover,
            &ta.vim_config.current_line_style,
        ),
        style_backdrop(content_base),
        contrast_policy,
    );
    let vim_current_line_number_style = finalize_style(
        resolve_current_line_number_slot(
            vim_current_line_style,
            &ta.vim_config.current_line_number_style,
        ),
        style_backdrop(vim_current_line_style),
        contrast_policy,
    );
    let (effective_cursor, effective_anchor) = if ta.read_only && ta.on_change.is_none() {
        state
            .ctx
            .read_only_selection
            .and_then(|m| m.get(&node.id))
            .map(|(c, a)| (*c, *a))
            .unwrap_or((ta.cursor, ta.anchor))
    } else {
        (ta.cursor, ta.anchor)
    };

    let parent_tracks = if !ta.border
        && ((ta.scrollbar && matches!(ta.scrollbar_variant, ScrollbarVariant::Integrated))
            || (ta.h_scrollbar && matches!(ta.h_scrollbar_variant, ScrollbarVariant::Integrated)))
    {
        ancestor_frame_integrated_tracks(state, node.parent)
    } else {
        None
    };
    let parent_integrated_v = parent_tracks.and_then(|t| t.v);
    let parent_integrated_h = parent_tracks.and_then(|t| t.h);

    // When pin_scrollbar_focus is set (single-scrollbar DiffView), force the
    // normal thumb style to match the focus style when any sibling pane in the
    // parent container is focused.  Walk up 2 levels (TextArea → Frame → HStack)
    // and check if that ancestor is in the focus chain.
    #[allow(unused_mut)]
    let mut scrollbar_thumb_style = scrollbar_thumb_style;
    #[cfg(feature = "diff-view")]
    if !is_focused && ta.pin_scrollbar_focus {
        let container_focused = node
            .parent
            .and_then(|frame_id| state.ctx.tree.node(frame_id).parent)
            .is_some_and(|hstack_id| state.focus_chain.contains(&hstack_id));
        if container_focused {
            scrollbar_thumb_style = scrollbar_thumb_focus_style.or(scrollbar_thumb_style);
        }
    }

    let scrollbar_cache = &state.ctx.scrollbar_metrics_cache;
    let f = &mut *state.f;
    render_text_area(
        f,
        TextAreaContentRenderCtx {
            value: &ta.value,
            cursor: effective_cursor,
            anchor: effective_anchor,
            show_cursor_with_selection: ta.vim_motions,
            placeholder: ta.placeholder.as_deref(),
        },
        TextAreaRenderParts {
            vim: TextAreaVimRenderCtx {
                search_feedback: ta.vim_search_feedback.as_ref(),
                config: &ta.vim_config,
                mode: ta.vim_mode,
                visual_line_caret: ta.vim_visual_line_caret,
                yank_feedback_range: copy_feedback_active
                    .then_some(ta.vim_yank_feedback_range)
                    .flatten(),
                search_bar_style: vim_search_bar_style,
                search_bar_prefix_style: vim_search_bar_prefix_style,
                search_bar_count_style: vim_search_bar_count_style,
                search_match_style: vim_search_match_style,
                current_search_match_style: vim_current_search_match_style,
                current_line_style: vim_current_line_style,
                current_line_number_style: vim_current_line_number_style,
            },
            chrome: TextAreaChromeRenderCtx {
                chrome_style: chrome_base,
                content_style: content_base,
                hover_border_style: ta.hover_border_style,
                selection_style: finalize_text_area_selection_style(
                    content_base,
                    selection_style,
                    contrast_policy,
                ),
                placeholder_style: placeholder_base,
                line_numbers: ta.line_numbers,
                line_number_mode: ta.line_number_mode,
                line_number_style: finalize_style(
                    chrome_base.patch(line_number_style),
                    style_backdrop(chrome_base),
                    contrast_policy,
                ),
                min_line_number_width: ta.min_line_number_width,
                wrap: ta.wrap,
                border: ta.border,
                border_style: ta.border_style,
                padding: ta.padding,
            },
            scroll: TextAreaScrollRenderCtx {
                scroll_offset: ta.scroll_offset,
                scrollbar: ta.scrollbar,
                scrollbar_variant: ta.scrollbar_variant,
                scrollbar_gap: ta.scrollbar_gap,
                scrollbar_thumb: ta.scrollbar_thumb,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
                scrollbar_track_style,
                h_scrollbar_variant: ta.h_scrollbar_variant,
                h_scrollbar_thumb: ta.h_scrollbar_thumb,
                h_scroll_offset: ta.h_scroll_offset,
                value_hash: ta.content_hash,
                peer_hash: hash_peer_source_lines(ta.peer_source_lines.as_ref()),
                visual_cache: &ta.visual_cache,
                color_cache: &ta.color_cache,
                metrics_cache: Some(scrollbar_cache),
                parent_integrated_v,
                parent_integrated_h,
            },
            interaction: TextAreaInteractionRenderCtx {
                is_focused,
                show_selection_when_unfocused: ta.show_selection_when_unfocused,
                is_hovered,
                blink_visible: state.ctx.blink_visible,
                disabled: ta.disabled,
                read_only: ta.read_only,
                cursor_sink: Some(state.ctx.cursor_position),
            },
            layout: TextAreaLayoutRenderCtx {
                rect,
                rrect,
                clip_rect: clip_bounds,
            },
            extras: TextAreaExtrasRenderCtx {
                images_count: ta.images.len(),
                image_mode: ta.image_mode,
                image_placeholder: &ta.image_placeholder,
                image_placeholder_style: finalize_style(
                    chrome_base.patch(image_placeholder_style),
                    style_backdrop(chrome_base),
                    contrast_policy,
                ),
                image_placeholder_focus_style: finalize_style(
                    chrome_base.patch(image_placeholder_focus_style),
                    style_backdrop(chrome_base),
                    contrast_policy,
                ),
                image_placeholder_hover_style,
                sentinels: &ta.sentinels,
                gutter_lines: ta.gutter_lines.as_deref().map(|v| v.as_slice()),
                gutter_col_width: ta.gutter_col_width,
                gutter_gap: ta.gutter_gap,
                split_wrap_padding_gutter_style: ta.split_wrap_padding_gutter_style.map(|style| {
                    finalize_style(
                        chrome_base.patch(style),
                        style_backdrop(chrome_base),
                        contrast_policy,
                    )
                }),
                split_wrap_padding_style: ta.split_wrap_padding_style.map(|style| {
                    finalize_style(
                        chrome_base.patch(style),
                        style_backdrop(chrome_base),
                        contrast_policy,
                    )
                }),
                selection_excluded_lines: ta
                    .selection_excluded_lines
                    .as_deref()
                    .map(|v| v.as_slice()),
                decorations: ta.decorations.as_slice(),
                virtual_texts: ta.virtual_texts.as_slice(),
                geometry: &ta.geometry,
                #[cfg(feature = "diff-view")]
                diff_context_separator_click: ta.diff_context_separator_click.as_ref(),
                hover_mouse_pos: is_hovered.then_some(()).and(state.ctx.mouse_pos),
                tab_stop: ta.tab_display_width as usize,
            },
        },
    );
}

pub(crate) struct TextAreaCursorInput {
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub allow_selection_cursor: bool,
}

pub(crate) struct TextAreaCursorVimCtx<'a> {
    pub search_feedback: Option<&'a crate::widgets::TextAreaVimSearchFeedback>,
    pub visual_line_caret: Option<usize>,
}

pub(crate) struct TextAreaCursorLayout {
    pub wrap: bool,
    pub h_scroll_offset: usize,
    pub scroll_offset: usize,
    pub border: bool,
    pub padding: Padding,
    pub line_numbers: bool,
    pub min_line_number_width: u8,
    pub rect: Rect,
    pub clip_rect: Option<Rect>,
    pub parent_integrated_v_edge: bool,
    pub parent_integrated_h_edge: bool,
}

pub(crate) struct TextAreaCursorScrollCtx {
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub max_line_width: usize,
    pub read_only: bool,
}

pub(crate) struct TextAreaCursorExtrasCtx<'a> {
    pub visual_cache: &'a TextAreaVisualCache,
    pub value_hash: u64,
    pub peer_hash: u64,
    pub images_count: usize,
    pub image_mode: TextAreaImageMode,
    pub image_placeholder: &'a str,
    pub sentinels: &'a [crate::widgets::TextAreaSentinel],
    pub virtual_texts: &'a [TextAreaVirtualText],
    pub gutter_col_width: u16,
    pub gutter_gap: u16,
    pub geometry: &'a crate::widgets::TextAreaGeometry,
    pub tab_stop: usize,
}

pub(crate) fn text_area_cursor_position(
    value: &str,
    input: TextAreaCursorInput,
    vim: TextAreaCursorVimCtx<'_>,
    layout: TextAreaCursorLayout,
    scroll: TextAreaCursorScrollCtx,
    extras: TextAreaCursorExtrasCtx<'_>,
) -> Option<ratatui::layout::Position> {
    let TextAreaCursorInput {
        cursor,
        anchor,
        allow_selection_cursor,
    } = input;
    let TextAreaCursorVimCtx {
        search_feedback: vim_search_feedback,
        visual_line_caret: vim_visual_line_caret,
    } = vim;
    let TextAreaCursorLayout {
        wrap,
        h_scroll_offset,
        scroll_offset,
        border,
        padding,
        line_numbers,
        min_line_number_width,
        rect,
        clip_rect,
        parent_integrated_v_edge,
        parent_integrated_h_edge,
    } = layout;
    let TextAreaCursorScrollCtx {
        scrollbar,
        scrollbar_variant,
        scrollbar_gap,
        h_scrollbar: _h_scrollbar,
        h_scrollbar_variant,
        max_line_width: _max_line_width,
        read_only,
    } = scroll;
    let TextAreaCursorExtrasCtx {
        visual_cache,
        value_hash,
        peer_hash,
        images_count,
        image_mode,
        image_placeholder,
        sentinels,
        virtual_texts,
        gutter_col_width,
        gutter_gap,
        geometry,
        tab_stop,
    } = extras;
    if rect.w == 0 || rect.h == 0 {
        return None;
    }

    let sentinel =
        crate::widgets::sentinel_info_for(image_mode, images_count, image_placeholder, sentinels);

    let v_scrollbar_over_border = scrollbar
        && matches!(scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_integrated_v_edge);

    let mut chrome_rect = rect;
    if border {
        chrome_rect.x = chrome_rect.x.saturating_add(1);
        chrome_rect.w = chrome_rect.w.saturating_sub(2);
        chrome_rect.y = chrome_rect.y.saturating_add(1);
        chrome_rect.h = chrome_rect.h.saturating_sub(2);
    }

    let inner = chrome_rect.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return None;
    }

    let h_scrollbar_over_border = geometry.h_scrollbar_visible
        && matches!(h_scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_integrated_h_edge);

    let search_status_feedback = vim_search_feedback.filter(|_| inner.h > 0);
    let pending_search_feedback = search_status_feedback.filter(|feedback| feedback.pending);
    let search_bar_active = pending_search_feedback.is_some();

    let mut content_inner = inner;
    content_inner.h = if search_bar_active && !h_scrollbar_over_border {
        inner.h
    } else {
        geometry.content_viewport_h(h_scrollbar_over_border)
    };
    if content_inner.w == 0 || content_inner.h == 0 {
        return None;
    }
    let editor_viewport_h = if search_bar_active {
        content_inner.h.saturating_sub(1)
    } else {
        content_inner.h
    };
    if let Some(feedback) = pending_search_feedback {
        let prompt_width = inner.w;
        if prompt_width == 0 {
            return None;
        }
        let cx = inner
            .x
            .saturating_add(search_bar_cursor_x(feedback, prompt_width) as i16);
        let cy = content_inner.y.saturating_add(editor_viewport_h as i16);
        let clip = clip_rect.unwrap_or(rect);
        if cx < clip.x
            || cx >= clip.x.saturating_add(clip.w as i16)
            || cy < clip.y
            || cy >= clip.y.saturating_add(clip.h as i16)
        {
            return None;
        }
        return Some(ratatui::layout::Position::new(cx as u16, cy as u16));
    }

    let content_width = geometry.content_width;
    if content_width == 0 {
        return None;
    }

    let gutter_width = geometry.gutter_width;
    let content_x = content_inner.x.saturating_add(gutter_width as i16);
    let content_rect_width = content_width;
    let selection_range = anchor.map(|a| {
        let c = util::clamp_cursor(value, cursor.min(value.len()));
        let a = util::clamp_cursor(value, a.min(value.len()));
        (a.min(c), a.max(c))
    });
    if !allow_selection_cursor && selection_range.map(|(s, e)| s != e).unwrap_or(false) {
        return None;
    }

    let (sentinel_ph_width, sentinel_count) = sentinel
        .as_ref()
        .and_then(|si| si.image.map(|(_, _, pw)| (pw, images_count)))
        .unwrap_or((0, 0));
    let custom_sentinel_hash: u64 = {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        if let Some(si) = sentinel.as_ref()
            && let Some((_, _, ref widths, _)) = si.custom
        {
            widths.hash(&mut h);
        }
        h.finish()
    };
    let visual_key = make_text_area_visual_key(
        value_hash,
        peer_hash,
        TextAreaVisualKeyArgs {
            inner_w: geometry.inner_w,
            wrap,
            line_numbers,
            min_line_number_width,
            scrollbar,
            scrollbar_over_border: v_scrollbar_over_border,
            scrollbar_gap,
            read_only,
            cursor,
            tab_stop: tab_stop as u8,
            sentinel_ph_width,
            sentinel_count,
            custom_sentinel_hash,
            virtual_text_hash: text_area_virtual_text_hash(virtual_texts),
            gutter_col_width,
            gutter_gap,
            #[cfg(feature = "diff-view")]
            split_wrap_pane_widths: None,
            #[cfg(feature = "diff-view")]
            split_wrap_scrollbar_cols: None,
            #[cfg(feature = "diff-view")]
            split_wrap_layout_pass: 0,
        },
    );
    let visual_lines = build_visual_lines(
        value,
        content_width,
        wrap,
        (!read_only).then_some(cursor),
        visual_cache.get_lines(&visual_key),
        sentinel.as_ref(),
    );

    let mut cursor_x = 0u16;
    let mut cursor_visual_line_idx = 0usize;
    let mut found = false;

    let render_cursor = vim_visual_line_caret.unwrap_or(cursor).min(value.len());

    for (idx, vline) in visual_lines.iter().enumerate() {
        if visual_line_contains_cursor(&visual_lines, idx, render_cursor) {
            let logical_line_start = value[..vline.start].rfind('\n').map_or(0, |i| i + 1);
            let logical_line_end = value[logical_line_start..]
                .find('\n')
                .map(|i| logical_line_start + i)
                .unwrap_or(value.len());
            let insertions = inline_virtual_insertions_for_line(
                value,
                virtual_texts,
                logical_line_start,
                logical_line_end,
            );
            let logical_cursor_col = visual_col_with_virtual(
                &value[logical_line_start..render_cursor],
                0,
                tab_stop,
                sentinel.as_ref(),
                &insertions,
            );
            let full_cursor_x = if wrap {
                logical_cursor_col.saturating_sub(vline.visual_start_col)
            } else {
                logical_cursor_col
            }
            .min(u16::MAX as usize) as u16;
            let is_visible_horizontally = if !wrap {
                full_cursor_x >= h_scroll_offset as u16
                    && (full_cursor_x as usize) <= h_scroll_offset + content_rect_width
            } else {
                true
            };

            if is_visible_horizontally {
                cursor_x = if !wrap {
                    full_cursor_x.saturating_sub(h_scroll_offset as u16)
                } else {
                    full_cursor_x
                };
                cursor_visual_line_idx = idx;
                found = true;
            }
            break;
        }
    }

    if !found {
        return None;
    }

    if cursor_visual_line_idx < scroll_offset
        || cursor_visual_line_idx >= scroll_offset + editor_viewport_h as usize
    {
        return None;
    }

    let cursor_y = (cursor_visual_line_idx - scroll_offset) as i16;
    let cx = content_x.saturating_add(cursor_x as i16);
    let cy = content_inner.y.saturating_add(cursor_y);
    let clip = clip_rect.unwrap_or(rect);
    if cx < clip.x
        || cx >= clip.x.saturating_add(clip.w as i16)
        || cy < clip.y
        || cy >= clip.y.saturating_add(clip.h as i16)
    {
        return None;
    }

    Some(ratatui::layout::Position::new(cx as u16, cy as u16))
}

pub(crate) struct InputCursorDecor<'a> {
    pub prefix: Option<&'a str>,
    pub suffix: Option<&'a str>,
    pub truncate_head: bool,
    pub mask: Option<char>,
}

pub(crate) struct InputCursorLayout {
    pub border: bool,
    pub padding: Padding,
    pub rect: Rect,
    pub clip_rect: Option<Rect>,
}

pub(crate) fn input_cursor_position(
    value: &str,
    cursor: usize,
    anchor: Option<usize>,
    decor: InputCursorDecor<'_>,
    layout: InputCursorLayout,
) -> Option<ratatui::layout::Position> {
    let InputCursorDecor {
        prefix,
        suffix,
        truncate_head,
        mask,
    } = decor;
    let InputCursorLayout {
        border,
        padding,
        rect,
        clip_rect,
    } = layout;
    if rect.w == 0 || rect.h == 0 {
        return None;
    }

    let mut inner = rect;
    if border {
        inner.x = inner.x.saturating_add(1);
        inner.w = inner.w.saturating_sub(2);
        inner.y = inner.y.saturating_add(1);
        inner.h = inner.h.saturating_sub(2);
    }
    inner = inner.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return None;
    }

    let line_orig = value.lines().next().unwrap_or("");
    let (line, cursor, anchor) = if let Some(m) = mask {
        let mut masked = String::with_capacity(line_orig.len());
        let mut new_cursor = 0;
        let mut new_anchor = anchor.map(|_| 0);

        for (i, _ch) in line_orig.char_indices() {
            if i < cursor {
                new_cursor += m.len_utf8();
            }
            if let Some(a) = anchor
                && i < a
            {
                new_anchor = Some(new_anchor.unwrap() + m.len_utf8());
            }
            masked.push(m);
        }
        if cursor >= line_orig.len() {
            new_cursor = masked.len();
        }
        if let Some(a) = anchor
            && a >= line_orig.len()
        {
            new_anchor = Some(masked.len());
        }
        (std::borrow::Cow::Owned(masked), new_cursor, new_anchor)
    } else {
        (std::borrow::Cow::Borrowed(line_orig), cursor, anchor)
    };

    let cursor = util::clamp_cursor(&line, cursor.min(line.len()));
    let selection = anchor.map(|a| {
        let a = util::clamp_cursor(&line, a.min(line.len()));
        (a.min(cursor), a.max(cursor))
    });
    if selection.map(|(s, e)| s != e).unwrap_or(false) {
        return None;
    }

    let prefix_w = prefix
        .map(unicode_width::UnicodeWidthStr::width)
        .unwrap_or(0) as u16;
    let suffix_w = suffix
        .map(unicode_width::UnicodeWidthStr::width)
        .unwrap_or(0) as u16;
    let content_w = inner.w.saturating_sub(prefix_w.saturating_add(suffix_w));
    if content_w == 0 {
        return None;
    }

    let text_w = unicode_width::UnicodeWidthStr::width(line.as_ref()) as u16;
    let needs_cursor_reserve = text_w >= content_w;
    let _visible_w = if needs_cursor_reserve {
        content_w.saturating_sub(1)
    } else {
        content_w
    };

    let cursor_x = if line.is_empty() {
        0
    } else if truncate_head {
        let (start, end, _) = viewport(&line, cursor, content_w);
        let left_hidden = start > 0;
        let right_hidden = end < line.len();
        let ellipsis_count = (left_hidden as u16) + (right_hidden as u16);

        if content_w <= ellipsis_count {
            0
        } else {
            let text_w = content_w.saturating_sub(ellipsis_count);
            let (_s2, _e2, c2) = viewport(&line, cursor, text_w);
            if left_hidden {
                c2.saturating_add(1)
            } else {
                c2
            }
        }
    } else {
        let start = input_viewport_start(&line, cursor, content_w);
        let prefix_slice = &line[start..cursor];
        unicode_width::UnicodeWidthStr::width(prefix_slice) as u16
    };

    let cx = inner
        .x
        .saturating_add(prefix_w as i16)
        .saturating_add(cursor_x as i16);
    let cy = inner.y;
    let clip = clip_rect.unwrap_or(rect);
    if cx < clip.x
        || cx >= clip.x.saturating_add(clip.w as i16)
        || cy < clip.y
        || cy >= clip.y.saturating_add(clip.h as i16)
    {
        return None;
    }

    Some(ratatui::layout::Position::new(cx as u16, cy as u16))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::style::Span as UiSpan;

    fn test_cursor_pos_ctx<'a>(
        cache: &'a TextAreaVisualCache,
        geometry: &'a crate::widgets::TextAreaGeometry,
        rect: Rect,
        max_line_width: usize,
        gutter_col_width: u16,
        line_numbers: bool,
    ) -> (
        TextAreaCursorLayout,
        TextAreaCursorScrollCtx,
        TextAreaCursorExtrasCtx<'a>,
    ) {
        (
            TextAreaCursorLayout {
                wrap: false,
                h_scroll_offset: 0,
                scroll_offset: 0,
                border: false,
                padding: Padding::default(),
                line_numbers,
                min_line_number_width: 0,
                rect,
                clip_rect: None,
                parent_integrated_v_edge: false,
                parent_integrated_h_edge: false,
            },
            TextAreaCursorScrollCtx {
                scrollbar: false,
                scrollbar_variant: ScrollbarVariant::Standalone,
                scrollbar_gap: 0,
                h_scrollbar: false,
                h_scrollbar_variant: ScrollbarVariant::Standalone,
                max_line_width,
                read_only: false,
            },
            TextAreaCursorExtrasCtx {
                visual_cache: cache,
                value_hash: 0,
                peer_hash: 0,
                images_count: 0,
                image_mode: TextAreaImageMode::Inline,
                image_placeholder: "[image X]",
                sentinels: &[],
                virtual_texts: &[],
                gutter_col_width,
                gutter_gap: 0,
                geometry,
                tab_stop: 4,
            },
        )
    }

    #[test]
    fn test_wrapping_punctuation_glue() {
        let text = "hello word.";
        let width = 10;
        let lines = calculate_visual_lines(text, width, true, None, None);

        // Debug output
        for (i, line) in lines.iter().enumerate() {
            println!("Line {}: '{}'", i, line.text);
        }

        assert_eq!(lines.len(), 2, "Should wrap into 2 lines");
        assert_eq!(lines[0].text, "hello ", "First line should break at space");
        assert_eq!(
            lines[1].text, "word.",
            "Second line should contain glued word and punctuation"
        );
    }

    #[test]
    fn full_row_bg_prefers_empty_anchor_span() {
        let spans = vec![
            UiSpan::new("1 + ").style(Style::new().bg(crate::style::Color::rgb(40, 40, 40))),
            UiSpan::new("").style(Style::new().bg(crate::style::Color::rgb(10, 20, 30))),
            UiSpan::new("changed").style(Style::new().bg(crate::style::Color::rgb(80, 10, 10))),
        ];

        let bg = full_row_bg_from_spans(&spans).expect("expected background style");
        assert_eq!(
            bg.bg,
            Some(crate::style::Paint::Solid(crate::style::Color::rgb(
                10, 20, 30
            )))
        );
    }

    #[test]
    fn full_row_bg_requires_anchor_span() {
        let spans = vec![
            UiSpan::new("x").style(Style::new().bg(crate::style::Color::rgb(80, 10, 10))),
            UiSpan::new("long context text")
                .style(Style::new().bg(crate::style::Color::rgb(10, 20, 30))),
        ];

        assert!(full_row_bg_from_spans(&spans).is_none());
    }

    #[test]
    fn selection_style_contrast_uses_content_foreground_and_backdrop() {
        let content_base = Style::new()
            .fg(crate::style::Color::White)
            .bg(crate::style::Color::rgb(20, 20, 24));
        let selection_style = Style::new()
            .bg_alpha(crate::style::Color::rgb(80, 160, 255), 0.35)
            .contrast_policy(crate::app::ContrastPolicy::Apca);

        let resolved = finalize_text_area_selection_style(
            content_base,
            selection_style,
            crate::app::ContrastPolicy::Wcag,
        );

        assert_eq!(
            resolved.fg,
            Some(crate::style::Paint::Solid(crate::style::Color::White))
        );
        assert_eq!(
            resolved.bg,
            Some(crate::style::Paint::rgba(80, 160, 255, 89))
        );
    }

    #[test]
    fn gutter_inset_preserves_custom_gutter_style() {
        let style = Style::new().bg(crate::style::Color::rgb(10, 20, 30));
        let spans = vec![UiSpan::new("1 +").style(style)];

        let rendered = gutter_spans_with_inset(&spans, 2);

        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].content.as_ref(), "  ");
        assert_eq!(rendered[0].style.bg, to_ratatui_style(style).bg);
    }

    #[test]
    fn cursor_position_hides_non_vim_selection_but_allows_vim_selection() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 10,
            inner_h: 3,
            content_width: 10,
            total_visual_lines: 1,
            viewport_height: 3,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 3,
        };

        let (layout, scroll, extras) = test_cursor_pos_ctx(&cache, &geometry, rect, 3, 0, false);

        let non_vim = text_area_cursor_position(
            "abc",
            TextAreaCursorInput {
                cursor: 1,
                anchor: Some(0),
                allow_selection_cursor: false,
            },
            TextAreaCursorVimCtx {
                search_feedback: None,
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );
        let (layout, scroll, extras) = test_cursor_pos_ctx(&cache, &geometry, rect, 3, 0, false);
        let vim = text_area_cursor_position(
            "abc",
            TextAreaCursorInput {
                cursor: 1,
                anchor: Some(0),
                allow_selection_cursor: true,
            },
            TextAreaCursorVimCtx {
                search_feedback: None,
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(non_vim, None);
        assert_eq!(vim, Some(ratatui::layout::Position::new(1, 0)));
    }

    #[test]
    fn cursor_position_uses_visual_line_caret_override() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 10,
            inner_h: 3,
            content_width: 10,
            total_visual_lines: 2,
            viewport_height: 3,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 3,
        };

        let (layout, scroll, extras) = test_cursor_pos_ctx(&cache, &geometry, rect, 7, 0, false);
        let position = text_area_cursor_position(
            "abcd\nxy",
            TextAreaCursorInput {
                cursor: 7,
                anchor: Some(0),
                allow_selection_cursor: true,
            },
            TextAreaCursorVimCtx {
                search_feedback: None,
                visual_line_caret: Some(7),
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(position, Some(ratatui::layout::Position::new(2, 1)));
    }

    #[test]
    fn cursor_position_places_wrap_boundary_on_continuation_start() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 6,
            inner_h: 2,
            content_width: 5,
            total_visual_lines: 2,
            viewport_height: 2,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 2,
        };

        let (mut layout, scroll, extras) =
            test_cursor_pos_ctx(&cache, &geometry, rect, 8, 0, false);
        layout.wrap = true;
        let position = text_area_cursor_position(
            "abcdefgh",
            TextAreaCursorInput {
                cursor: 5,
                anchor: None,
                allow_selection_cursor: false,
            },
            TextAreaCursorVimCtx {
                search_feedback: None,
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(position, Some(ratatui::layout::Position::new(0, 1)));
    }

    #[test]
    fn cursor_position_places_visible_wrap_break_on_continuation_start() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 6,
            inner_h: 2,
            content_width: 5,
            total_visual_lines: 2,
            viewport_height: 2,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 2,
        };

        let (mut layout, scroll, extras) =
            test_cursor_pos_ctx(&cache, &geometry, rect, 9, 0, false);
        layout.wrap = true;
        let position = text_area_cursor_position(
            "abcd-efgh",
            TextAreaCursorInput {
                cursor: 5,
                anchor: None,
                allow_selection_cursor: false,
            },
            TextAreaCursorVimCtx {
                search_feedback: None,
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(position, Some(ratatui::layout::Position::new(0, 1)));
    }

    #[test]
    fn cursor_position_reserves_end_of_exactly_full_final_row() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 5,
            inner_h: 2,
            content_width: 5,
            total_visual_lines: 2,
            viewport_height: 2,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 2,
        };

        let (mut layout, scroll, extras) =
            test_cursor_pos_ctx(&cache, &geometry, rect, 5, 0, false);
        layout.wrap = true;
        let position = text_area_cursor_position(
            "abcde",
            TextAreaCursorInput {
                cursor: 5,
                anchor: None,
                allow_selection_cursor: false,
            },
            TextAreaCursorVimCtx {
                search_feedback: None,
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(position, Some(ratatui::layout::Position::new(1, 1)));
    }

    #[test]
    fn cursor_position_keeps_trailing_space_before_next_input() {
        for (value, expected_x) in [
            ("really long tex ", 4),
            ("really long tex d", 5),
            ("really long te ", 0),
            ("really long te t", 1),
        ] {
            let cache = TextAreaVisualCache::default();
            let geometry = crate::widgets::TextAreaGeometry {
                inner_w: 15,
                inner_h: 2,
                content_width: 15,
                total_visual_lines: 2,
                viewport_height: 2,
                ..Default::default()
            };
            let rect = Rect {
                x: 0,
                y: 0,
                w: 15,
                h: 2,
            };
            let (mut layout, scroll, extras) =
                test_cursor_pos_ctx(&cache, &geometry, rect, value.len(), 0, false);
            layout.wrap = true;

            let position = text_area_cursor_position(
                value,
                TextAreaCursorInput {
                    cursor: value.len(),
                    anchor: None,
                    allow_selection_cursor: false,
                },
                TextAreaCursorVimCtx {
                    search_feedback: None,
                    visual_line_caret: None,
                },
                layout,
                scroll,
                extras,
            );

            assert_eq!(
                position,
                Some(ratatui::layout::Position::new(expected_x, 1))
            );
        }
    }

    #[test]
    fn cursor_position_moves_to_pending_vim_search_bar() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 10,
            inner_h: 3,
            content_width: 10,
            total_visual_lines: 1,
            viewport_height: 3,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 3,
        };
        let feedback = crate::widgets::TextAreaVimSearchFeedback {
            query: Arc::from("al"),
            cursor: 2,
            forward: true,
            pending: true,
            target_range: Some((0, 2)),
            current_match_index: Some(1),
            match_count: 1,
        };

        let (layout, scroll, extras) = test_cursor_pos_ctx(&cache, &geometry, rect, 5, 0, false);
        let position = text_area_cursor_position(
            "alpha",
            TextAreaCursorInput {
                cursor: 0,
                anchor: None,
                allow_selection_cursor: true,
            },
            TextAreaCursorVimCtx {
                search_feedback: Some(&feedback),
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(position, Some(ratatui::layout::Position::new(6, 2)));
    }

    #[test]
    fn cursor_position_keeps_search_bar_when_content_width_collapses() {
        let cache = TextAreaVisualCache::default();
        let geometry = crate::widgets::TextAreaGeometry {
            inner_w: 4,
            inner_h: 2,
            gutter_width: 4,
            content_width: 0,
            total_visual_lines: 1,
            viewport_height: 2,
            ..Default::default()
        };
        let rect = Rect {
            x: 0,
            y: 0,
            w: 4,
            h: 2,
        };
        let feedback = crate::widgets::TextAreaVimSearchFeedback {
            query: Arc::from("a"),
            cursor: 1,
            forward: true,
            pending: true,
            target_range: Some((0, 1)),
            current_match_index: Some(1),
            match_count: 1,
        };

        let (layout, scroll, extras) = test_cursor_pos_ctx(&cache, &geometry, rect, 5, 4, true);
        let position = text_area_cursor_position(
            "alpha",
            TextAreaCursorInput {
                cursor: 0,
                anchor: None,
                allow_selection_cursor: true,
            },
            TextAreaCursorVimCtx {
                search_feedback: Some(&feedback),
                visual_line_caret: None,
            },
            layout,
            scroll,
            extras,
        );

        assert_eq!(position, Some(ratatui::layout::Position::new(3, 1)));
    }

    #[test]
    fn wrapped_custom_gutter_keeps_background_painted() {
        let style = Style::new().bg(crate::style::Color::rgb(10, 20, 30));
        let spans = vec![UiSpan::new("12 + ").style(style)];

        let rendered = blank_gutter_spans_with_inset(&spans, 7, 2, None);

        assert_eq!(rendered[0].content.as_ref(), "  ");
        assert_eq!(rendered[1].content.as_ref(), "     ");
        assert_eq!(rendered[0].style.bg, to_ratatui_style(style).bg);
        assert_eq!(rendered[1].style.bg, to_ratatui_style(style).bg);
    }

    #[test]
    fn wrapped_custom_gutter_override_style_replaces_source_style() {
        let source = Style::new().bg(crate::style::Color::rgb(10, 20, 30));
        let override_style = Style::new()
            .fg(crate::style::Color::DarkGray)
            .bg(crate::style::Color::rgb(30, 40, 50));
        let spans = vec![UiSpan::new("12 + ").style(source)];

        let rendered = blank_gutter_spans_with_inset(&spans, 7, 2, Some(override_style));

        assert_eq!(rendered[0].style.bg, to_ratatui_style(override_style).bg);
        assert_eq!(rendered[1].style.bg, to_ratatui_style(override_style).bg);
        assert_eq!(rendered[1].style.fg, to_ratatui_style(override_style).fg);
    }
}
