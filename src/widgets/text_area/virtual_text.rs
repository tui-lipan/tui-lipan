use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;
use unicode_width::UnicodeWidthStr;

use crate::style::Span;
use crate::utils::text::{self, VirtualTextInsertion};

use super::{TextAreaVirtualText, TextAreaVisualLine, VirtualTextPlacement};

pub(crate) fn sanitize_virtual_text_spans(spans: Vec<Span>) -> Vec<Span> {
    spans
        .into_iter()
        .filter_map(|mut span| {
            if span.content.contains('\n') || span.content.contains('\r') {
                let stripped = span
                    .content
                    .chars()
                    .filter(|ch| *ch != '\n' && *ch != '\r')
                    .collect::<String>();
                span.content = stripped.into();
            }
            (!span.content.is_empty()).then_some(span)
        })
        .collect()
}

pub(crate) fn virtual_text_content_width(vt: &TextAreaVirtualText) -> usize {
    vt.content
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

pub(crate) fn text_area_virtual_text_hash(virtual_texts: &[TextAreaVirtualText]) -> u64 {
    let mut hasher = FxHasher::default();
    virtual_texts.len().hash(&mut hasher);
    for vt in virtual_texts {
        vt.anchor.hash(&mut hasher);
        vt.placement.hash(&mut hasher);
        vt.priority.hash(&mut hasher);
        vt.content.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn normalized_anchor(value: &str, anchor: usize) -> usize {
    text::clamp_cursor(value, anchor.min(value.len()))
}

pub(crate) fn logical_line_bounds_for_anchor(value: &str, anchor: usize) -> (usize, usize, usize) {
    let anchor = normalized_anchor(value, anchor);
    let mut line_idx = 0usize;
    let mut line_start = 0usize;
    for segment in value.split_inclusive('\n') {
        let content_len = segment.strip_suffix('\n').map_or(segment.len(), str::len);
        let line_end = line_start.saturating_add(content_len);
        let total_end = line_start.saturating_add(segment.len());
        if anchor <= line_end || anchor < total_end {
            return (line_idx, line_start, line_end);
        }
        line_idx = line_idx.saturating_add(1);
        line_start = total_end;
    }
    (line_idx, value.len(), value.len())
}

pub(crate) fn inline_virtual_insertions_for_line(
    value: &str,
    virtual_texts: &[TextAreaVirtualText],
    line_start: usize,
    line_end: usize,
) -> Vec<VirtualTextInsertion> {
    let mut insertions = virtual_texts
        .iter()
        .enumerate()
        .filter_map(|(order, vt)| {
            (vt.placement == VirtualTextPlacement::Inline).then_some((order, vt))
        })
        .filter_map(|(order, vt)| {
            let anchor = normalized_anchor(value, vt.anchor);
            (anchor >= line_start && anchor <= line_end).then(|| VirtualTextInsertion {
                anchor: anchor.saturating_sub(line_start),
                width: virtual_text_content_width(vt),
                priority: vt.priority,
                order,
            })
        })
        .filter(|insertion| insertion.width > 0)
        .collect::<Vec<_>>();
    insertions.sort_by_key(|insertion| (insertion.anchor, insertion.priority, insertion.order));
    insertions
}

pub(crate) fn inline_virtual_texts_for_visual_line<'a>(
    value: &str,
    virtual_texts: &'a [TextAreaVirtualText],
    line: &TextAreaVisualLine,
) -> Vec<(usize, &'a TextAreaVirtualText)> {
    let mut out = virtual_texts
        .iter()
        .enumerate()
        .filter(|(_, vt)| vt.placement == VirtualTextPlacement::Inline)
        .filter_map(|(order, vt)| {
            let anchor = normalized_anchor(value, vt.anchor);
            let included = (anchor > line.start && anchor < line.end)
                || (anchor == line.start && line.starts_with_virtual_text)
                || (anchor == line.end && line.ends_with_virtual_text);
            included.then_some((order, anchor, vt))
        })
        .filter(|(_, _, vt)| virtual_text_content_width(vt) > 0)
        .collect::<Vec<_>>();
    out.sort_by_key(|(order, anchor, vt)| (*anchor, vt.priority, *order));
    out.into_iter()
        .map(|(_, anchor, vt)| (anchor, vt))
        .collect()
}

pub(crate) fn eol_virtual_texts_for_visual_line<'a>(
    value: &str,
    virtual_texts: &'a [TextAreaVirtualText],
    line: &TextAreaVisualLine,
    is_last_visual_for_logical_line: bool,
) -> Vec<&'a TextAreaVirtualText> {
    if !is_last_visual_for_logical_line {
        return Vec::new();
    }
    let mut out = virtual_texts
        .iter()
        .enumerate()
        .filter(|(_, vt)| vt.placement == VirtualTextPlacement::Eol)
        .filter_map(|(order, vt)| {
            let (_line_idx, line_start, line_end) =
                logical_line_bounds_for_anchor(value, vt.anchor);
            (line.start >= line_start && line.end <= line_end && line.end == line_end)
                .then_some((order, vt))
        })
        .filter(|(_, vt)| virtual_text_content_width(vt) > 0)
        .collect::<Vec<_>>();
    out.sort_by_key(|(order, vt)| (normalized_anchor(value, vt.anchor), vt.priority, *order));
    out.into_iter().map(|(_, vt)| vt).collect()
}
